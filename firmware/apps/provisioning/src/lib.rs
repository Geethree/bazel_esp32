#![no_std]

mod console;
mod network;
mod reset;
mod runtime;

use core::fmt::Write as _;

use board::StatusLed;
use board_api::StatusIndicator;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::{Ipv4Cidr, Stack, StackResources, StaticConfigV4};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    clock::CpuClock,
    efuse::{self, InterfaceMacAddress},
    rng::Rng,
    time::Duration as HalDuration,
    timer::timg::TimerGroup,
};
use esp_radio::wifi::{
    AuthenticationMethod, AuthenticationMethodConfig, Config as WifiConfig, ControllerConfig,
    Interface, Password, Ssid, WifiController,
    ap::AccessPointConfig,
    scan::{ScanConfig, ScanTypeConfig},
    sta::StationConfig,
};
use heapless::String;
use runtime::{GATEWAY, PortalLock, SCAN_REQUESTS, SUBMISSIONS};
use static_cell::StaticCell;
use wifi_provisioning::{
    Authentication, CredentialStore, Credentials, FlashJournal, Network, PortalService,
    PortalStatus, ProvisioningBackend, ProvisioningConfig, ScanCache, provision,
};

const _: () = assert!(!wifi_provisioning::HIL_ENABLED);

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
    status_led: StatusLed,
    station: Stack<'static>,
    portal: &'static PortalLock,
    scan_cache: ScanCache<16>,
    ap_ssid: String<32>,
    config: ProvisioningConfig,
    portal_active: bool,
}

impl EspBackend<'_> {
    fn access_point_config(&self) -> AccessPointConfig {
        AccessPointConfig::default()
            .with_ssid(Ssid::try_from(self.ap_ssid.as_str()).expect("valid AP SSID"))
            .with_channel(self.config.channel)
    }

    fn station_config(credentials: &Credentials) -> StationConfig {
        let auth_method = match credentials.authentication() {
            Authentication::Open => AuthenticationMethodConfig::Open,
            Authentication::Personal => AuthenticationMethodConfig::Wpa2Personal(
                Password::try_from(credentials.password()).expect("valid Wi-Fi password"),
            ),
        };
        StationConfig::default()
            .with_ssid(Ssid::try_from(credentials.ssid()).expect("valid Wi-Fi SSID"))
            .with_authentication(auth_method)
    }

    async fn set_portal_status(&self, status: PortalStatus) {
        self.portal.lock().await.set_status(status);
    }

    async fn refresh_networks(
        &mut self,
        scan_config: &ScanConfig,
    ) -> Result<(usize, usize), BackendError> {
        let scan = match self.controller.scan_async(scan_config).await {
            Ok(scan) => scan,
            Err(_) => {
                self.scan_cache
                    .observe(Instant::now().as_millis(), core::iter::empty());
                self.portal
                    .lock()
                    .await
                    .set_networks(self.scan_cache.networks());
                return Err(BackendError::Wifi);
            }
        };
        let raw_count = scan.len();
        let networks = scan.into_iter().filter_map(|network| {
            Network::new(
                network.ssid.as_str(),
                network.signal_strength,
                !matches!(network.auth_method, None | Some(AuthenticationMethod::None)),
            )
            .ok()
        });
        self.scan_cache
            .observe(Instant::now().as_millis(), networks);
        let mut portal = self.portal.lock().await;
        portal.set_networks(self.scan_cache.networks());
        Ok((raw_count, portal.networks().len()))
    }

    async fn refresh_networks_in_station_mode(&mut self) -> Result<(usize, usize), BackendError> {
        let station_ready = self
            .controller
            .set_config(&WifiConfig::Station(StationConfig::default()))
            .is_ok();
        let mut observations = 0;
        let mut successful = false;
        if station_ready {
            Timer::after(Duration::from_millis(200)).await;
            // The default 10–20 ms dwell can miss weak APs. Manual scans may
            // interrupt the setup AP, so give each channel more time here.
            let scan_config =
                ScanConfig::default()
                    .with_max(16)
                    .with_scan_type(ScanTypeConfig::Active {
                        min: HalDuration::from_millis(50),
                        max: HalDuration::from_millis(120),
                    });
            for attempt in 0..3 {
                if let Either::First(Ok((raw, _))) = select(
                    self.refresh_networks(&scan_config),
                    Timer::after(Duration::from_secs(3)),
                )
                .await
                {
                    observations += raw;
                    successful = true;
                }
                if attempt < 2 {
                    Timer::after(Duration::from_millis(250)).await;
                }
            }
        }

        // Restore the setup AP even if the scan failed.
        self.controller
            .set_config(&WifiConfig::AccessPointStation(
                StationConfig::default(),
                self.access_point_config(),
            ))
            .map_err(|_| BackendError::Wifi)?;
        if !successful {
            return Err(BackendError::Wifi);
        }
        let listed = self.portal.lock().await.networks().len();
        Ok((observations, listed))
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
        let _ = self
            .refresh_networks(&ScanConfig::default().with_max(16))
            .await?;
        self.set_portal_status(PortalStatus::Ready).await;

        // Keep the station interface available for periodic and manual scans.
        // esp-radio does not support scanning in access-point-only mode.
        self.controller
            .set_config(&WifiConfig::AccessPointStation(
                StationConfig::default(),
                self.access_point_config(),
            ))
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
                Either::Second(trigger) => {
                    let manual = matches!(trigger, Either::First(()));
                    let scan = async {
                        if manual {
                            self.refresh_networks_in_station_mode().await
                        } else {
                            self.refresh_networks(&ScanConfig::default().with_max(16))
                                .await
                        }
                    };
                    match select(scan, SUBMISSIONS.receive()).await {
                        Either::First(Ok((raw, listed))) if manual => {
                            esp_println::println!(
                                "Wi-Fi scan complete: {raw} AP observations, {listed} listed"
                            );
                        }
                        Either::First(Err(error)) if manual => {
                            esp_println::println!("Wi-Fi scan failed: {error:?}");
                        }
                        Either::Second(credentials) => return Ok(credentials),
                        _ => {}
                    }
                }
            }
        }
    }
}

pub async fn run(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    esp_println::println!("Provisioning on {}", board::NAME);
    let hal_config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(hal_config);
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    let boot = board::take_button!(peripherals);
    let mut status_led = board::take_led!(peripherals);
    let mut store = FlashJournal::new(
        esp_storage::FlashStorage::new(peripherals.FLASH),
        board::WIFI_CFG_OFFSET,
    );
    let has_saved_credentials = matches!(store.load(), Ok(Some(_)));
    let policy = ProvisioningConfig::default();
    let reset = reset::reset_requested(
        &boot,
        &mut status_led,
        policy.reset_window_seconds,
        policy.reset_hold_seconds,
    )
    .await;
    let force_portal = has_saved_credentials && reset;
    status_led.set(false);

    let initial_wifi = WifiConfig::Station(StationConfig::default());
    let controller = WifiController::new(
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
        Interface::access_point(),
        ap_network,
        mk_static!(StackResources<6>, StackResources::new()),
        seed,
    );
    let (station_stack, station_runner) = embassy_net::new(
        Interface::station(),
        station_network,
        mk_static!(StackResources<4>, StackResources::new()),
        seed ^ 0x5a5a_a5a5,
    );
    let portal = mk_static!(PortalLock, PortalLock::new(PortalService::new()));
    let serial = board::take_console!(peripherals);

    spawner.spawn(network::network_runner(ap_runner).unwrap());
    spawner.spawn(network::network_runner(station_runner).unwrap());
    spawner.spawn(network::dhcp_server(ap_stack).unwrap());
    spawner.spawn(network::dns_server(ap_stack).unwrap());
    spawner.spawn(network::http_server(ap_stack, portal).unwrap());
    spawner.spawn(console::serial_console(serial, portal).unwrap());

    let mac = efuse::interface_mac_address(InterfaceMacAddress::Station);
    let mut ap_ssid = String::<32>::new();
    write!(
        ap_ssid,
        "{}-{:02X}{:02X}",
        board::AP_PREFIX,
        mac.as_bytes()[4],
        mac.as_bytes()[5]
    )
    .unwrap();
    let mut backend = EspBackend {
        controller,
        status_led,
        station: station_stack,
        portal,
        scan_cache: ScanCache::new(120_000),
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
