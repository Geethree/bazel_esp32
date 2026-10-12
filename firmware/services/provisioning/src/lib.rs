#![no_std]

//! Transport-independent pieces of ESP32 Wi-Fi provisioning.
//!
//! The production portal and optional HIL adapter both feed raw HTTP requests
//! through [`PortalService`]. The `hil` feature is off by default and adds only
//! provisioning-specific test controls; generic framing lives in
//! `hil-testing`.

#[cfg(test)]
extern crate std;

pub mod credentials;
pub mod dns;
pub mod http;
pub mod runtime;
pub mod scan_cache;
pub mod storage;

#[cfg(feature = "hil")]
pub mod hil;

pub use credentials::{Authentication, CredentialError, Credentials, Network};
pub use http::{
    HttpError, PortalAction, PortalService, PortalStatus, index_html, is_index_request,
};
pub use runtime::{ProvisionError, ProvisioningBackend, provision};
pub use scan_cache::ScanCache;
pub use storage::{CredentialStore, FlashJournal, StoreError};

/// Allows production binaries to reject accidental HIL-enabled linkage.
pub const HIL_ENABLED: bool = cfg!(feature = "hil");

/// Runtime policy for the provisioning lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvisioningConfig {
    pub gateway: [u8; 4],
    pub channel: u8,
    pub saved_attempts: u8,
    pub connect_timeout_seconds: u16,
    pub scan_interval_seconds: u16,
    pub reset_window_seconds: u8,
    pub reset_hold_seconds: u8,
}

impl Default for ProvisioningConfig {
    fn default() -> Self {
        Self {
            gateway: [192, 168, 4, 1],
            channel: 6,
            saved_attempts: 3,
            connect_timeout_seconds: 15,
            scan_interval_seconds: 15,
            reset_window_seconds: 10,
            reset_hold_seconds: 3,
        }
    }
}
