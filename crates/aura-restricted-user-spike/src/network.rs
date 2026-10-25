//! Loopback probes use disposable listeners only. They cannot prove Internet,
//! DNS or service-broker isolation and never return whole-Offline acceptance.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    io::ErrorKind,
    net::{TcpListener, TcpStream, UdpSocket},
    path::Path,
    time::Duration,
};

pub fn run(
    sid: &str,
    executable: &Path,
    root: &Path,
    device_install_id: Option<&str>,
) -> Result<Value> {
    let tcp4 = TcpListener::bind("127.0.0.1:0")?;
    let tcp6 = TcpListener::bind("[::1]:0")?;
    let udp4 = UdpSocket::bind("127.0.0.1:0")?;
    let udp6 = UdpSocket::bind("[::1]:0")?;
    let ports = [
        tcp4.local_addr()?.port(),
        tcp6.local_addr()?.port(),
        udp4.local_addr()?.port(),
        udp6.local_addr()?.port(),
    ];
    fs::write(root.join("loopback-ports.json"), serde_json::to_vec(&ports)?)?;
    let arguments = vec!["payload".into(), "loopback".into(), root.display().to_string()];
    let mut child =
        crate::monitor::launch(sid, executable, &arguments, root, device_install_id, None)?;
    let observation = crate::monitor::collect(&mut child, false, 8192)?;
    tcp4.set_nonblocking(true)?;
    tcp6.set_nonblocking(true)?;
    udp4.set_nonblocking(true)?;
    udp6.set_nonblocking(true)?;
    let mut data = [0u8; 64];
    let received = [
        tcp4.accept().is_ok(),
        tcp6.accept().is_ok(),
        udp4.recv_from(&mut data).is_ok(),
        udp6.recv_from(&mut data).is_ok(),
    ];
    let delivered = received.iter().any(|value| *value);
    Ok(json!({"scope":"loopback_tcp_udp_ipv4_ipv6","network_reached_listener":delivered,
        "received_by_protocol":received,
        "observation":observation,"offline_enforcement":"not_established",
        "missing":["controlled_LAN_Internet_peers","DNS_and_service_brokers","Home_Pro_matrix"]}))
}

pub fn payload(root: &Path) -> Result<()> {
    let ports: [u16; 4] = serde_json::from_slice(&fs::read(root.join("loopback-ports.json"))?)?;
    ensure!(ports.iter().all(|p| *p != 0), "invalid loopback fixture");
    let mut observations = Vec::new();
    for (index, port) in ports.into_iter().enumerate() {
        let host = if index % 2 == 0 { "127.0.0.1" } else { "::1" };
        let address = std::net::SocketAddr::new(host.parse()?, port);
        let result = if index < 2 {
            TcpStream::connect_timeout(&address, Duration::from_secs(1)).map(|_| ())
        } else {
            UdpSocket::bind(if index == 2 { "127.0.0.1:0" } else { "[::1]:0" })
                .and_then(|s| s.send_to(b"r06-fixed-probe", address).map(|_| ()))
        };
        observations.push(match result {
            Ok(()) => json!({"index":index,"status":"connected_or_sent"}),
            Err(error) => json!({"index":index,"status":if error.kind()==ErrorKind::PermissionDenied {"permission_denied"} else {"inconclusive"},"os_error":error.raw_os_error()}),
        });
    }
    println!("{}", json!({"observations":observations}));
    Ok(())
}
