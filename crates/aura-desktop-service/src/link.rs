//! The one desktop host: started the first time it is needed, one request at a time, each with a
//! time limit. A host that ended is started again for the next request; a host that did not
//! answer in time is ended, and the request reports that whether it reached the program is not
//! known. What the host says by itself between replies, such as a hold it gave back, is read as
//! it comes.
use crate::launch::{HostEvent, HostLauncher, HostProcess};
use aura_desktop_domain::{
    DESKTOP_PROTOCOL_VERSION, Delivery, HoldEnd, HostCommand, HostError, HostErrorCode,
    HostMessage, HostOutcome, HostRequest, HostResult,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use tokio::time::Instant;

/// How long a host that was just started has to say it is ready.
pub const READY_WITHIN: Duration = Duration::from_secs(10);
/// What a request may take on top of its own limit: the host's own checks and the line itself.
pub const REPLY_MARGIN: Duration = Duration::from_secs(5);
/// The time limit of giving a hold back.
const RELEASE_LIMIT: Duration = Duration::from_secs(5);
/// The most holds whose end the link remembers.
const ENDS_REMEMBERED: usize = 64;

/// Why a request got no reply from the host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkError {
    /// The host could not be started, or did not say it was ready. Nothing was sent.
    NotStarted(String),
    /// The host did not answer in time and was ended.
    TimedOut,
    /// The host ended while the request was out.
    Ended,
    /// The host's line was not a reply.
    Garbled(String),
}

impl LinkError {
    /// The failure as the host's own error: what the model reads.
    #[must_use]
    pub fn into_host_error(self) -> HostError {
        let unsure = Delivery::Uncertain;
        let (code, message, delivery) = match self {
            Self::NotStarted(why) => (
                HostErrorCode::Internal,
                format!("电脑操控的宿主程序没有启动：{why}"),
                Delivery::NotDelivered,
            ),
            Self::TimedOut => (
                HostErrorCode::Timeout,
                "电脑操控没有及时回应，已结束它，下次会重新启动".to_owned(),
                unsure,
            ),
            Self::Ended => {
                (HostErrorCode::Internal, "电脑操控的宿主程序意外退出".to_owned(), unsure)
            }
            Self::Garbled(why) => {
                (HostErrorCode::Internal, format!("电脑操控返回了无法识别的结果：{why}"), unsure)
            }
        };
        HostError::new(code, message, delivery)
    }
}

type Message = Result<HostMessage, LinkError>;

/// What a request does to the hold it names.
#[derive(Clone, Copy)]
enum ToHold {
    /// An action of the hold keeps the desktop for it.
    Act,
    Release,
    Keep,
}

struct Running {
    process: Box<dyn HostProcess>,
    events: mpsc::UnboundedReceiver<Message>,
    next_id: u64,
}

/// The hold the host keeps, as its last words said.
#[derive(Default)]
struct Holds {
    /// Counts the hosts started and ended: what the reader of an ended host says changes
    /// nothing.
    host: u64,
    held: Option<String>,
    /// How holds ended lately, the newest last, with what of the user's took one over when the
    /// host said.
    ended: VecDeque<(String, HoldEnd, Option<String>)>,
}

impl Holds {
    fn ended(&mut self, key: &str, reason: HoldEnd, cause: Option<String>) {
        if self.held.as_deref() == Some(key) {
            self.held = None;
        }
        let said = self
            .ended
            .iter()
            .find(|(ended, end, _)| ended == key && *end == reason)
            .and_then(|(_, _, cause)| cause.clone());
        self.ended.retain(|(ended, _, _)| ended != key);
        if self.ended.len() == ENDS_REMEMBERED {
            self.ended.pop_front();
        }
        self.ended.push_back((key.to_owned(), reason, cause.or(said)));
    }

    /// The host answered an action of hold `key`. A hold the user took over is never kept again,
    /// so when the host said so before answering, the user took it over after the action's last
    /// step. Any other end it said was of the hold before: the action took the desktop again.
    fn answered(&mut self, key: String) {
        if self
            .ended
            .iter()
            .any(|(ended, reason, _)| *ended == key && *reason == HoldEnd::TakenOver)
        {
            return;
        }
        self.ended.retain(|(ended, _, _)| *ended != key);
        self.held = Some(key);
    }

    /// The host ended, and every hold with it.
    fn host_ended(&mut self) {
        self.host += 1;
        self.held = None;
    }
}

type Shared<T> = Arc<std::sync::Mutex<T>>;

fn lock<T>(shared: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Dropping a pending request must end its host before another request can acquire the link.
struct InFlight<'a> {
    running: &'a mut Option<Running>,
    holds: &'a std::sync::Mutex<Holds>,
    finished: bool,
}
impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if !self.finished {
            end(self.running, self.holds);
        }
    }
}

pub struct HostLink {
    progress: std::sync::Mutex<BTreeMap<String, (Option<aura_domain_common::ToolCallId>, u32)>>,
    holds: Shared<Holds>,
    launcher: std::sync::Mutex<Arc<dyn HostLauncher>>,
    running: Mutex<Option<Running>>,
    ready_within: Duration,
    reply_margin: Duration,
}

impl HostLink {
    #[must_use]
    pub fn new(launcher: Arc<dyn HostLauncher>) -> Self {
        Self::with_times(launcher, READY_WITHIN, REPLY_MARGIN)
    }

    /// A link with its own waits, for tests that should not wait seconds.
    #[must_use]
    pub fn with_times(
        launcher: Arc<dyn HostLauncher>,
        ready_within: Duration,
        reply_margin: Duration,
    ) -> Self {
        Self {
            progress: std::sync::Mutex::default(),
            holds: Shared::default(),
            launcher: std::sync::Mutex::new(launcher),
            running: Mutex::new(None),
            ready_within,
            reply_margin,
        }
    }

    /// Sends one request and waits for its reply, at most `limit` and the reply margin.
    pub async fn request(
        &self,
        command: HostCommand,
        limit: Duration,
    ) -> Result<HostOutcome, LinkError> {
        self.request_for_call(command, limit, None).await
    }

    pub async fn request_for_call(
        &self,
        command: HostCommand,
        limit: Duration,
        call: Option<aura_domain_common::ToolCallId>,
    ) -> Result<HostOutcome, LinkError> {
        let mut running = self.running.lock().await;
        lock(&self.progress).clear();
        if running.is_none() {
            *running = Some(self.start().await?);
        }
        let hold = match &command {
            HostCommand::Guarded { hold: Some(hold), .. } => Some((hold.clone(), ToHold::Act)),
            HostCommand::Release { hold } => Some((hold.clone(), ToHold::Release)),
            HostCommand::Keep { hold } => Some((hold.clone(), ToHold::Keep)),
            _ => None,
        };
        let mut flight = InFlight { running: &mut running, holds: &self.holds, finished: false };
        let host = flight.running.as_mut().ok_or(LinkError::Ended)?;
        host.next_id += 1;
        let id = host.next_id;
        let mut line = serde_json::to_vec(&HostRequest { id, command })
            .map_err(|error| LinkError::Garbled(error.to_string()))?;
        line.push(b'\n');
        let answer = match host.process.send(line) {
            Ok(()) => {
                reply(
                    &mut host.events,
                    id,
                    Instant::now() + limit + self.reply_margin,
                    &self.progress,
                    call,
                )
                .await
            }
            Err(_) => Err(LinkError::Ended),
        };
        flight.finished = answer.is_ok();
        if let (Some((key, kind)), Ok(outcome)) = (hold, &answer) {
            let mut holds = lock(&self.holds);
            match (kind, outcome) {
                (ToHold::Release, _)
                | (ToHold::Keep, HostOutcome::Done(HostResult::Kept { kept: false })) => {
                    if holds.held.as_deref() == Some(key.as_str()) {
                        holds.held = None;
                    }
                }
                (ToHold::Keep, _) => {}
                (ToHold::Act, HostOutcome::Failed(error)) if error.code == HostErrorCode::Busy => {}
                (ToHold::Act, HostOutcome::Failed(error))
                    if error.code == HostErrorCode::TakenOver =>
                {
                    holds.ended(&key, HoldEnd::TakenOver, None);
                }
                (ToHold::Act, _) => holds.answered(key),
            }
        }
        answer
    }

    /// The turn of hold `key` goes on: the host keeps its desktop longer, when it keeps it.
    pub async fn keep(&self, key: &str) {
        if self.held().as_deref() == Some(key) {
            let _ = self.request(HostCommand::Keep { hold: key.to_owned() }, RELEASE_LIMIT).await;
        }
    }

    /// The hold the host keeps now, as far as Core heard.
    #[must_use]
    pub fn held(&self) -> Option<String> {
        lock(&self.holds).held.clone()
    }

    /// How the host ended hold `key`, when it said so.
    #[must_use]
    pub fn hold_end(&self, key: &str) -> Option<HoldEnd> {
        lock(&self.holds)
            .ended
            .iter()
            .rev()
            .find(|(ended, _, _)| ended == key)
            .map(|(_, reason, _)| *reason)
    }

    /// What of the user's took hold `key` over, when the host said.
    #[must_use]
    pub fn takeover_cause(&self, key: &str) -> Option<String> {
        lock(&self.holds)
            .ended
            .iter()
            .rev()
            .find(|(ended, reason, _)| ended == key && *reason == HoldEnd::TakenOver)
            .and_then(|(_, _, cause)| cause.clone())
    }

    /// Gives hold `key` back, when the host keeps it.
    pub async fn release(&self, key: &str) {
        if self.held().as_deref() == Some(key) {
            let _ =
                self.request(HostCommand::Release { hold: key.to_owned() }, RELEASE_LIMIT).await;
        }
    }

    #[must_use]
    pub fn steps_done(&self, app_id: &str) -> u32 {
        lock(&self.progress).get(app_id).map(|(_, count)| *count).unwrap_or(0)
    }
    #[must_use]
    pub fn steps_done_for(&self, call: aura_domain_common::ToolCallId, app_id: &str) -> u32 {
        lock(&self.progress)
            .get(app_id)
            .filter(|(owner, _)| *owner == Some(call))
            .map(|(_, count)| *count)
            .unwrap_or(0)
    }

    /// Ends the host, if one runs; the next request starts another.
    pub async fn stop(&self) {
        end(&mut *self.running.lock().await, &self.holds);
    }

    /// Ends the host and starts this one from the next request on.
    #[cfg(any(test, feature = "test-hooks"))]
    pub async fn use_launcher(&self, launcher: Arc<dyn HostLauncher>) {
        let mut running = self.running.lock().await;
        end(&mut running, &self.holds);
        *lock(&self.launcher) = launcher;
    }

    /// Whether a host runs now.
    pub async fn running(&self) -> bool {
        self.running.lock().await.is_some()
    }

    async fn start(&self) -> Result<Running, LinkError> {
        let (sender, lines) = mpsc::unbounded_channel();
        let launcher = Arc::clone(&lock(&self.launcher));
        let mut process = launcher.start(sender).map_err(LinkError::NotStarted)?;
        let (forward, mut events) = mpsc::unbounded_channel();
        let host = lock(&self.holds).host;
        tokio::spawn(read(lines, forward, Arc::clone(&self.holds), host));
        let ready = match message(&mut events, Instant::now() + self.ready_within).await {
            Ok(HostMessage::Ready { protocol }) if protocol == DESKTOP_PROTOCOL_VERSION => Ok(()),
            Ok(HostMessage::Ready { protocol }) => Err(format!("协议版本 {protocol} 不对")),
            Ok(_) => Err("第一行不是“就绪”".into()),
            Err(LinkError::TimedOut) => Err("没有及时说就绪".into()),
            Err(error) => Err(format!("{error:?}")),
        };
        match ready {
            Ok(()) => Ok(Running { process, events, next_id: 0 }),
            Err(why) => {
                process.stop();
                lock(&self.holds).host_ended();
                Err(LinkError::NotStarted(why))
            }
        }
    }
}

fn end(running: &mut Option<Running>, holds: &std::sync::Mutex<Holds>) {
    if let Some(mut host) = running.take() {
        host.process.stop();
        lock(holds).host_ended();
    }
}

/// Reads host `host`'s lines as they come: what it says of its holds is taken in at once, and
/// everything else waits for the request that reads it.
async fn read(
    mut lines: mpsc::UnboundedReceiver<HostEvent>,
    forward: mpsc::UnboundedSender<Message>,
    holds: Shared<Holds>,
    host: u64,
) {
    while let Some(event) = lines.recv().await {
        let message = match event {
            HostEvent::Ended => {
                let _ = forward.send(Err(LinkError::Ended));
                break;
            }
            HostEvent::TooLong => Err(LinkError::Garbled("一行太长".into())),
            HostEvent::Line(line) => {
                serde_json::from_slice(&line).map_err(|error| LinkError::Garbled(error.to_string()))
            }
        };
        if let Ok(HostMessage::HoldEnded { hold, reason, cause }) = &message {
            let mut holds = lock(&holds);
            if holds.host == host {
                holds.ended(hold, *reason, cause.clone());
            }
            continue;
        }
        if forward.send(message).is_err() {
            return;
        }
    }
    let mut holds = lock(&holds);
    if holds.host == host {
        holds.held = None;
    }
}

async fn message(
    events: &mut mpsc::UnboundedReceiver<Message>,
    until: Instant,
) -> Result<HostMessage, LinkError> {
    match tokio::time::timeout_at(until, events.recv()).await {
        Err(_) => Err(LinkError::TimedOut),
        Ok(None) => Err(LinkError::Ended),
        Ok(Some(message)) => message,
    }
}

/// The reply to request `id`. The host answers in order, so an earlier reply is skipped only
/// after a request that failed here, and the host ended then anyway.
async fn reply(
    events: &mut mpsc::UnboundedReceiver<Message>,
    id: u64,
    until: Instant,
    progress: &std::sync::Mutex<BTreeMap<String, (Option<aura_domain_common::ToolCallId>, u32)>>,
    call: Option<aura_domain_common::ToolCallId>,
) -> Result<HostOutcome, LinkError> {
    loop {
        match message(events, until).await? {
            HostMessage::Reply { id: replied, outcome } if replied == id => return Ok(outcome),
            HostMessage::Reply { .. } | HostMessage::HoldEnded { .. } => {}
            HostMessage::Control { id: request, app_id, steps_done } if request == id => {
                lock(progress).insert(app_id, (call, steps_done));
            }
            HostMessage::Control { .. } => {}
            HostMessage::Ready { .. } => {
                return Err(LinkError::Garbled("宿主又说了一次“就绪”".into()));
            }
        }
    }
}
