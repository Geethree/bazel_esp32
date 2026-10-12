#![no_main]
#![no_std]
#![deny(clippy::mem_forget)]

use core::fmt::Write as _;

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::{IpListenEndpoint, Runner, Stack, StackResources, tcp::TcpSocket};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read, Write};
use esp_hal::{
    clock::CpuClock,
    efuse::{self, InterfaceMacAddress},
    rng::Rng,
    system::software_reset,
    timer::timg::TimerGroup,
    usb::usb_serial_jtag::UsbSerialJtag,
};
use esp_radio::wifi::{
    AuthenticationMethod, AuthenticationMethodConfig, Config as WifiConfig, ControllerConfig,
    Interface, Password, Ssid, WifiController, scan::ScanConfig, sta::StationConfig,
};
use heapless::Vec;
use hil_testing::{Frame, FrameError, FrameKind, HEADER_LEN, decode_frame, encode, encoded_len};
use static_cell::StaticCell;
use wifi_provisioning::{
    Authentication, CredentialStore, Credentials, FlashJournal, Network, PortalAction,
    PortalService, PortalStatus, hil::Control,
};

const _: () = assert!(wifi_provisioning::HIL_ENABLED);

esp_bootloader_esp_idf::esp_app_desc!();

macro_rules! mk_static {
    ($type:ty, $value:expr) => {{
        static CELL: StaticCell<$type> = StaticCell::new();
        CELL.uninit().write($value)
    }};
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    esp_println::println!("panic: {info}");
    loop {
        core::hint::spin_loop();
    }
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    let mut serial = UsbSerialJtag::new(peripherals.USB_DEVICE).into_async();
    let mut store = FlashJournal::new(
        esp_storage::FlashStorage::new(peripherals.FLASH),
        board::WIFI_CFG_OFFSET,
    );
    let mut portal = PortalService::new();
    let mut input = Vec::<u8, 2048>::new();

    let mut controller = WifiController::new(
        peripherals.WIFI,
        ControllerConfig::default()
            .with_initial_config(WifiConfig::Station(StationConfig::default())),
    )
    .unwrap();
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let (station, runner) = embassy_net::new(
        Interface::station(),
        embassy_net::Config::dhcpv4(Default::default()),
        mk_static!(StackResources<5>, StackResources::new()),
        seed,
    );
    spawner.spawn(network_runner(runner).unwrap());

    let mac = efuse::interface_mac_address(InterfaceMacAddress::Station);
    let health_token = mk_static!(heapless::String<24>, {
        let mut token = heapless::String::new();
        write!(
            token,
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            mac.as_bytes()[0],
            mac.as_bytes()[1],
            mac.as_bytes()[2],
            mac.as_bytes()[3],
            mac.as_bytes()[4],
            mac.as_bytes()[5]
        )
        .unwrap();
        token
    });
    spawner.spawn(health_server(station, health_token.as_str()).unwrap());

    refresh_networks(&mut controller, &mut portal).await;
    send_event(&mut serial, b"ready").await;
    let mut token_event = heapless::String::<40>::new();
    write!(token_event, "health-token:{}", health_token.as_str()).unwrap();
    send_event(&mut serial, token_event.as_bytes()).await;

    let mut stored_reconnected = false;
    if let Ok(Some(saved)) = store.load() {
        send_event(&mut serial, b"stored-reconnect-started").await;
        match connect_station(&mut controller, station, &saved).await {
            Ok(()) => {
                portal.set_status(PortalStatus::Connected);
                stored_reconnected = true;
                send_network_events(&mut serial, station, b"stored-reconnected").await;
            }
            Err(failure) => {
                portal.set_status(PortalStatus::Failed(
                    heapless::String::try_from("Stored network failed.").unwrap(),
                ));
                send_event(&mut serial, failure.as_bytes()).await;
                send_event(&mut serial, b"stored-reconnect-failed").await;
            }
        }
    }

    loop {
        let mut chunk = [0_u8; 64];
        let count = serial.read(&mut chunk).await.unwrap();
        if input.extend_from_slice(&chunk[..count]).is_err() {
            input.clear();
            send_event(&mut serial, b"frame-too-large").await;
            continue;
        }

        while input.len() >= HEADER_LEN {
            let frame_len = match encoded_len(&input) {
                Ok(frame_len) => frame_len,
                Err(FrameError::Incomplete) => break,
                Err(_) => {
                    discard_input(&mut input, 1);
                    send_event(&mut serial, b"bad-frame").await;
                    continue;
                }
            };

            match decode_frame(&input[..frame_len]) {
                Ok(Frame {
                    kind: FrameKind::Request,
                    payload: request,
                }) => {
                    let mut response = Vec::<u8, 2048>::new();
                    if let Ok(action) = portal.handle(request, &mut response) {
                        let mut framed = Vec::<u8, 2080>::new();
                        if encode(FrameKind::Response, &response, &mut framed).is_ok() {
                            let _ = serial.write_all(&framed).await;
                            let _ = serial.flush().await;
                        }
                        send_event(&mut serial, b"ready").await;
                        send_event(&mut serial, token_event.as_bytes()).await;
                        if stored_reconnected {
                            send_network_events(&mut serial, station, b"stored-reconnected").await;
                        }
                        if let PortalAction::Connect(credentials) = action {
                            if let Err(failure) =
                                connect_station(&mut controller, station, &credentials).await
                            {
                                portal.set_status(PortalStatus::Failed(
                                    heapless::String::try_from("Unable to join network.").unwrap(),
                                ));
                                send_event(&mut serial, failure.as_bytes()).await;
                            } else {
                                portal.set_status(PortalStatus::Connected);
                                if store.save(&credentials).is_ok() {
                                    send_network_events(&mut serial, station, b"credentials-saved")
                                        .await;
                                } else {
                                    send_event(&mut serial, b"storage-error").await;
                                }
                            }
                        }
                    } else {
                        send_event(&mut serial, b"http-error").await;
                    }
                }
                Ok(Frame {
                    kind: FrameKind::Control,
                    payload,
                }) => match Control::decode(payload) {
                    Ok(Control::ClearStorage) => {
                        if store.clear().is_ok() {
                            let _ = controller.disconnect_async().await;
                            stored_reconnected = false;
                            portal.set_status(PortalStatus::Ready);
                            refresh_networks(&mut controller, &mut portal).await;
                            send_event(&mut serial, b"storage-cleared").await;
                        } else {
                            send_event(&mut serial, b"storage-error").await;
                        }
                    }
                    Ok(Control::Reset) => {
                        let _ = select(
                            controller.disconnect_async(),
                            Timer::after(Duration::from_secs(3)),
                        )
                        .await;
                        Timer::after(Duration::from_secs(1)).await;
                        send_event(&mut serial, b"resetting").await;
                        Timer::after(Duration::from_millis(50)).await;
                        software_reset();
                    }
                    Err(_) => send_event(&mut serial, b"bad-control").await,
                },
                _ => send_event(&mut serial, b"bad-frame").await,
            }

            discard_input(&mut input, frame_len);
        }
    }
}

async fn refresh_networks(controller: &mut WifiController<'_>, portal: &mut PortalService) {
    if controller
        .set_config(&WifiConfig::Station(StationConfig::default()))
        .is_err()
    {
        return;
    }
    // Give the radio time to settle after initialization or a disconnect,
    // then sample long enough to catch APs that beacon intermittently.
    Timer::after(Duration::from_secs(1)).await;
    let mut networks = Vec::<Network, 16>::new();
    for attempt in 0..5 {
        if let Ok(scan) = controller
            .scan_async(&ScanConfig::default().with_max(16))
            .await
        {
            for network in scan {
                if let Ok(network) = Network::new(
                    network.ssid.as_str(),
                    network.signal_strength,
                    !matches!(network.auth_method, None | Some(AuthenticationMethod::None)),
                ) {
                    if let Some(existing) = networks
                        .iter_mut()
                        .find(|existing| existing.ssid == network.ssid)
                    {
                        if network.rssi > existing.rssi {
                            *existing = network;
                        }
                    } else if networks.len() < networks.capacity() {
                        let _ = networks.push(network);
                    } else if let Some((weakest, _)) = networks
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, network)| network.rssi)
                        && network.rssi > networks[weakest].rssi
                    {
                        networks[weakest] = network;
                    }
                }
            }
        }
        if attempt < 4 {
            Timer::after(Duration::from_secs(1)).await;
        }
    }
    portal.set_networks(networks);
}

async fn connect_station(
    controller: &mut WifiController<'_>,
    station: Stack<'static>,
    credentials: &Credentials,
) -> Result<(), heapless::String<64>> {
    let auth_method = match credentials.authentication() {
        Authentication::Open => AuthenticationMethodConfig::Open,
        Authentication::Personal => AuthenticationMethodConfig::Wpa2Personal(
            Password::try_from(credentials.password()).expect("valid Wi-Fi password"),
        ),
    };
    let station_config = StationConfig::default()
        .with_ssid(Ssid::try_from(credentials.ssid()).expect("valid Wi-Fi SSID"))
        .with_authentication(auth_method);
    let mut associated = false;
    for attempt in 0..5 {
        if controller
            .set_config(&WifiConfig::Station(station_config.clone()))
            .is_err()
        {
            return Err(heapless::String::try_from("association-failed:configuration").unwrap());
        }
        let failure = match select(
            controller.connect_async(),
            Timer::after(Duration::from_secs(15)),
        )
        .await
        {
            Either::First(Ok(_)) => {
                associated = true;
                break;
            }
            Either::First(Err(error)) => {
                let mut failure = heapless::String::new();
                write!(failure, "association-failed:{error}").unwrap();
                failure
            }
            Either::Second(_) => heapless::String::try_from("association-failed:timeout").unwrap(),
        };
        if attempt == 4 {
            return Err(failure);
        }
        let _ = controller.disconnect_async().await;
        Timer::after(Duration::from_secs(2 * (attempt + 1) as u64)).await;
    }
    if !associated {
        return Err(heapless::String::try_from("association-failed:unknown").unwrap());
    }
    if matches!(
        select(
            station.wait_config_up(),
            Timer::after(Duration::from_secs(15))
        )
        .await,
        Either::First(_)
    ) {
        Ok(())
    } else {
        Err(heapless::String::try_from("association-failed:dhcp-timeout").unwrap())
    }
}

async fn send_network_events(
    serial: &mut UsbSerialJtag<'_, esp_hal::Async>,
    station: Stack<'static>,
    final_event: &[u8],
) {
    send_event(serial, b"associated").await;
    if let Some(config) = station.config_v4() {
        let mut event = heapless::String::<48>::new();
        write!(event, "dhcp:{}", config.address.address()).unwrap();
        send_event(serial, event.as_bytes()).await;
    }
    send_event(serial, final_event).await;
}

#[embassy_executor::task]
async fn network_runner(mut runner: Runner<'static, Interface>) {
    runner.run().await;
}

#[embassy_executor::task]
async fn health_server(stack: Stack<'static>, token: &'static str) {
    let mut rx_buffer = [0_u8; 1024];
    let mut tx_buffer = [0_u8; 1024];
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        if socket
            .accept(IpListenEndpoint {
                addr: None,
                port: 80,
            })
            .await
            .is_err()
        {
            continue;
        }
        let mut request = [0_u8; 512];
        let count = socket.read(&mut request).await.unwrap_or(0);
        let expected = format_health_request(token);
        let valid = core::str::from_utf8(&request[..count])
            .ok()
            .and_then(|request| request.lines().next())
            .is_some_and(|line| line == expected.as_str());
        let response: &[u8] = if valid {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
        } else {
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        };
        let _ = socket.write_all(response).await;
        let _ = socket.flush().await;
        socket.close();
        Timer::after(Duration::from_millis(20)).await;
        socket.abort();
    }
}

fn format_health_request(token: &str) -> heapless::String<64> {
    let mut request = heapless::String::new();
    write!(request, "GET /health?token={token} HTTP/1.1").unwrap();
    request
}

async fn send_event(serial: &mut UsbSerialJtag<'_, esp_hal::Async>, event: &[u8]) {
    let mut framed = Vec::<u8, 128>::new();
    if encode(FrameKind::Event, event, &mut framed).is_ok() {
        let _ = serial.write_all(&framed).await;
        let _ = serial.flush().await;
    }
}

fn discard_input<const N: usize>(input: &mut Vec<u8, N>, count: usize) {
    let remaining = input.len() - count;
    input.copy_within(count.., 0);
    input.truncate(remaining);
}
