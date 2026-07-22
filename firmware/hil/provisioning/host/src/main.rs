use std::{
    collections::BTreeSet,
    env,
    error::Error,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use hil_testing::{
    FrameKind,
    host::{OwnedFrame as HilFrame, Session},
};
use serial_transport::Options as SerialOptions;
use wifi_provisioning::hil::Control;

// Spectrum's 2.4 GHz device network isolates Wi-Fi clients. Enable this on a
// HIL network that permits the host to connect directly to the board.
const RUN_LAN_HEALTH_CHECKS: bool = false;
const WIFI_SSID_ENV: &str = "HIL_WIFI_SSID";
const WIFI_PASSWORD_ENV: &str = "HIL_WIFI_PASSWORD";

struct State {
    events: BTreeSet<String>,
    health_token: Option<String>,
    address: Option<IpAddr>,
}

enum OwnedFrame {
    Response(Vec<u8>),
    Event(String),
    Other,
}

struct Hil {
    session: Session,
}

impl Hil {
    fn open(path: &str) -> Result<Self, Box<dyn Error>> {
        let options = SerialOptions {
            port: Some(path.to_owned()),
            ..SerialOptions::default()
        };
        Ok(Self {
            session: Session::open(options)?,
        })
    }

    fn reopen(&mut self, timeout: Duration) -> Result<(), Box<dyn Error>> {
        self.session.reopen(timeout)?;
        Ok(())
    }

    fn send_http(&mut self, request: &[u8]) -> Result<(), Box<dyn Error>> {
        self.session.send(FrameKind::Request, request)?;
        Ok(())
    }

    fn send_control(&mut self, control: Control) -> Result<(), Box<dyn Error>> {
        self.session.send(FrameKind::Control, &control.encode())?;
        Ok(())
    }

    fn next(&mut self, timeout: Duration) -> Result<OwnedFrame, Box<dyn Error>> {
        Ok(match self.session.next(timeout)? {
            HilFrame {
                kind: FrameKind::Response,
                payload,
            } => OwnedFrame::Response(payload),
            HilFrame {
                kind: FrameKind::Event,
                payload,
            } => OwnedFrame::Event(String::from_utf8(payload)?),
            _ => OwnedFrame::Other,
        })
    }
}

struct Options {
    espflash: PathBuf,
    firmware: PathBuf,
    partition_table: PathBuf,
    chip: String,
    wifi_offset: String,
    wifi_size: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse()?;
    let (ssid, password) = wifi_credentials_from_env()?;
    let port = discover_serial_port()?;
    prepare_device(&options, &port)?;
    thread::sleep(Duration::from_secs(1));
    let mut hil = Hil::open(&port)?;
    println!("HIL: serial transport open");
    let mut state = State {
        events: BTreeSet::new(),
        health_token: None,
        address: None,
    };

    assert_status(
        &mut hil,
        &mut state,
        b"GET /status HTTP/1.1\r\nHost: device\r\n\r\n",
        200,
    )?;
    println!("HIL: shared status route passed");
    wait_until(&mut hil, &mut state, Duration::from_secs(15), |state| {
        state.events.contains("ready") && state.health_token.is_some()
    })?;
    let networks_response = assert_status(
        &mut hil,
        &mut state,
        b"GET /networks HTTP/1.1\r\nHost: device\r\n\r\n",
        200,
    )?;
    println!("HIL: shared network-list route passed");
    if let Some(body) = response_body(&networks_response) {
        println!("HIL: aggregated device scan: {body}");
    }
    if let Some(rssi) = network_rssi(&networks_response, &ssid) {
        println!("HIL: configured SSID {ssid:?} observed at {rssi} dBm");
    } else {
        println!("HIL: configured SSID {ssid:?} was not observed; trying hidden-SSID entry");
    }
    assert_status(
        &mut hil,
        &mut state,
        b"POST /connect HTTP/1.1\r\nContent-Length: 24\r\n\r\nssid=test&password=bad",
        400,
    )?;
    println!("HIL: malformed form rejected");

    let body = format!(
        "ssid={}&password={}",
        form_encode(&ssid),
        form_encode(&password)
    );
    let request = format!(
        "POST /connect HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    assert_status(&mut hil, &mut state, request.as_bytes(), 202)?;
    wait_until(&mut hil, &mut state, Duration::from_secs(90), |state| {
        state.events.contains("associated")
            && state.events.contains("credentials-saved")
            && state.address.is_some()
    })?;
    println!("HIL: associated, received DHCP, and persisted credentials");
    if RUN_LAN_HEALTH_CHECKS {
        check_health(&state)?;
        println!("HIL: first tokenized LAN health check passed");
    } else {
        println!("HIL: LAN health check skipped (network-dependent)");
    }

    hil.send_control(Control::Reset)?;
    wait_until(&mut hil, &mut state, Duration::from_secs(10), |state| {
        state.events.contains("resetting")
    })
    .map_err(|error| format!("clean reset handshake failed: {error}"))?;
    println!("HIL: station disconnected and reset requested");
    thread::sleep(Duration::from_millis(500));
    hil.reopen(Duration::from_secs(15))?;
    println!("HIL: serial transport reopened after reset");
    state = State {
        events: BTreeSet::new(),
        health_token: None,
        address: None,
    };
    wait_until(&mut hil, &mut state, Duration::from_secs(60), |state| {
        state.events.contains("ready") && state.health_token.is_some()
    })
    .map_err(|error| format!("post-reset boot handshake failed: {error}"))?;
    println!("HIL: firmware ready after reset");
    wait_until(&mut hil, &mut state, Duration::from_secs(90), |state| {
        state.events.contains("stored-reconnected") && state.address.is_some()
    })
    .map_err(|error| format!("stored-credential reconnection failed: {error}"))?;
    println!("HIL: stored credentials reconnected after reset");
    assert_status(
        &mut hil,
        &mut state,
        b"GET /status HTTP/1.1\r\nHost: device\r\n\r\n",
        200,
    )
    .map_err(|error| format!("post-reset status request failed: {error}"))?;
    if RUN_LAN_HEALTH_CHECKS {
        check_health(&state)?;
        println!("HIL: post-reset tokenized LAN health check passed");
    } else {
        println!("HIL: post-reset LAN health check skipped (network-dependent)");
    }

    hil.send_control(Control::ClearStorage)?;
    wait_until(&mut hil, &mut state, Duration::from_secs(45), |state| {
        state.events.contains("storage-cleared")
    })
    .map_err(|error| format!("credential clearing failed: {error}"))?;
    let response = request_response(
        &mut hil,
        &mut state,
        b"GET /status HTTP/1.1\r\nHost: device\r\n\r\n",
    )?;
    if !response
        .windows(b"\"state\":\"ready\"".len())
        .any(|window| window == b"\"state\":\"ready\"")
    {
        return Err("setup mode did not return after clearing storage".into());
    }
    println!("provisioning HIL passed");
    Ok(())
}

impl Options {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let mut arguments = std::env::args().skip(1);
        let mut value = |name: &str| -> Result<String, Box<dyn Error>> {
            let flag = arguments.next().ok_or("missing HIL rule argument")?;
            if flag != name {
                return Err(format!("expected {name}").into());
            }
            arguments
                .next()
                .ok_or_else(|| format!("missing value for {name}").into())
        };
        Ok(Self {
            espflash: value("--espflash")?.into(),
            firmware: value("--firmware")?.into(),
            partition_table: value("--partition-table")?.into(),
            chip: value("--chip")?,
            wifi_offset: value("--wifi-offset")?,
            wifi_size: value("--wifi-size")?,
        })
    }
}

fn discover_serial_port() -> Result<String, Box<dyn Error>> {
    Ok(serial_transport::discover(&SerialOptions::default())?)
}

fn wifi_credentials_from_env() -> Result<(String, String), Box<dyn Error>> {
    let ssid = env::var(WIFI_SSID_ENV).map_err(|_| format!("missing {WIFI_SSID_ENV}"))?;
    let password =
        env::var(WIFI_PASSWORD_ENV).map_err(|_| format!("missing {WIFI_PASSWORD_ENV}"))?;
    wifi_provisioning::Credentials::new(&ssid, &password)
        .map_err(|_| "invalid HIL Wi-Fi credentials")?;
    Ok((ssid, password))
}

fn escaped_ssid_marker(ssid: &str) -> String {
    let mut escaped = String::from("\"ssid\":\"");
    for character in ssid.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            value if value < ' ' => {
                use std::fmt::Write as _;
                write!(escaped, "\\u{:04x}", value as u32).unwrap();
            }
            value => escaped.push(value),
        }
    }
    escaped.push('"');
    escaped
}

fn network_rssi(response: &[u8], ssid: &str) -> Option<i16> {
    let marker = escaped_ssid_marker(ssid);
    let offset = response
        .windows(marker.len())
        .position(|window| window == marker.as_bytes())?;
    let remainder = core::str::from_utf8(&response[offset + marker.len()..]).ok()?;
    let value = remainder.strip_prefix(",\"rssi\":")?;
    let end = value.find(',')?;
    value[..end].parse().ok()
}

fn response_body(response: &[u8]) -> Option<&str> {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .and_then(|offset| core::str::from_utf8(&response[offset + 4..]).ok())
}

fn prepare_device(options: &Options, port: &str) -> Result<(), Box<dyn Error>> {
    let erased = Command::new(&options.espflash)
        .args([
            "erase-region",
            "--chip",
            options.chip.as_str(),
            "--port",
            port,
            "--non-interactive",
            "--skip-update-check",
            options.wifi_offset.as_str(),
            options.wifi_size.as_str(),
        ])
        .status()?;
    if !erased.success() {
        return Err("espflash could not erase wifi_cfg".into());
    }

    let flashed = Command::new(&options.espflash)
        .args([
            "flash",
            "--chip",
            options.chip.as_str(),
            "--port",
            port,
            "--non-interactive",
            "--skip-update-check",
            "--partition-table",
        ])
        .arg(&options.partition_table)
        .arg(&options.firmware)
        .status()?;
    if !flashed.success() {
        return Err("espflash could not flash the HIL firmware".into());
    }
    Ok(())
}

fn assert_status(
    hil: &mut Hil,
    state: &mut State,
    request: &[u8],
    expected: u16,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let response = request_response(hil, state, request)?;
    let prefix = format!("HTTP/1.1 {expected} ");
    if !response.starts_with(prefix.as_bytes()) {
        return Err(format!("unexpected HTTP status; expected {expected}").into());
    }
    Ok(response)
}

fn request_response(
    hil: &mut Hil,
    state: &mut State,
    request: &[u8],
) -> Result<Vec<u8>, Box<dyn Error>> {
    hil.send_http(request)?;
    loop {
        match hil.next(Duration::from_secs(30))? {
            OwnedFrame::Response(response) => return Ok(response),
            OwnedFrame::Event(event) => record_event(state, event),
            OwnedFrame::Other => {}
        }
    }
}

fn wait_until(
    hil: &mut Hil,
    state: &mut State,
    timeout: Duration,
    ready: impl Fn(&State) -> bool,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    while !ready(state) {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("timed out waiting for expected HIL events")?;
        if let OwnedFrame::Event(event) = hil.next(remaining)? {
            if event.ends_with("-failed") || event.starts_with("association-failed:") {
                return Err(format!("HIL device reported {event}").into());
            }
            record_event(state, event);
        }
    }
    Ok(())
}

fn record_event(state: &mut State, event: String) {
    if let Some(token) = event.strip_prefix("health-token:") {
        state.health_token = Some(token.to_owned());
    } else if let Some(address) = event.strip_prefix("dhcp:") {
        state.address = address.parse().ok();
    }
    state.events.insert(event);
}

fn check_health(state: &State) -> Result<(), Box<dyn Error>> {
    let address = state.address.ok_or("missing DHCP address")?;
    let token = state
        .health_token
        .as_deref()
        .ok_or("missing health token")?;
    let mut stream =
        TcpStream::connect_timeout(&SocketAddr::new(address, 80), Duration::from_secs(5))
            .map_err(|error| format!("LAN health connection to {address}: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "GET /health?token={token} HTTP/1.1\r\nHost: device\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = [0_u8; 128];
    let count = stream.read(&mut response)?;
    if !response[..count].starts_with(b"HTTP/1.1 200 ") {
        return Err("station health check failed".into());
    }
    Ok(())
}

fn form_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write as _;
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{network_rssi, response_body};

    #[test]
    fn finds_only_complete_json_escaped_ssids() {
        let response = br#"[{"ssid":"Home Network","rssi":-30,"secure":true}]"#;

        assert_eq!(network_rssi(response, "Home Network"), Some(-30));
        assert_eq!(network_rssi(response, "Home"), None);
    }

    #[test]
    fn extracts_http_response_body() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";

        assert_eq!(response_body(response), Some("[]"));
    }
}
