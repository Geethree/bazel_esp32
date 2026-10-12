use core::fmt;

use heapless::String;

pub const MAX_SSID_LEN: usize = 32;
pub const MAX_PASSWORD_LEN: usize = 63;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Authentication {
    Open = 0,
    Personal = 1,
}

impl TryFrom<u8> for Authentication {
    type Error = CredentialError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Open),
            1 => Ok(Self::Personal),
            _ => Err(CredentialError::UnsupportedAuthentication),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialError {
    EmptySsid,
    SsidTooLong,
    PasswordTooShort,
    PasswordTooLong,
    UnsupportedAuthentication,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    ssid: String<MAX_SSID_LEN>,
    password: String<MAX_PASSWORD_LEN>,
    authentication: Authentication,
}

impl Credentials {
    pub fn new(ssid: &str, password: &str) -> Result<Self, CredentialError> {
        if ssid.is_empty() {
            return Err(CredentialError::EmptySsid);
        }
        if ssid.len() > MAX_SSID_LEN {
            return Err(CredentialError::SsidTooLong);
        }
        if password.len() > MAX_PASSWORD_LEN {
            return Err(CredentialError::PasswordTooLong);
        }
        if !password.is_empty() && password.len() < 8 {
            return Err(CredentialError::PasswordTooShort);
        }

        let authentication = if password.is_empty() {
            Authentication::Open
        } else {
            Authentication::Personal
        };
        let mut ssid_value = String::new();
        ssid_value
            .push_str(ssid)
            .map_err(|_| CredentialError::SsidTooLong)?;
        let mut password_value = String::new();
        password_value
            .push_str(password)
            .map_err(|_| CredentialError::PasswordTooLong)?;

        Ok(Self {
            ssid: ssid_value,
            password: password_value,
            authentication,
        })
    }

    pub fn ssid(&self) -> &str {
        &self.ssid
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    pub fn authentication(&self) -> Authentication {
        self.authentication
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("ssid", &self.ssid)
            .field("password", &"[redacted]")
            .field("authentication", &self.authentication)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    pub ssid: String<MAX_SSID_LEN>,
    pub rssi: i8,
    pub secure: bool,
}

impl Network {
    pub fn new(ssid: &str, rssi: i8, secure: bool) -> Result<Self, CredentialError> {
        let credentials = Credentials::new(ssid, "")?;
        Ok(Self {
            ssid: credentials.ssid,
            rssi,
            secure,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_credentials() {
        assert_eq!(
            Credentials::new("", "").unwrap_err(),
            CredentialError::EmptySsid
        );
        assert_eq!(
            Credentials::new("wifi", "short").unwrap_err(),
            CredentialError::PasswordTooShort
        );
        assert!(Credentials::new("wifi", "password").is_ok());
        assert!(Credentials::new("open", "").is_ok());
    }

    #[test]
    fn debug_redacts_password() {
        let credentials = Credentials::new("wifi", "secret-password").unwrap();
        let rendered = std::format!("{credentials:?}");
        assert!(rendered.contains("[redacted]"));
        assert!(!rendered.contains("secret-password"));
    }
}
