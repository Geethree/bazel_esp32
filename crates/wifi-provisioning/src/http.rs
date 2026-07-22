use core::{
    cmp::Reverse,
    fmt::{self, Write},
};

use heapless::{String, Vec};
use portal_assets::INDEX_HTML;

use crate::{CredentialError, Credentials, Network};

const MAX_NETWORKS: usize = 16;
const MAX_REQUEST: usize = 1536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalStatus {
    Ready,
    Connecting,
    Failed(String<96>),
    Connected,
}

/// The generated, self-contained captive-portal page.
pub fn index_html() -> &'static [u8] {
    INDEX_HTML.as_bytes()
}

/// Whether a bounded HTTP request asks for the static portal document.
pub fn is_index_request(request: &[u8]) -> bool {
    core::str::from_utf8(request)
        .ok()
        .and_then(|request| request.lines().next())
        .is_some_and(|line| {
            let mut words = line.split_ascii_whitespace();
            matches!((words.next(), words.next()), (Some("GET"), Some("/")))
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalAction {
    None,
    Connect(Credentials),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpError {
    RequestTooLarge,
    MalformedRequest,
    ResponseTooLarge,
}

pub struct PortalService {
    networks: Vec<Network, MAX_NETWORKS>,
    status: PortalStatus,
}

impl Default for PortalService {
    fn default() -> Self {
        Self::new()
    }
}

impl PortalService {
    pub const fn new() -> Self {
        Self {
            networks: Vec::new(),
            status: PortalStatus::Ready,
        }
    }

    pub fn set_networks(&mut self, networks: impl IntoIterator<Item = Network>) {
        self.networks.clear();
        for network in networks {
            if network.ssid.is_empty() {
                continue;
            }
            if let Some(existing) = self
                .networks
                .iter_mut()
                .find(|item| item.ssid == network.ssid)
            {
                if network.rssi > existing.rssi {
                    *existing = network;
                }
            } else {
                let _ = self.networks.push(network);
            }
        }
        self.networks
            .sort_unstable_by_key(|network| Reverse(network.rssi));
    }

    pub fn set_status(&mut self, status: PortalStatus) {
        self.status = status;
    }

    /// Password-free snapshot used by local diagnostics transports.
    pub fn networks(&self) -> &[Network] {
        &self.networks
    }

    /// Current provisioning state used by local diagnostics transports.
    pub fn status(&self) -> &PortalStatus {
        &self.status
    }

    pub fn handle<const N: usize>(
        &mut self,
        request: &[u8],
        response: &mut Vec<u8, N>,
    ) -> Result<PortalAction, HttpError> {
        response.clear();
        if request.len() > MAX_REQUEST {
            return Err(HttpError::RequestTooLarge);
        }
        let request = core::str::from_utf8(request).map_err(|_| HttpError::MalformedRequest)?;
        let header_end = request
            .find("\r\n\r\n")
            .ok_or(HttpError::MalformedRequest)?;
        let (head, body_with_separator) = request.split_at(header_end);
        let body = &body_with_separator[4..];
        let request_line = head.lines().next().ok_or(HttpError::MalformedRequest)?;
        let mut words = request_line.split_ascii_whitespace();
        let method = words.next().ok_or(HttpError::MalformedRequest)?;
        let path = words.next().ok_or(HttpError::MalformedRequest)?;

        match (method, path) {
            ("GET", "/") => {
                write_response(response, 200, "text/html; charset=utf-8", index_html())?;
                Ok(PortalAction::None)
            }
            ("GET", "/networks") => {
                let mut json = String::<1024>::new();
                json.push('[').map_err(|_| HttpError::ResponseTooLarge)?;
                for (index, network) in self.networks.iter().enumerate() {
                    if index != 0 {
                        json.push(',').map_err(|_| HttpError::ResponseTooLarge)?;
                    }
                    write!(
                        json,
                        "{{\"ssid\":\"{}\",\"rssi\":{},\"secure\":{}}}",
                        Escaped(&network.ssid),
                        network.rssi,
                        network.secure
                    )
                    .map_err(|_| HttpError::ResponseTooLarge)?;
                }
                json.push(']').map_err(|_| HttpError::ResponseTooLarge)?;
                write_response(response, 200, "application/json", json.as_bytes())?;
                Ok(PortalAction::None)
            }
            ("GET", "/status") => {
                let mut json = String::<160>::new();
                match &self.status {
                    PortalStatus::Ready => json
                        .push_str("{\"state\":\"ready\"}")
                        .map_err(|_| HttpError::ResponseTooLarge)?,
                    PortalStatus::Connecting => json
                        .push_str("{\"state\":\"connecting\"}")
                        .map_err(|_| HttpError::ResponseTooLarge)?,
                    PortalStatus::Connected => json
                        .push_str("{\"state\":\"connected\"}")
                        .map_err(|_| HttpError::ResponseTooLarge)?,
                    PortalStatus::Failed(message) => write!(
                        json,
                        "{{\"state\":\"failed\",\"message\":\"{}\"}}",
                        Escaped(message)
                    )
                    .map_err(|_| HttpError::ResponseTooLarge)?,
                }
                write_response(response, 200, "application/json", json.as_bytes())?;
                Ok(PortalAction::None)
            }
            ("POST", "/connect") => match parse_form(body) {
                Ok(credentials) => {
                    self.status = PortalStatus::Connecting;
                    write_response(
                        response,
                        202,
                        "application/json",
                        br#"{"message":"Credentials accepted; connecting."}"#,
                    )?;
                    Ok(PortalAction::Connect(credentials))
                }
                Err(error) => {
                    let message = credential_error_message(error);
                    let mut json = String::<128>::new();
                    write!(json, "{{\"message\":\"{}\"}}", message)
                        .map_err(|_| HttpError::ResponseTooLarge)?;
                    write_response(response, 400, "application/json", json.as_bytes())?;
                    Ok(PortalAction::None)
                }
            },
            ("GET", path) if is_captive_path(path) => {
                write_redirect(response)?;
                Ok(PortalAction::None)
            }
            _ => {
                write_response(response, 404, "text/plain", b"Not found")?;
                Ok(PortalAction::None)
            }
        }
    }
}

fn parse_form(body: &str) -> Result<Credentials, CredentialError> {
    let mut ssid = String::<32>::new();
    let mut password = String::<63>::new();
    for field in body.split('&') {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        match key {
            "ssid" => decode_form(value, &mut ssid).map_err(|_| CredentialError::SsidTooLong)?,
            "password" => {
                decode_form(value, &mut password).map_err(|_| CredentialError::PasswordTooLong)?
            }
            _ => {}
        }
    }
    Credentials::new(&ssid, &password)
}

fn decode_form<const N: usize>(source: &str, output: &mut String<N>) -> Result<(), ()> {
    output.clear();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let value = match bytes[index] {
            b'+' => b' ',
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1]).ok_or(())?;
                let low = hex(bytes[index + 2]).ok_or(())?;
                index += 2;
                high << 4 | low
            }
            byte if byte.is_ascii() => byte,
            _ => return Err(()),
        };
        output.push(value as char).map_err(|_| ())?;
        index += 1;
    }
    Ok(())
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn credential_error_message(error: CredentialError) -> &'static str {
    match error {
        CredentialError::EmptySsid => "SSID is required.",
        CredentialError::SsidTooLong => "SSID is too long.",
        CredentialError::PasswordTooShort => "Password must be at least 8 bytes.",
        CredentialError::PasswordTooLong => "Password is too long.",
        CredentialError::UnsupportedAuthentication => "Authentication is unsupported.",
    }
}

fn is_captive_path(path: &str) -> bool {
    matches!(
        path,
        "/generate_204"
            | "/gen_204"
            | "/hotspot-detect.html"
            | "/canonical.html"
            | "/ncsi.txt"
            | "/connecttest.txt"
            | "/redirect"
    )
}

fn write_redirect<const N: usize>(response: &mut Vec<u8, N>) -> Result<(), HttpError> {
    response
        .extend_from_slice(b"HTTP/1.1 302 Found\r\nLocation: http://192.168.4.1/\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .map_err(|_| HttpError::ResponseTooLarge)
}

fn write_response<const N: usize>(
    response: &mut Vec<u8, N>,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), HttpError> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let mut header = String::<160>::new();
    write!(
        header,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .map_err(|_| HttpError::ResponseTooLarge)?;
    response
        .extend_from_slice(header.as_bytes())
        .map_err(|_| HttpError::ResponseTooLarge)?;
    response
        .extend_from_slice(body)
        .map_err(|_| HttpError::ResponseTooLarge)
}

struct Escaped<'a>(&'a str);

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for character in self.0.chars() {
            match character {
                '"' => formatter.write_str("\\\"")?,
                '\\' => formatter.write_str("\\\\")?,
                '\n' => formatter.write_str("\\n")?,
                '\r' => formatter.write_str("\\r")?,
                '\t' => formatter.write_str("\\t")?,
                '<' => formatter.write_str("\\u003c")?,
                '>' => formatter.write_str("\\u003e")?,
                '&' => formatter.write_str("\\u0026")?,
                value if value < ' ' => write!(formatter, "\\u{:04x}", value as u32)?,
                value => formatter.write_char(value)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(service: &mut PortalService, request: &[u8]) -> (PortalAction, Vec<u8, 2048>) {
        let mut response = Vec::new();
        let action = service.handle(request, &mut response).unwrap();
        (action, response)
    }

    #[test]
    fn parses_the_browser_form() {
        let mut service = PortalService::new();
        let body = "ssid=My+WiFi&password=a%26password";
        let mut raw = String::<256>::new();
        write!(
            raw,
            "POST /connect HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        let (action, response) = request(&mut service, raw.as_bytes());
        let PortalAction::Connect(credentials) = action else {
            panic!("expected connect action")
        };
        assert_eq!(credentials.ssid(), "My WiFi");
        assert_eq!(credentials.password(), "a&password");
        assert!(response.starts_with(b"HTTP/1.1 202"));
    }

    #[test]
    fn rejects_short_password() {
        let mut service = PortalService::new();
        let (_, response) = request(
            &mut service,
            b"POST /connect HTTP/1.1\r\n\r\nssid=wifi&password=short",
        );
        assert!(response.starts_with(b"HTTP/1.1 400"));
    }

    #[test]
    fn deduplicates_and_orders_networks() {
        let mut service = PortalService::new();
        service.set_networks([
            Network::new("weak", -80, true).unwrap(),
            Network::new("strong", -20, false).unwrap(),
            Network::new("weak", -40, true).unwrap(),
        ]);
        let (_, response) = request(&mut service, b"GET /networks HTTP/1.1\r\n\r\n");
        let text = core::str::from_utf8(&response).unwrap();
        assert!(text.find("strong").unwrap() < text.find("weak").unwrap());
        assert_eq!(text.matches("weak").count(), 1);
    }

    #[test]
    fn captive_checks_redirect() {
        let mut service = PortalService::new();
        for path in ["/generate_204", "/hotspot-detect.html"] {
            let mut raw = std::string::String::from("GET ");
            raw.push_str(path);
            raw.push_str(" HTTP/1.1\r\n\r\n");
            let (_, response) = request(&mut service, raw.as_bytes());
            assert!(response.starts_with(b"HTTP/1.1 302"));
            assert!(
                response
                    .windows(b"Location: http://192.168.4.1/".len())
                    .any(|window| window == b"Location: http://192.168.4.1/")
            );
            assert!(
                response
                    .windows(b"Cache-Control: no-store".len())
                    .any(|window| window == b"Cache-Control: no-store")
            );
        }
    }

    #[test]
    fn generated_portal_is_self_contained() {
        assert!(is_index_request(b"GET / HTTP/1.1\r\nHost: device\r\n\r\n"));
        assert!(!is_index_request(
            b"GET /networks HTTP/1.1\r\nHost: device\r\n\r\n"
        ));
        assert!(index_html().starts_with(b"<!doctype html>"));
        assert!(index_html().windows(7).any(|window| window == b"<style>"));
        assert!(index_html().windows(8).any(|window| window == b"<script>"));
        assert!(!index_html().windows(6).any(|window| window == b"PORTAL"));
        assert!(index_html().len() < 24 * 1024);
    }
}
