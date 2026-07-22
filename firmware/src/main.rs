#![no_main]
#![no_std]
#![deny(clippy::mem_forget)]

use core::{fmt::Write as _, net::Ipv4Addr, str::FromStr};

use edge_nal::UdpBind;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::{
    IpListenEndpoint, Ipv4Cidr, Runner, Stack, StackResources, StaticConfigV4,
    tcp::TcpSocket,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read as _, Write as _};
use esp_hal::{
    Async,
    clock::CpuClock,
    efuse::{self, InterfaceMacAddress},
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    interrupt::software::SoftwareInterruptControl,
    rng::Rng,
    timer::timg::TimerGroup,
    usb_serial_jtag::UsbSerialJtag,
};
use esp_radio::wifi::{
    AuthenticationMethod, Config as WifiConfig, ControllerConfig, Interface, WifiController,
    ap::AccessPointConfig, scan::ScanConfig, sta::StationConfig,
};
use heapless::{String, Vec};
use static_cell::StaticCell;
use wifi_provisioning::{
    Authentication, CredentialStore, Credentials, FlashJournal, Network, PortalAction,
    PortalService, PortalStatus, ProvisioningBackend, ProvisioningConfig, index_html,
    is_index_request, provision,
};

const WIFI_CFG_OFFSET: u32 = 0x3f0000;
const GATEWAY: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);
const _: () = assert!(!wifi_provisioning::HIL_ENABLED);

type PortalLock = Mutex<CriticalSectionRawMutex, PortalService>;

static SUBMISSIONS: Channel<CriticalSectionRawMutex, Credentials, 1> = Channel::new();
static SCAN_REQUESTS: Channel<CriticalSectionRawMutex, (), 1> = Channel::new();

esp_bootloader_esp_idf::esp_app_desc!();

macro_rules! mk_static {
    ($type:ty, $value:expr) => {{
        static CELL: StaticCell<$type> = StaticCell::new();
        CELL.uninit().write($value)
    }};
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendError {
    Wifi,
    Timeout,
}

struct EspBackend<'d> {
    controller: WifiController<'d>,
    status_led: StatusLed<'d>,
    station: Stack<'static>,
    portal: &'static PortalLock,
    ap_ssid: String<32>,
    config: ProvisioningConfig,
    portal_active: bool,
}

struct StatusLed<'d> {
    pin: Output<'d>,
    on: bool,
}

impl<'d> StatusLed<'d> {
    fn new(pin: impl esp_hal::gpio::OutputPin + 'd) -> Self {
        Self {
            // The XIAO ESP32-C6 user LED is wired active-low on GPIO15.
            pin: Output::new(pin, Level::High, OutputConfig::default()),
            on: false,
        }
    }

    fn set(&mut self, on: bool) {
        self.on = on;
        self.pin
            .set_level(if on { Level::Low } else { Level::High });
    }

    fn toggle(&mut self) {
        self.set(!self.on);
    }
}

impl EspBackend<'_> {
    fn access_point_config(&self) -> AccessPointConfig {
        AccessPointConfig::default()
            .with_ssid(self.ap_ssid.as_str())
            .with_channel(self.config.channel)
    }

    fn station_config(credentials: &Credentials) -> StationConfig {
        let auth_method = match credentials.authentication() {
            Authentication::Open => AuthenticationMethod::None,
            // esp-radio applies this as a minimum threshold. WPA3 networks
            // remain eligible while WPA2-only IoT networks are not rejected.
            Authentication::Personal => AuthenticationMethod::Wpa2Personal,
        };
        StationConfig::default()
            .with_ssid(credentials.ssid())
            .with_auth_method(auth_method)
            .with_password(credentials.password().into())
    }

    async fn set_portal_status(&self, status: PortalStatus) {
        self.portal.lock().await.set_status(status);
    }

    async fn refresh_networks(&mut self) -> Result<(), BackendError> {
        let scan = self
            .controller
            .scan_async(&ScanConfig::default().with_max(16))
            .await
            .map_err(|_| BackendError::Wifi)?;
        let networks = scan.into_iter().filter_map(|network| {
            Network::new(
                network.ssid.as_str(),
                network.signal_strength,
                !matches!(network.auth_method, None | Some(AuthenticationMethod::None)),
            )
            .ok()
        });
        self.portal.lock().await.set_networks(networks);
        Ok(())
    }

    async fn finish(&mut self, credentials: &Credentials) -> Result<(), BackendError> {
        if !self.portal_active {
            return Ok(());
        }
        self.portal_active = false;
        self.controller
            .set_config(&WifiConfig::Station(Self::station_config(credentials)))
            .map_err(|_| BackendError::Wifi)?;
        let result = connect_with_timeout(
            &mut self.controller,
            self.station,
            self.config.connect_timeout_seconds,
        )
        .await;
        self.status_led.set(result.is_err());
        result
    }
}

impl ProvisioningBackend for EspBackend<'_> {
    type Error = BackendError;

    async fn connect(
        &mut self,
        credentials: &Credentials,
        timeout_seconds: u16,
    ) -> Result<(), Self::Error> {
        if self.portal_active {
            self.set_portal_status(PortalStatus::Connecting).await;
        }
        let station = Self::station_config(credentials);
        let wifi = if self.portal_active {
            WifiConfig::AccessPointStation(station, self.access_point_config())
        } else {
            WifiConfig::Station(station)
        };
        self.controller
            .set_config(&wifi)
            .map_err(|_| BackendError::Wifi)?;

        let result =
            connect_with_timeout(&mut self.controller, self.station, timeout_seconds).await;
        if self.portal_active {
            let status = if result.is_ok() {
                PortalStatus::Connected
            } else {
                PortalStatus::Failed(String::try_from("Unable to join that network.").unwrap())
            };
            self.set_portal_status(status).await;
        }
        result
    }

    async fn delay_seconds(&mut self, seconds: u16) {
        Timer::after(Duration::from_secs(seconds as u64)).await;
    }

    async fn wait_for_credentials(&mut self) -> Result<Credentials, Self::Error> {
        self.status_led.set(true);
        self.controller
            .set_config(&WifiConfig::Station(StationConfig::default()))
            .map_err(|_| BackendError::Wifi)?;
        self.refresh_networks().await?;
        self.set_portal_status(PortalStatus::Ready).await;

        // Keep setup in AP-only mode until credentials arrive, matching the
        // upstream ESP radio DHCP example. Add station mode only while trying
        // the submitted network.
        self.controller
            .set_config(&WifiConfig::AccessPoint(self.access_point_config()))
            .map_err(|_| BackendError::Wifi)?;
        self.portal_active = true;
        loop {
            match select(
                SUBMISSIONS.receive(),
                select(
                    SCAN_REQUESTS.receive(),
                    Timer::after(Duration::from_secs(u64::from(
                        self.config.scan_interval_seconds.max(1),
                    ))),
                ),
            )
            .await
            {
                Either::First(credentials) => return Ok(credentials),
                Either::Second(_) => {
                    if let Either::Second(credentials) =
                        select(self.refresh_networks(), SUBMISSIONS.receive()).await
                    {
                        return Ok(credentials);
                    }
                }
            }
        }
    }
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
    esp_println::logger::init_logger_from_env();
    let hal_config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(hal_config);
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let boot = Input::new(
        peripherals.GPIO9,
        InputConfig::default().with_pull(Pull::Up),
    );
    let mut status_led = StatusLed::new(peripherals.GPIO15);
    let mut store = FlashJournal::new(
        esp_storage::FlashStorage::new(peripherals.FLASH),
        WIFI_CFG_OFFSET,
    );
    let has_saved_credentials = matches!(store.load(), Ok(Some(_)));
    let policy = ProvisioningConfig::default();
    let reset = reset_requested(&boot, &mut status_led, policy).await;
    let force_portal = has_saved_credentials && reset;
    status_led.set(false);

    let initial_wifi = WifiConfig::Station(StationConfig::default());
    let (controller, interfaces) = esp_radio::wifi::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(initial_wifi),
    )
    .unwrap();

    let ap_network = embassy_net::Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(GATEWAY, 24),
        gateway: Some(GATEWAY),
        dns_servers: Default::default(),
    });
    let station_network = embassy_net::Config::dhcpv4(Default::default());
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let (ap_stack, ap_runner) = embassy_net::new(
        interfaces.access_point,
        ap_network,
        mk_static!(StackResources<6>, StackResources::new()),
        seed,
    );
    let (station_stack, station_runner) = embassy_net::new(
        interfaces.station,
        station_network,
        mk_static!(StackResources<4>, StackResources::new()),
        seed ^ 0x5a5a_a5a5,
    );
    let portal = mk_static!(PortalLock, PortalLock::new(PortalService::new()));
    let serial = UsbSerialJtag::new(peripherals.USB_DEVICE).into_async();

    spawner.spawn(network_runner(ap_runner).unwrap());
    spawner.spawn(network_runner(station_runner).unwrap());
    spawner.spawn(dhcp_server(ap_stack).unwrap());
    spawner.spawn(dns_server(ap_stack).unwrap());
    spawner.spawn(http_server(ap_stack, portal).unwrap());
    spawner.spawn(serial_console(serial, portal).unwrap());

    let mac = efuse::interface_mac_address(InterfaceMacAddress::Station);
    let mut ap_ssid = String::<32>::new();
    write!(
        ap_ssid,
        "XIAO-C6-Setup-{:02X}{:02X}",
        mac.as_bytes()[4],
        mac.as_bytes()[5]
    )
    .unwrap();
    let mut backend = EspBackend {
        controller,
        status_led,
        station: station_stack,
        portal,
        ap_ssid,
        config: policy,
        portal_active: false,
    };

    match provision(&mut backend, &mut store, policy, force_portal).await {
        Ok(credentials) => {
            if backend.finish(&credentials).await.is_ok()
                && let Some(network) = station_stack.config_v4()
            {
                backend.set_portal_status(PortalStatus::Connected).await;
                esp_println::println!("Wi-Fi ready at {}", network.address.address());
            }
        }
        Err(error) => esp_println::println!("Provisioning failed: {error:?}"),
    }

    loop {
        Timer::after(Duration::from_secs(60)).await;
    }
}

#[embassy_executor::task]
async fn serial_console(mut serial: UsbSerialJtag<'static, Async>, portal: &'static PortalLock) {
    let mut line = String::<48>::new();
    let _ = serial
        .write_all(b"\r\nESP32 serial console ready. Type help.\r\nesp32> ")
        .await;
    loop {
        let mut input = [0_u8; 64];
        let count = match serial.read(&mut input).await {
            Ok(count) => count,
            Err(never) => match never {},
        };
        for &byte in &input[..count] {
            match byte {
                b'\r' => {}
                b'\n' => {
                    handle_console_command(&mut serial, portal, line.as_str()).await;
                    line.clear();
                }
                8 | 127 => {
                    line.pop();
                }
                0x20..=0x7e => match line.push(byte as char) {
                    Ok(()) => {}
                    Err(()) => {
                        line.clear();
                        let _ = serial
                            .write_all(b"\r\nerror: command too long\r\nesp32> ")
                            .await;
                    }
                },
                _ => {}
            }
        }
    }
}

async fn handle_console_command(
    serial: &mut UsbSerialJtag<'_, Async>,
    portal: &'static PortalLock,
    command: &str,
) {
    let mut output = String::<1024>::new();
    match command.trim() {
        "" => {}
        "help" => {
            let _ = output.push_str("help       show commands\r\nstatus     provisioning state\r\nnetworks   cached Wi-Fi scan\r\nrescan     refresh the provisioning scan");
        }
        "status" => {
            let state = portal.lock().await.status().clone();
            let name = match state {
                PortalStatus::Ready => "ready",
                PortalStatus::Connecting => "connecting",
                PortalStatus::Failed(_) => "failed",
                PortalStatus::Connected => "connected",
            };
            let _ = write!(output, "state: {name}");
        }
        "networks" => {
            let portal = portal.lock().await;
            if portal.networks().is_empty() {
                let _ = output.push_str("no cached networks");
            } else {
                for network in portal.networks() {
                    let security = if network.secure { "secure" } else { "open" };
                    let _ = writeln!(
                        output,
                        "{}\t{} dBm\t{}",
                        network.ssid, network.rssi, security
                    );
                }
            }
        }
        "rescan" => {
            let state = portal.lock().await.status().clone();
            if matches!(state, PortalStatus::Ready | PortalStatus::Failed(_)) {
                let message = if SCAN_REQUESTS.try_send(()).is_ok() {
                    "scan requested"
                } else {
                    "scan already requested"
                };
                let _ = output.push_str(message);
            } else {
                let _ = output.push_str("rescan is available only while provisioning is idle");
            }
        }
        _ => {
            let _ = output.push_str("unknown command; type help");
        }
    }
    let _ = output.push_str("\r\nesp32> ");
    let _ = serial.write_all(output.as_bytes()).await;
    let _ = serial.flush().await;
}

async fn connect_with_timeout(
    controller: &mut WifiController<'_>,
    station: Stack<'static>,
    timeout_seconds: u16,
) -> Result<(), BackendError> {
    let timeout = Timer::after(Duration::from_secs(timeout_seconds as u64));
    if !matches!(
        select(controller.connect_async(), timeout).await,
        Either::First(Ok(_))
    ) {
        return Err(BackendError::Timeout);
    }
    let timeout = Timer::after(Duration::from_secs(timeout_seconds as u64));
    if matches!(
        select(station.wait_config_up(), timeout).await,
        Either::First(_)
    ) {
        Ok(())
    } else {
        Err(BackendError::Timeout)
    }
}

async fn reset_requested(
    button: &Input<'_>,
    status_led: &mut StatusLed<'_>,
    config: ProvisioningConfig,
) -> bool {
    const SAMPLE_MS: u64 = 50;
    let samples = u64::from(config.reset_window_seconds) * 1_000 / SAMPLE_MS;
    let hold_samples = u64::from(config.reset_hold_seconds) * 1_000 / SAMPLE_MS;
    let mut armed = button.is_high();
    let mut held = 0_u64;
    for sample in 0..samples {
        Timer::after(Duration::from_millis(SAMPLE_MS)).await;
        if sample % (250 / SAMPLE_MS) == 0 {
            status_led.toggle();
        }
        if !armed {
            armed = button.is_high();
            continue;
        }
        if button.is_low() {
            held += 1;
            if held >= hold_samples {
                return true;
            }
        } else {
            held = 0;
        }
    }
    false
}

#[embassy_executor::task(pool_size = 2)]
async fn network_runner(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await;
}

#[embassy_executor::task]
async fn dhcp_server(stack: Stack<'static>) {
    use edge_dhcp::{
        io::{self, DEFAULT_SERVER_PORT},
        server::{Server, ServerOptions},
    };
    use edge_nal_embassy::{Udp, UdpBuffers};

    let mut packet = [0_u8; 1500];
    let mut gateway = [Ipv4Addr::UNSPECIFIED];
    let dns = [GATEWAY];
    let buffers = UdpBuffers::<3, 1024, 1024, 10>::new();
    let socket = Udp::new(stack, &buffers);
    let mut socket = socket
        .bind(core::net::SocketAddr::V4(core::net::SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            DEFAULT_SERVER_PORT,
        )))
        .await
        .unwrap();
    loop {
        let mut options = ServerOptions::new(GATEWAY, Some(&mut gateway));
        options.dns = &dns;
        options.captive_url = Some("http://192.168.4.1/");
        let _ = io::server::run(
            &mut Server::<_, 64>::new_with_et(GATEWAY),
            &options,
            &mut socket,
            &mut packet,
        )
        .await;
        Timer::after(Duration::from_millis(250)).await;
    }
}

#[embassy_executor::task]
async fn dns_server(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buffer = [0_u8; 1024];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buffer = [0_u8; 1024];
    let mut socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buffer,
        &mut tx_meta,
        &mut tx_buffer,
    );
    socket.bind(53).unwrap();
    let mut request = [0_u8; 512];
    let mut response = [0_u8; 512];
    loop {
        let Ok((length, remote)) = socket.recv_from(&mut request).await else {
            continue;
        };
        if let Ok(length) = wifi_provisioning::dns::captive_response(
            &request[..length],
            &mut response,
            GATEWAY.octets(),
        ) {
            let _ = socket.send_to(&response[..length], remote).await;
        }
    }
}

#[embassy_executor::task]
async fn http_server(stack: Stack<'static>, portal: &'static PortalLock) {
    let mut rx_buffer = [0_u8; 2048];
    let mut tx_buffer = [0_u8; 2048];
    let mut request = Vec::<u8, 1536>::new();
    let mut response = Vec::<u8, 2048>::new();
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));
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
        request.clear();
        loop {
            let mut chunk = [0_u8; 256];
            match socket.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if request.extend_from_slice(&chunk[..count]).is_err() {
                        break;
                    }
                    if request_complete(&request) {
                        break;
                    }
                }
            }
        }
        if is_index_request(&request) {
            let body = index_html();
            let mut header = String::<192>::new();
            let _ = write!(
                header,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(body).await;
            let _ = socket.flush().await;
        } else {
            response.clear();
            let action = portal.lock().await.handle(&request, &mut response);
            if let Ok(action) = action {
                let _ = socket.write_all(&response).await;
                let _ = socket.flush().await;
                if let PortalAction::Connect(credentials) = action {
                    SUBMISSIONS.send(credentials).await;
                }
            }
        }
        socket.close();
        Timer::after(Duration::from_millis(20)).await;
        socket.abort();
    }
}

fn request_complete(request: &[u8]) -> bool {
    let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = core::str::from_utf8(&request[..header_end]).unwrap_or("");
    let content_length = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| usize::from_str(value.trim()).ok())
            .flatten()
    });
    request.len() >= header_end + 4 + content_length.unwrap_or(0)
}
