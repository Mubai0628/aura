use super::{UserProxyFailureKindV1 as Kind, failure};
use crate::NetworkControlError;
use aura_tool_contract::{ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

const LOOPBACK_CONNECT: std::time::Duration = std::time::Duration::from_millis(500);

pub(super) fn check_endpoint(endpoint: SocketAddr) -> Result<(), NetworkControlError> {
    if !endpoint.ip().is_loopback() {
        return Err(NetworkControlError::DestinationBlocked);
    }
    crate::ordinary_tools::check_destination(&aura_security_contract::ResolvedNetworkDestination {
        scheme: "http".into(),
        host: endpoint.ip().to_string(),
        port: endpoint.port(),
        addresses: vec![endpoint.ip().to_string()],
        classes: vec![aura_security_contract::NetworkDestinationClass::Loopback],
    })
}

pub(super) async fn connect(
    proxy: &ToolNetworkProxyV1,
    host: &str,
    port: u16,
) -> Result<TcpStream, NetworkControlError> {
    let addresses =
        proxy.loopback_endpoints().map_err(|_| NetworkControlError::DestinationBlocked)?;
    use futures_util::{StreamExt, stream::FuturesUnordered};
    let mut connecting = FuturesUnordered::new();
    for address in addresses {
        connecting.push(async move {
            // Check at the actual connect; racing localhost's two literals cannot be redirected
            // by system DNS, and an IPv4 refusal must not hide a working IPv6 listener.
            check_endpoint(address)?;
            // A running loopback proxy completes the TCP handshake at once. Windows retries a
            // refused loopback connect for about two seconds, and a stopped proxy must not
            // add that to every tool call before the direct attempt.
            match tokio::time::timeout(LOOPBACK_CONNECT, TcpStream::connect(address)).await {
                Ok(Ok(socket)) => Ok(socket),
                Ok(Err(_)) | Err(_) => {
                    Err(failure(proxy, Kind::Unavailable, "无法连接代理端口（拒绝连接或连接失败）"))
                }
            }
        });
    }
    let mut stream = None;
    while let Some(result) = connecting.next().await {
        match result {
            Ok(socket) => {
                stream = Some(socket);
                break;
            }
            Err(NetworkControlError::UserProxyFailure(_)) => {}
            Err(error) => return Err(error),
        }
    }
    let mut stream = stream.ok_or_else(|| {
        failure(proxy, Kind::Unavailable, "无法连接代理端口（拒绝连接或连接失败）")
    })?;
    match proxy.protocol {
        ToolNetworkProxyProtocolV1::Http => http_connect(proxy, &mut stream, host, port).await?,
        ToolNetworkProxyProtocolV1::Socks5 => socks_connect(proxy, &mut stream, host, port).await?,
    }
    Ok(stream)
}

async fn http_connect(
    proxy: &ToolNetworkProxyV1,
    stream: &mut TcpStream,
    host: &str,
    port: u16,
) -> Result<(), NetworkControlError> {
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_owned() };
    let authority = format!("{host}:{port}");
    let message = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    stream
        .write_all(message.as_bytes())
        .await
        .map_err(|_| failure(proxy, Kind::Unavailable, "代理连接在 CONNECT 握手中中断"))?;
    let mut header = Vec::new();
    loop {
        let byte = stream
            .read_u8()
            .await
            .map_err(|_| failure(proxy, Kind::Protocol, "HTTP CONNECT 响应无效或不完整"))?;
        header.push(byte);
        if header.len() > 16_384 {
            return Err(failure(proxy, Kind::Protocol, "HTTP CONNECT 响应头过长"));
        }
        if header.len() >= 5 && !header.starts_with(b"HTTP/") {
            return Err(failure(proxy, Kind::Protocol, "代理未返回 HTTP CONNECT 响应"));
        }
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let line = header.split(|byte| *byte == b'\n').next().unwrap_or_default();
    let text = std::str::from_utf8(line)
        .map_err(|_| failure(proxy, Kind::Protocol, "HTTP CONNECT 状态行无效"))?;
    let mut parts = text.trim_end_matches('\r').splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    let status = parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|status| (100..600).contains(status))
        .ok_or_else(|| failure(proxy, Kind::Protocol, "HTTP CONNECT 状态码无效"))?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(failure(proxy, Kind::Protocol, "HTTP CONNECT 版本无效"));
    }
    if !(200..300).contains(&status) {
        let reason: String = parts
            .next()
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .collect();
        return Err(failure(proxy, Kind::Refused, format!("HTTP CONNECT 返回 {status} {reason}")));
    }
    Ok(())
}

async fn socks_connect(
    proxy: &ToolNetworkProxyV1,
    stream: &mut TcpStream,
    host: &str,
    port: u16,
) -> Result<(), NetworkControlError> {
    stream
        .write_all(&[5, 1, 0])
        .await
        .map_err(|_| failure(proxy, Kind::Unavailable, "SOCKS5 协商时连接中断"))?;
    let mut method = [0_u8; 2];
    stream
        .read_exact(&mut method)
        .await
        .map_err(|_| failure(proxy, Kind::Protocol, "SOCKS5 方法协商响应不完整"))?;
    if method[0] != 5 {
        return Err(failure(proxy, Kind::Protocol, "代理未返回 SOCKS5 响应"));
    }
    if method[1] == 255 {
        return Err(failure(proxy, Kind::Refused, "SOCKS5 拒绝无认证连接（不支持代理认证）"));
    }
    if method[1] != 0 {
        return Err(failure(proxy, Kind::Protocol, "SOCKS5 代理要求未声明的认证方法"));
    }
    // Names use ATYP=3 and remote DNS (socks5h); URL IP literals retain their IP wire types.
    let mut request = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            let mut request = vec![5, 1, 0, 1];
            request.extend(ip.octets());
            request
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            let mut request = vec![5, 1, 0, 4];
            request.extend(ip.octets());
            request
        }
        Err(_) => {
            let length = u8::try_from(host.len())
                .map_err(|_| failure(proxy, Kind::Protocol, "SOCKS5 目标域名过长"))?;
            let mut request = vec![5, 1, 0, 3, length];
            request.extend_from_slice(host.as_bytes());
            request
        }
    };
    request.extend_from_slice(&port.to_be_bytes());
    stream
        .write_all(&request)
        .await
        .map_err(|_| failure(proxy, Kind::Unavailable, "SOCKS5 请求隧道时连接中断"))?;
    let mut reply = [0_u8; 4];
    stream
        .read_exact(&mut reply)
        .await
        .map_err(|_| failure(proxy, Kind::Protocol, "SOCKS5 隧道响应不完整"))?;
    if reply[0] != 5 || reply[2] != 0 {
        return Err(failure(proxy, Kind::Protocol, "SOCKS5 隧道响应格式无效"));
    }
    if reply[1] != 0 {
        let reason = match reply[1] {
            1 => "代理内部错误",
            2 => "代理规则拒绝",
            3 => "网络不可达",
            4 => "主机不可达",
            5 => "目标拒绝连接",
            6 => "TTL 已过期",
            7 => "不支持 CONNECT",
            8 => "不支持地址类型",
            _ => "未知拒绝原因",
        };
        return Err(failure(proxy, Kind::Refused, format!("SOCKS5 返回 {}：{reason}", reply[1])));
    }
    let length = match reply[3] {
        1 => 4,
        4 => 16,
        3 => stream
            .read_u8()
            .await
            .map_err(|_| failure(proxy, Kind::Protocol, "SOCKS5 响应地址不完整"))?
            as usize,
        _ => return Err(failure(proxy, Kind::Protocol, "SOCKS5 响应地址类型无效")),
    };
    let mut address = vec![0_u8; length + 2];
    stream
        .read_exact(&mut address)
        .await
        .map_err(|_| failure(proxy, Kind::Protocol, "SOCKS5 响应地址不完整"))?;
    Ok(())
}

/// Local handshake only. HTTP checks that the port accepts a connection. SOCKS5 finishes
/// no-auth method negotiation and then stops. Neither form opens a tunnel or sends a target.
pub(super) async fn probe(
    proxy: &ToolNetworkProxyV1,
    budget: std::time::Duration,
) -> Result<(), NetworkControlError> {
    match tokio::time::timeout(budget, probe_once(proxy)).await {
        Ok(result) => result,
        Err(_) => Err(failure(proxy, Kind::Unavailable, "超时")),
    }
}

async fn probe_once(proxy: &ToolNetworkProxyV1) -> Result<(), NetworkControlError> {
    use futures_util::{StreamExt, stream::FuturesUnordered};
    let addresses =
        proxy.loopback_endpoints().map_err(|_| NetworkControlError::DestinationBlocked)?;
    let mut connecting = FuturesUnordered::new();
    for address in addresses {
        let proxy = proxy.clone();
        connecting.push(async move {
            check_endpoint(address)?;
            match TcpStream::connect(address).await {
                Ok(stream) => Ok(stream),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    Err(failure(&proxy, Kind::Unavailable, "拒绝连接"))
                }
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                    Err(failure(&proxy, Kind::Unavailable, "超时"))
                }
                Err(_) => Err(failure(
                    &proxy,
                    Kind::Unavailable,
                    "无法连接代理端口（拒绝连接或连接失败）",
                )),
            }
        });
    }
    let mut last = failure(proxy, Kind::Unavailable, "拒绝连接");
    let mut stream = None;
    while let Some(result) = connecting.next().await {
        match result {
            Ok(socket) => {
                stream = Some(socket);
                break;
            }
            Err(NetworkControlError::UserProxyFailure(inner)) => {
                last = NetworkControlError::UserProxyFailure(inner);
            }
            Err(error) => return Err(error),
        }
    }
    let mut stream = stream.ok_or(last)?;
    if proxy.protocol == ToolNetworkProxyProtocolV1::Socks5 {
        socks_method(&mut stream, proxy).await?;
    }
    let _ = stream.shutdown().await;
    Ok(())
}

async fn socks_method(
    stream: &mut TcpStream,
    proxy: &ToolNetworkProxyV1,
) -> Result<(), NetworkControlError> {
    stream.write_all(&[5, 1, 0]).await.map_err(|_| failure(proxy, Kind::Protocol, "协议不符"))?;
    let mut method = [0_u8; 2];
    stream.read_exact(&mut method).await.map_err(|_| failure(proxy, Kind::Protocol, "协议不符"))?;
    if method != [5, 0] {
        return Err(failure(proxy, Kind::Protocol, "协议不符"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn socks_literals_use_ip_address_frames_without_remote_dns() {
        for ip in ["8.8.8.8", "2606:4700:4700::1111"] {
            let address: std::net::IpAddr = ip.parse().expect("literal");
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fixture proxy");
            let proxy = ToolNetworkProxyV1::new(
                true,
                ToolNetworkProxyProtocolV1::Socks5,
                "127.0.0.1".into(),
                listener.local_addr().expect("address").port(),
            )
            .expect("proxy");
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.expect("connect");
                let mut greeting = [0_u8; 3];
                socket.read_exact(&mut greeting).await.expect("method");
                assert_eq!(greeting, [5, 1, 0]);
                socket.write_all(&[5, 0]).await.expect("method accepted");
                let mut header = [0_u8; 4];
                socket.read_exact(&mut header).await.expect("request");
                let (kind, expected) = match address {
                    std::net::IpAddr::V4(ip) => (1, ip.octets().to_vec()),
                    std::net::IpAddr::V6(ip) => (4, ip.octets().to_vec()),
                };
                assert_eq!(header, [5, 1, 0, kind]);
                let mut bytes = vec![0_u8; expected.len()];
                socket.read_exact(&mut bytes).await.expect("IP");
                assert_eq!(bytes, expected);
                assert_eq!(socket.read_u16().await.expect("port"), 443);
                socket.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await.expect("connected");
            });
            tokio::time::timeout(std::time::Duration::from_secs(2), connect(&proxy, ip, 443))
                .await
                .expect("bounded")
                .expect("IP tunnel");
            server.await.expect("fixture");
        }
    }

    fn proxy(protocol: ToolNetworkProxyProtocolV1, port: u16) -> ToolNetworkProxyV1 {
        ToolNetworkProxyV1::new(true, protocol, "127.0.0.1".into(), port).expect("loopback proxy")
    }

    fn reason(error: NetworkControlError) -> String {
        match error {
            NetworkControlError::UserProxyFailure(failure) => failure.reason,
            other => panic!("expected a proxy failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_probe_only_checks_the_port_and_sends_nothing() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let port = listener.local_addr().expect("address").port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = std::sync::Arc::clone(&seen);
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buffer = [0_u8; 16];
            let read = tokio::time::timeout(
                std::time::Duration::from_millis(200),
                socket.read(&mut buffer),
            )
            .await;
            let bytes = match read {
                Ok(Ok(count)) => buffer[..count].to_vec(),
                _ => Vec::new(),
            };
            *recorded.lock().expect("bytes") = bytes;
        });
        probe(&proxy(ToolNetworkProxyProtocolV1::Http, port), std::time::Duration::from_secs(2))
            .await
            .expect("open port");
        server.await.expect("server");
        assert!(seen.lock().expect("bytes").is_empty(), "HTTP probe must not send CONNECT");
    }

    #[tokio::test]
    async fn a_stopped_proxy_is_given_up_without_the_windows_refusal_wait() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("address").port();
        drop(listener);
        let started = std::time::Instant::now();
        let error = connect(&proxy(ToolNetworkProxyProtocolV1::Http, port), "example.test", 443)
            .await
            .expect_err("nothing listens");
        assert!(matches!(error, NetworkControlError::UserProxyFailure(_)), "{error:?}");
        // Windows retries a refused loopback connect for about two seconds; each tool call
        // with a stopped proxy must not wait that long before it can go direct.
        assert!(
            started.elapsed() < std::time::Duration::from_millis(1_500),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_closed_port_is_connection_refused() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("address").port();
        drop(listener);
        let error = probe(
            &proxy(ToolNetworkProxyProtocolV1::Http, port),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect_err("closed");
        assert_eq!(reason(error), "拒绝连接");
    }

    #[tokio::test]
    async fn socks_probe_stops_after_method_negotiation() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let port = listener.local_addr().expect("address").port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut greeting = [0_u8; 3];
            socket.read_exact(&mut greeting).await.expect("greeting");
            assert_eq!(greeting, [5, 1, 0]);
            socket.write_all(&[5, 0]).await.expect("no auth");
            let mut extra = [0_u8; 8];
            let read = tokio::time::timeout(
                std::time::Duration::from_millis(200),
                socket.read(&mut extra),
            )
            .await;
            let sent_a_request = matches!(read, Ok(Ok(count)) if count > 0);
            assert!(!sent_a_request, "no CONNECT request follows");
        });
        probe(&proxy(ToolNetworkProxyProtocolV1::Socks5, port), std::time::Duration::from_secs(2))
            .await
            .expect("socks method");
        server.await.expect("server");
    }

    #[tokio::test]
    async fn socks_probe_reports_a_protocol_mismatch() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let port = listener.local_addr().expect("address").port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut greeting = [0_u8; 3];
            let _ = socket.read_exact(&mut greeting).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.expect("not socks");
        });
        let error = probe(
            &proxy(ToolNetworkProxyProtocolV1::Socks5, port),
            std::time::Duration::from_secs(2),
        )
        .await
        .expect_err("http is not socks");
        assert_eq!(reason(error), "协议不符");
        server.await.expect("server");
    }

    #[tokio::test]
    async fn a_silent_peer_is_a_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let port = listener.local_addr().expect("address").port();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.expect("accept");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        });
        let error = probe(
            &proxy(ToolNetworkProxyProtocolV1::Socks5, port),
            std::time::Duration::from_millis(200),
        )
        .await
        .expect_err("stall");
        assert_eq!(reason(error), "超时");
        server.abort();
    }

    #[tokio::test]
    async fn a_registered_control_endpoint_is_refused_before_connect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let registration = crate::ordinary_tools::register_control_endpoint(
            aura_domain_common::RuntimeInstanceId::new(),
            address,
        )
        .expect("register");
        let error = probe(
            &proxy(ToolNetworkProxyProtocolV1::Http, address.port()),
            std::time::Duration::from_secs(2),
        )
        .await
        .expect_err("control endpoint");
        assert!(matches!(error, NetworkControlError::ControlEndpointBlocked));
        drop(registration);
    }
}
