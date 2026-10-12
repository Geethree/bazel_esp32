use core::net::Ipv4Addr;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use wifi_provisioning::{Credentials, PortalService};

pub const GATEWAY: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);
pub type PortalLock = Mutex<CriticalSectionRawMutex, PortalService>;
pub static SUBMISSIONS: Channel<CriticalSectionRawMutex, Credentials, 1> = Channel::new();
pub static SCAN_REQUESTS: Channel<CriticalSectionRawMutex, (), 1> = Channel::new();
