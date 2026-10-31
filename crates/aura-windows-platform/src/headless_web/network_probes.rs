//! Pages and local peers for the render's network proofs. Every AppContainer deny is paired with an
//! unrestricted positive control that has to reach a local peer of the same kind, so a silent peer
//! is never taken for a deny.
use super::*;
use std::io::{self, Write};
use std::net::{TcpListener, UdpSocket};

fn start_unrestricted(page: &HeadlessPage) -> Result<HeadlessRenderSession, HeadlessRenderError> {
    start_headless_render_with(page, NetworkIsolation::None, Some(take_edge_slot_blocking()))
}

fn start_real_time(
    page: &HeadlessPage,
    isolation: NetworkIsolation,
) -> Result<HeadlessRenderSession, HeadlessRenderError> {
    start_headless_render_configured(
        page,
        isolation,
        Some(take_edge_slot_blocking()),
        None,
        Some(8_000),
    )
}

pub(super) fn render_unrestricted(
    page: &HeadlessPage,
    timeout: Duration,
) -> Result<RenderedHtml, HeadlessRenderError> {
    start_unrestricted(page)?.finish(timeout).into_rendered()
}

pub(super) fn render_real_time(
    page: &HeadlessPage,
    isolation: NetworkIsolation,
    timeout: Duration,
) -> Result<RenderedHtml, HeadlessRenderError> {
    start_real_time(page, isolation)?.finish(timeout).into_rendered()
}

pub(super) fn stun_endpoint() -> (UdpSocket, String, u16) {
    let probe = UdpSocket::bind("0.0.0.0:0").expect("probe");
    probe.connect("1.1.1.1:80").expect("route");
    let ip = match probe.local_addr().expect("probe addr").ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => ip,
        other => panic!("not_exercised: no non-loopback IPv4 for STUN ({other})"),
    };
    let socket = UdpSocket::bind((ip, 0)).expect("stun bind");
    let port = socket.local_addr().expect("stun addr").port();
    (socket, ip.to_string(), port)
}

pub(super) fn fetch_attempt_page(port: u16) -> String {
    format!(
        "<!doctype html><html><body data-attempt=\"pending\"><script>\
document.body.setAttribute('data-attempt','started');\
var ctrl=new AbortController();\
setTimeout(function(){{ctrl.abort();}},800);\
fetch('http://127.0.0.1:{port}/leak',{{mode:'no-cors',signal:ctrl.signal}}).then(function(){{\
document.body.setAttribute('data-attempt','resolved');\
}}).catch(function(err){{\
document.body.setAttribute('data-attempt','rejected');\
document.body.setAttribute('data-error', String(err && err.message || err));\
}});\
</script></body></html>"
    )
}

pub(super) fn rtc_hold_page(stun_host: &str, stun_port: u16, hold_port: u16) -> String {
    format!(
        "<!doctype html><html><body data-rtc=\"pending\">\
<img src=\"http://127.0.0.1:{hold_port}/hold\" alt=\"\">\
<script>\
try {{\
  document.body.setAttribute('data-rtc','started');\
  var pc = new RTCPeerConnection({{iceServers:[{{urls:'stun:{stun_host}:{stun_port}'}}],iceCandidatePoolSize:4,iceTransportPolicy:'all'}});\
  pc.createDataChannel('x');\
  pc.onicecandidate = function(ev){{\
    if(ev.candidate) document.body.setAttribute('data-ice','candidate');\
  }};\
  pc.createOffer().then(function(o){{return pc.setLocalDescription(o);}})\
    .then(function(){{document.body.setAttribute('data-rtc','offer');}})\
    .catch(function(err){{document.body.setAttribute('data-rtc','error'); document.body.setAttribute('data-error', String(err));}});\
}} catch (err) {{\
  document.body.setAttribute('data-rtc','unavailable');\
  document.body.setAttribute('data-error', String(err));\
}}\
</script></body></html>"
    )
}

pub(super) fn rtc_isolated_page(stun_host: &str, stun_port: u16, hold_port: u16) -> HeadlessPage {
    HeadlessPage {
        html: format!(
            "<!doctype html><html><body data-rtc=\"pending\"><script>\
try {{\
  document.body.setAttribute('data-rtc','started');\
  var pc = new RTCPeerConnection({{iceServers:[{{urls:'stun:{stun_host}:{stun_port}'}}],iceCandidatePoolSize:4,iceTransportPolicy:'all'}});\
  pc.createDataChannel('x');\
  pc.onicecandidate = function(ev){{\
    if(ev.candidate) document.body.setAttribute('data-ice','candidate');\
  }};\
  pc.createOffer().then(function(o){{return pc.setLocalDescription(o);}})\
    .then(function(){{document.body.setAttribute('data-rtc','offer');}})\
    .catch(function(err){{document.body.setAttribute('data-rtc','error'); document.body.setAttribute('data-error', String(err));}});\
}} catch (err) {{\
  document.body.setAttribute('data-rtc','unavailable');\
  document.body.setAttribute('data-error', String(err));\
}}\
fetch('http://127.0.0.1:{hold_port}/hold',{{mode:'no-cors'}}).catch(function(){{}});\
document.body.setAttribute('data-wait','fetch-hold');\
</script></body></html>"
        ),
        scripts: Vec::new(),
    }
}

pub(super) fn serve_delayed_http(listener: TcpListener, delay: Duration) {
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            thread::sleep(delay);
            let _ = stream.write_all(
                b"HTTP/1.1 204 No Content\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            );
        }
    });
}

/// A local peer that listens from its creation until it is asked whether it was reached. It has
/// no deadline of its own: a render can wait for the browser slot for any length of time, and a
/// peer that stopped on a clock could close before the browser had started.
pub(super) struct Peer {
    asked: Arc<AtomicBool>,
    listening: Option<thread::JoinHandle<bool>>,
}

impl Peer {
    fn listen(observe: impl FnOnce(&AtomicBool) -> bool + Send + 'static) -> Self {
        let asked = Arc::new(AtomicBool::new(false));
        let flag = asked.clone();
        Self { asked, listening: Some(thread::spawn(move || observe(&flag))) }
    }

    /// Ask once the render has returned: everything its browser sent has arrived by then.
    pub(super) fn reached(mut self) -> bool {
        self.asked.store(true, Ordering::SeqCst);
        self.listening.take().is_some_and(|listening| listening.join().expect("peer"))
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.asked.store(true, Ordering::SeqCst);
    }
}

pub(super) fn tcp_peer(listener: TcpListener) -> Peer {
    Peer::listen(move |asked| accept_tcp(&listener, asked))
}

pub(super) fn udp_peer(socket: UdpSocket) -> Peer {
    Peer::listen(move |asked| recv_udp(&socket, asked))
}

// The question is read before each attempt, so one more attempt follows it: a connection or a
// datagram that arrived before the question is still counted.
fn accept_tcp(listener: &TcpListener, asked: &AtomicBool) -> bool {
    listener.set_nonblocking(true).expect("nonblocking");
    loop {
        let last = asked.load(Ordering::SeqCst);
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.write_all(
                    b"HTTP/1.1 204 No Content\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                );
                return true;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(_) => return false,
        }
        if last {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn recv_udp(socket: &UdpSocket, asked: &AtomicBool) -> bool {
    socket.set_read_timeout(Some(Duration::from_millis(50))).expect("read timeout");
    let mut buf = [0_u8; 512];
    loop {
        let last = asked.load(Ordering::SeqCst);
        if socket.recv_from(&mut buf).is_ok() {
            return true;
        }
        if last {
            return false;
        }
    }
}

pub(super) fn network_evidence(
    name: &str,
    isolation: &str,
    api: &str,
    html: Option<&str>,
    peer: bool,
    error: Option<&str>,
) {
    eprintln!(
        "network_case={name} isolation={isolation} api={api} peer_reached={peer} error={error:?} html={}",
        html.unwrap_or("<none>")
    );
}

pub(super) fn fetch_attempted(html: &str) -> bool {
    html.contains("data-attempt=\"started\"")
        || html.contains("data-attempt=\"rejected\"")
        || html.contains("data-attempt=\"resolved\"")
        || html.contains("data-attempt=\"aborted\"")
}
