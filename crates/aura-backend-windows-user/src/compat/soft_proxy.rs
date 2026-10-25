//! A retained loopback reject proxy, affecting only the selected child's environment.
use anyhow::Result;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

pub(crate) struct SoftProxy {
    pub(crate) endpoint: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl SoftProxy {
    pub(crate) fn open() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::Builder::new().name("aura-shell-reject-proxy".into()).spawn(
            move || {
                while !stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => refuse(stream),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(_) => break,
                    }
                }
            },
        )?;
        Ok(Self { endpoint, stop, worker: Some(worker) })
    }
}
fn refuse(mut stream: TcpStream) {
    // Winsock accepts inherit the listener's nonblocking property. Reads below
    // must honor their timeout rather than immediately returning WouldBlock.
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
    // Consume bounded headers before closing, so an HTTP client observes the
    // refusal rather than a reset caused by unread inbound bytes. Never forward.
    let mut header = Vec::new();
    let mut bytes = [0; 512];
    while header.len() < 8192 {
        match stream.read(&mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                header.extend_from_slice(&bytes[..count]);
                if header.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    let _ = stream
        .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
}
impl Drop for SoftProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
