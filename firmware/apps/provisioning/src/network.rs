use core::{fmt::Write as _, net::Ipv4Addr, str::FromStr};

use crate::runtime::{GATEWAY, PortalLock, SUBMISSIONS};
use edge_nal::UdpBind;
use embassy_net::{
    IpListenEndpoint, Runner, Stack,
    tcp::TcpSocket,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_time::{Duration, Timer};
use embedded_io_async::Write as _;
use esp_radio::wifi::Interface;
use heapless::{String, Vec};
use wifi_provisioning::{PortalAction, index_html, is_index_request};

#[embassy_executor::task(pool_size = 2)]
pub async fn network_runner(mut runner: Runner<'static, Interface>) {
    runner.run().await;
}

#[embassy_executor::task]
pub async fn dhcp_server(stack: Stack<'static>) {
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
pub async fn dns_server(stack: Stack<'static>) {
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
pub async fn http_server(stack: Stack<'static>, portal: &'static PortalLock) {
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
