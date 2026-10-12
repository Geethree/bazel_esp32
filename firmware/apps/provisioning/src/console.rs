use core::fmt::Write as _;

use crate::runtime::{PortalLock, SCAN_REQUESTS};
use board::Console;
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read, Write};
use heapless::String;
use wifi_provisioning::PortalStatus;

#[embassy_executor::task]
pub async fn serial_console(mut serial: Console, portal: &'static PortalLock) {
    let mut line = String::<48>::new();
    let _ = serial
        .write_all(b"\r\nESP32 serial console ready. Type help.\r\nesp32> ")
        .await;
    loop {
        let mut input = [0_u8; 64];
        let count = match Read::read(&mut serial, &mut input).await {
            Ok(count) => count,
            Err(_) => {
                Timer::after(Duration::from_millis(10)).await;
                continue;
            }
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

async fn handle_console_command(serial: &mut Console, portal: &'static PortalLock, command: &str) {
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
    let _ = Write::flush(serial).await;
}
