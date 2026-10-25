use super::support::{capture_tunnel, read_headers};
use aura_tool_contract::{ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};

#[derive(Clone, Copy)]
pub(super) enum Behavior {
    Relay,
    Refuse,
    Stall,
}

pub(super) struct Proxy {
    pub settings: ToolNetworkProxyV1,
    pub handshakes: Arc<Mutex<Vec<Vec<u8>>>>,
    pub destinations: Arc<Mutex<Vec<(String, u16, bool)>>>,
    pub tls_bytes: Arc<Mutex<Vec<u8>>>,
    task: JoinHandle<()>,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn proxy(
    protocol: ToolNetworkProxyProtocolV1,
    behavior: Behavior,
    targets: &[SocketAddr],
) -> Proxy {
    proxy_on_loopback(protocol, behavior, targets, "127.0.0.1:0").await
}

pub(super) async fn proxy_on_loopback(
    protocol: ToolNetworkProxyProtocolV1,
    behavior: Behavior,
    targets: &[SocketAddr],
    listen: &str,
) -> Proxy {
    assert!(targets.iter().all(|target| target.ip().is_loopback()));
    let targets: BTreeMap<_, _> =
        targets.iter().map(|address| (address.port(), *address)).collect();
    let listener = TcpListener::bind(listen).await.expect("proxy listener");
    let address = listener.local_addr().expect("proxy address");
    let settings =
        ToolNetworkProxyV1::new(true, protocol, address.ip().to_string(), address.port())
            .expect("proxy settings");
    let handshakes = Arc::new(Mutex::new(Vec::new()));
    let destinations = Arc::new(Mutex::new(Vec::new()));
    let tls_bytes = Arc::new(Mutex::new(Vec::new()));
    let h = handshakes.clone();
    let d = destinations.clone();
    let t = tls_bytes.clone();
    let task = tokio::spawn(async move {
        let mut workers = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (mut socket, _) = match accepted { Ok(v) => v, Err(_) => break };
                    let targets = targets.clone(); let h = h.clone(); let d = d.clone(); let t = t.clone();
                    workers.spawn(async move {
                        let exchange = async {
                            if matches!(behavior, Behavior::Stall) { let _ = socket.read_u8().await; std::future::pending::<()>().await; }
                            let (host, port, remote_dns) = match protocol {
                                ToolNetworkProxyProtocolV1::Http => {
                                    let first = socket.read_u8().await?;
                                    if first != b'C' { socket.write_all(b"HTTP/1.1 400 Wrong proxy protocol\r\nContent-Length: 0\r\n\r\n").await?; return Ok(()); }
                                    let mut headers = vec![first];
                                    headers.extend(read_headers(&mut socket).await?);
                                    h.lock().expect("handshakes").push(headers.clone());
                                    let text = String::from_utf8_lossy(&headers);
                                    let authority = text.lines().next().and_then(|line| line.strip_prefix("CONNECT ")).and_then(|v| v.split(' ').next())
                                        .ok_or(std::io::ErrorKind::InvalidData)?;
                                    let (host, port) = authority.rsplit_once(':').ok_or(std::io::ErrorKind::InvalidData)?;
                                    let port = port.parse::<u16>().map_err(|_| std::io::ErrorKind::InvalidData)?;
                                    if matches!(behavior, Behavior::Refuse) { socket.write_all(b"HTTP/1.1 403 Fixture policy\r\nContent-Length: 0\r\n\r\n").await?; return Ok(()); }
                                    socket.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await?;
                                    (host.trim_matches(['[', ']']).to_owned(), port, true)
                                }
                                ToolNetworkProxyProtocolV1::Socks5 => {
                                    let mut greeting = [0_u8; 2]; socket.read_exact(&mut greeting).await?;
                                    if greeting[0] != 5 || greeting[1] > 16 {
                                        socket.write_all(&[5, 255]).await?; return Ok(());
                                    }
                                    let mut methods = vec![0_u8; greeting[1] as usize]; socket.read_exact(&mut methods).await?;
                                    socket.write_all(&[5, 0]).await?;
                                    let mut command = [0_u8; 4]; socket.read_exact(&mut command).await?;
                                    let (host, raw, remote_dns) = match command[3] {
                                        3 => { let length = socket.read_u8().await?; let mut raw = vec![0_u8; length as usize]; socket.read_exact(&mut raw).await?;
                                            (String::from_utf8(raw.clone()).map_err(|_| std::io::ErrorKind::InvalidData)?, raw, true) },
                                        1 => { let mut raw = vec![0_u8; 4]; socket.read_exact(&mut raw).await?;
                                            (std::net::Ipv4Addr::new(raw[0], raw[1], raw[2], raw[3]).to_string(), raw, false) },
                                        _ => return Err(std::io::ErrorKind::InvalidData.into()),
                                    };
                                    let port = socket.read_u16().await?;
                                    let mut handshake = greeting.to_vec(); handshake.extend(methods); handshake.extend(command); handshake.extend(raw); handshake.extend(port.to_be_bytes());
                                    h.lock().expect("handshakes").push(handshake);
                                    if matches!(behavior, Behavior::Refuse) { socket.write_all(&[5, 2, 0, 1, 0, 0, 0, 0, 0, 0]).await?; return Ok(()); }
                                    socket.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await?;
                                    (host, port, remote_dns)
                                }
                            };
                            d.lock().expect("destinations").push((host, port, remote_dns));
                            let address = targets.get(&port).ok_or(std::io::ErrorKind::PermissionDenied)?;
                            let target = TcpStream::connect(address).await?;
                            capture_tunnel(socket, target, t).await
                        };
                        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), exchange).await;
                    });
                }
                _ = workers.join_next(), if !workers.is_empty() => {}
            }
        }
    });
    Proxy { settings, handshakes, destinations, tls_bytes, task }
}
