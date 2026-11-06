//! The connected supervisor as the launcher of the child session's host. Starting a host is the
//! order `start`; the host's lines come as the supervisor passes them, from its `started` on. What
//! comes before that, such as the last lines and the `ended` of a host Core no longer counts on,
//! is dropped. When the supervisor disconnects, the host Core counts on has ended, and no other
//! starts until a new opening connects.
use super::Supervisor;
use crate::launch::{HostEvent, HostLauncher, HostProcess, Lines};
use aura_desktop_domain::child::{
    ChildLine, HOST_LINE, MAX_CHILD_LINE_BYTES, SupervisorOrder, SupervisorSays, read_child_line,
    supervisor_line,
};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Why no host can start while no supervisor is connected.
const NOT_CONNECTED: &str = "小窗没有连上子会话";
const DISCONNECTED: &str = "子会话连接已中断，请关闭小窗后重新打开并登录；没有执行";

struct Current {
    number: u64,
    events: mpsc::UnboundedSender<HostEvent>,
    started: bool,
}

#[derive(Default)]
struct Shared {
    orders: Option<mpsc::UnboundedSender<Vec<u8>>>,
    current: Option<Current>,
    why: Option<String>,
    numbers: u64,
}

impl Shared {
    /// The host Core counted on ended: it hears so, once.
    fn ended(&mut self) {
        if let Some(current) = self.current.take() {
            let _ = current.events.send(HostEvent::Ended);
        }
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The launcher of the child session's host, through the supervisor connected now.
#[derive(Clone, Default)]
pub struct ChildHost {
    shared: Arc<Mutex<Shared>>,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl ChildHost {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Serves the supervisor that was let in, in place of any earlier one.
    pub fn attach(&self, supervisor: Supervisor) {
        self.close();
        let (orders, mut pending) = mpsc::unbounded_channel::<Vec<u8>>();
        let (mut reader, mut writer) = tokio::io::split(supervisor.pipe);
        {
            let mut shared = lock(&self.shared);
            shared.orders = Some(orders.clone());
            shared.why = None;
        }
        let shared = Arc::clone(&self.shared);
        let rest = supervisor.rest;
        let read = tokio::spawn(async move {
            let mut lines = Lines::with_limit(MAX_CHILD_LINE_BYTES);
            let mut chunk = vec![0_u8; 64 * 1024];
            let mut events = lines.push(&rest);
            loop {
                for event in events.drain(..) {
                    heard(&shared, &orders, event);
                }
                match reader.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(count) => events = lines.push(&chunk[..count]),
                }
            }
            let mut shared = lock(&shared);
            if shared.orders.as_ref().is_some_and(|sender| sender.same_channel(&orders)) {
                shared.orders = None;
                shared.why = Some(DISCONNECTED.to_owned());
                shared.ended();
            }
        });
        let write = tokio::spawn(async move {
            while let Some(line) = pending.recv().await {
                if writer.write_all(&line).await.is_err() || writer.flush().await.is_err() {
                    break;
                }
            }
        });
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        tasks.push(read);
        tasks.push(write);
    }

    /// Whether a supervisor is connected now.
    #[must_use]
    pub fn connected(&self) -> bool {
        lock(&self.shared).orders.is_some()
    }

    /// Ends only the current host. The authenticated supervisor remains for a later turn.
    pub fn interrupt(&self) {
        let mut shared = lock(&self.shared);
        if let Some(orders) = shared.orders.as_ref() {
            let _ = orders.send(supervisor_line(&SupervisorOrder::Stop));
        }
        shared.ended();
    }

    /// Disconnects the supervisor, which ends its host and itself.
    pub fn close(&self) {
        for task in self.tasks.lock().unwrap_or_else(PoisonError::into_inner).drain(..) {
            task.abort();
        }
        let mut shared = lock(&self.shared);
        shared.orders = None;
        shared.why = Some(NOT_CONNECTED.to_owned());
        shared.ended();
    }
}

/// One line the supervisor passed or said, for the host Core counts on.
fn heard(shared: &Mutex<Shared>, origin: &mpsc::UnboundedSender<Vec<u8>>, event: HostEvent) {
    let mut shared = lock(shared);
    if !shared.orders.as_ref().is_some_and(|current| current.same_channel(origin)) {
        return;
    }
    let line = match event {
        HostEvent::Line(line) => line,
        HostEvent::TooLong => {
            if let Some(current) = shared.current.as_ref().filter(|current| current.started) {
                let _ = current.events.send(HostEvent::TooLong);
            }
            return;
        }
        HostEvent::Ended => return,
    };
    match read_child_line::<SupervisorSays>(&line) {
        Ok(ChildLine::Host(host)) => {
            if let Some(current) = shared.current.as_ref().filter(|current| current.started) {
                let _ = current.events.send(HostEvent::Line(host.to_vec()));
            }
        }
        Ok(ChildLine::Supervisor(SupervisorSays::Started)) => {
            if let Some(current) = shared.current.as_mut() {
                current.started = true;
            }
        }
        Ok(ChildLine::Supervisor(SupervisorSays::NotStarted { .. })) => {
            if shared.current.as_ref().is_some_and(|current| !current.started) {
                shared.ended();
            }
        }
        Ok(ChildLine::Supervisor(SupervisorSays::Ended)) => {
            if shared.current.as_ref().is_some_and(|current| current.started) {
                shared.ended();
            }
        }
        Ok(ChildLine::Supervisor(SupervisorSays::Hello { .. })) | Err(_) => {}
    }
}

struct ChildProcess {
    shared: Arc<Mutex<Shared>>,
    number: u64,
}

impl HostLauncher for ChildHost {
    fn start(
        &self,
        events: mpsc::UnboundedSender<HostEvent>,
    ) -> Result<Box<dyn HostProcess>, String> {
        let mut shared = lock(&self.shared);
        let Some(orders) = shared.orders.clone() else {
            return Err(shared.why.clone().unwrap_or_else(|| NOT_CONNECTED.to_owned()));
        };
        shared.ended();
        shared.numbers += 1;
        let number = shared.numbers;
        orders
            .send(supervisor_line(&SupervisorOrder::Start))
            .map_err(|_| DISCONNECTED.to_owned())?;
        shared.current = Some(Current { number, events, started: false });
        Ok(Box::new(ChildProcess { shared: Arc::clone(&self.shared), number }))
    }
}

impl HostProcess for ChildProcess {
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()> {
        let shared = lock(&self.shared);
        if shared.current.as_ref().is_none_or(|current| current.number != self.number) {
            anyhow::bail!("这个操控主机已经结束");
        }
        let orders = shared.orders.as_ref().ok_or_else(|| anyhow::anyhow!(DISCONNECTED))?;
        let mut framed = Vec::with_capacity(line.len() + 2);
        framed.push(HOST_LINE);
        framed.extend_from_slice(&line);
        if framed.last() != Some(&b'\n') {
            framed.push(b'\n');
        }
        orders.send(framed).map_err(|_| anyhow::anyhow!(DISCONNECTED))
    }

    fn stop(&mut self) {
        let mut shared = lock(&self.shared);
        if shared.current.as_ref().is_some_and(|current| current.number == self.number) {
            shared.current = None;
            if let Some(orders) = shared.orders.as_ref() {
                let _ = orders.send(supervisor_line(&SupervisorOrder::Stop));
            }
        }
    }
}
