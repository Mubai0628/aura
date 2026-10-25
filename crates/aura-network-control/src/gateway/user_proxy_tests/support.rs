use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
};

pub(super) const CERTIFICATE: &[u8] =
    include_bytes!("../../../../../fixtures/network/l01/certificate.pem");
// Public, synthetic test key already used by the P07 local HTTPS fixture. No owner credential.
const KEY: &[u8] = br#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgWnMihkcZABZdwVb3
6z7FPjDKVr5urHHr0F0i2+c6WmqhRANCAATghvDuEXFevIkWyS8vQDeTQF7VZBFC
/9J/SyLQ9KgQZIMV6E/sY5Fh/5a+rF4iiFiXDZnGPbHYvT+C/hUzg7RK
-----END PRIVATE KEY-----
"#;

trait FixtureIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> FixtureIo for T {}

#[derive(Clone)]
pub(super) struct Reply {
    pub status: u16,
    pub location: Option<String>,
    pub body: Vec<u8>,
    pub interrupt: bool,
    pub headers: Vec<(String, String)>,
}
impl Reply {
    pub fn ok() -> Self {
        Self {
            status: 200,
            location: None,
            body: b"local proxy fixture".to_vec(),
            interrupt: false,
            headers: vec![],
        }
    }
    pub fn redirect(url: String) -> Self {
        Self { status: 302, location: Some(url), ..Self::ok() }
    }
}

pub(super) struct Target {
    pub address: SocketAddr,
    pub requests: Arc<Mutex<Vec<Vec<u8>>>>,
    pub(super) task: JoinHandle<()>,
}
impl Drop for Target {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Target {
    pub fn url(&self, scheme: &str, host: &str) -> String {
        format!("{scheme}://{host}:{}/resource", self.address.port())
    }
}

pub(super) async fn target(tls: bool, replies: Vec<Reply>) -> Target {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("target listener");
    let address = listener.local_addr().expect("target address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let acceptor = tls.then(|| tls_acceptor(vec![]));
    let task = tokio::spawn(async move {
        let mut workers = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (socket, _) = match accepted { Ok(value) => value, Err(_) => break };
                    let acceptor = acceptor.clone();
                    let observed = observed.clone();
                    let replies = replies.clone();
                    workers.spawn(async move {
                        let exchange = async {
                            let mut io: Box<dyn FixtureIo> = match acceptor {
                                Some(acceptor) => match acceptor.accept(socket).await { Ok(io) => Box::new(io), Err(_) => return },
                                None => Box::new(socket),
                            };
                            let mut bytes = match read_headers(&mut io).await { Ok(bytes) => bytes, Err(_) => return };
                            let head = String::from_utf8_lossy(&bytes);
                            let length = head.lines().find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
                            }).unwrap_or(0);
                            if length > 16_384 { return; }
                            let head_end = bytes.windows(4).position(|v| v == b"\r\n\r\n").expect("header end") + 4;
                            while bytes.len() < head_end + length {
                                let mut buf = [0_u8; 1024];
                                match io.read(&mut buf).await { Ok(0) | Err(_) => return, Ok(n) => bytes.extend_from_slice(&buf[..n]) }
                            }
                            let index = { let mut held = observed.lock().expect("requests"); let index = held.len(); held.push(bytes); index };
                            let reply = replies.get(index).or_else(|| replies.last()).cloned().unwrap_or_else(Reply::ok);
                            if reply.interrupt { return; }
                            let location = reply.location.map(|url| format!("Location: {url}\r\n")).unwrap_or_default();
                            let extra: String = reply.headers.iter().map(|(name, value)| format!("{name}: {value}\r\n")).collect();
                            let header = format!("HTTP/1.1 {} Fixture\r\nConnection: close\r\nContent-Length: {}\r\n{location}{extra}\r\n", reply.status, reply.body.len());
                            let _ = io.write_all(header.as_bytes()).await;
                            let _ = io.write_all(&reply.body).await;
                            let _ = io.shutdown().await;
                        };
                        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), exchange).await;
                    });
                }
                _ = workers.join_next(), if !workers.is_empty() => {}
            }
        }
    });
    Target { address, requests, task }
}

pub(super) async fn read_headers(
    io: &mut (impl AsyncRead + Unpin + ?Sized),
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let byte = io.read_u8().await?;
        bytes.push(byte);
        if bytes.ends_with(b"\r\n\r\n") {
            return Ok(bytes);
        }
        if bytes.len() > 16_384 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    }
}

pub(super) fn tls_acceptor(alpn_protocols: Vec<Vec<u8>>) -> TlsAcceptor {
    let certs = CertificateDer::pem_slice_iter(CERTIFICATE)
        .collect::<Result<Vec<_>, _>>()
        .expect("certificate");
    let key = PrivateKeyDer::from_pem_slice(KEY).expect("test key");
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("server TLS");
    config.alpn_protocols = alpn_protocols;
    TlsAcceptor::from(Arc::new(config))
}

pub(super) async fn capture_tunnel(
    client: TcpStream,
    target: TcpStream,
    capture: Arc<Mutex<Vec<u8>>>,
) -> std::io::Result<()> {
    let (mut client_read, mut client_write) = client.into_split();
    let (mut target_read, mut target_write) = target.into_split();
    let forward = async {
        let mut buf = [0_u8; 4096];
        loop {
            let n = client_read.read(&mut buf).await?;
            if n == 0 {
                target_write.shutdown().await?;
                return Ok::<_, std::io::Error>(());
            }
            capture.lock().expect("TLS bytes").extend_from_slice(&buf[..n]);
            target_write.write_all(&buf[..n]).await?;
        }
    };
    let backward = async {
        tokio::io::copy(&mut target_read, &mut client_write).await?;
        client_write.shutdown().await
    };
    tokio::try_join!(forward, backward).map(|_| ())
}
