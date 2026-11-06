//! The child desktop's lifetime and input lease. Its window survives individual turns; its
//! host and references are separate from the foreground host, with no fallback between them.
mod native;
mod worker;
use crate::{Caller, HostEvent, HostProcess, child::ChildHost};
use aura_desktop_domain::pip::PipCommand;
use aura_desktop_domain::{ComputerPipStatus, Delivery, HostError, HostErrorCode, PipPhase};
use aura_domain_common::{SessionId, TurnTaskId};
pub use native::NativePip;
use std::{
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

pub const LOGIN_WITHIN: Duration = Duration::from_secs(180);
pub const HOLD_WITHIN: Duration = Duration::from_secs(5);
pub type Login = Pin<Box<dyn Future<Output = Result<u32, String>> + Send>>;
/// Cleanup is called after admission as well as on every failed/cancelled opening.
pub trait StartupEntry: Send {
    fn remove(&mut self) -> Result<(), String>;
}
pub struct Opened {
    pub window: Box<dyn HostProcess>,
    pub events: mpsc::UnboundedReceiver<HostEvent>,
    pub login: Login,
    pub startup: Box<dyn StartupEntry>,
}
impl Drop for Opened {
    fn drop(&mut self) {
        let _ = self.startup.remove();
        self.window.stop();
    }
}
pub trait PipBackend: Send + Sync {
    fn open(&self, child: ChildHost, stop_keys: &str) -> Result<Opened, String>;
}
type Owner = (SessionId, TurnTaskId);
#[derive(Clone, Default)]
pub(super) struct State {
    pub generation: u64,
    pub status: ComputerPipStatus,
    pub owner: Option<Owner>,
    pub blocked: BTreeSet<Owner>,
    pub held: bool,
    pub acknowledgement: u64,
    pub request: u64,
    pub commands: Option<mpsc::UnboundedSender<PipCommand>>,
}
pub(super) fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
pub(super) fn failure(message: impl Into<String>) -> HostError {
    HostError::new(HostErrorCode::Internal, message, Delivery::NotDelivered)
}

pub struct PipSession {
    backend: Arc<dyn PipBackend>,
    pub(super) child: ChildHost,
    pub(super) state: Arc<Mutex<State>>,
    pub(super) changed: watch::Sender<u64>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    opening: tokio::sync::Mutex<()>,
    control: tokio::sync::Mutex<()>,
    waiters: AtomicUsize,
    login_within: Duration,
    hold_within: Duration,
}
impl PipSession {
    pub fn new(backend: Arc<dyn PipBackend>, child: ChildHost) -> Arc<Self> {
        Self::with_times(backend, child, LOGIN_WITHIN, HOLD_WITHIN)
    }
    pub fn with_times(
        backend: Arc<dyn PipBackend>,
        child: ChildHost,
        login_within: Duration,
        hold_within: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            backend,
            child,
            state: Arc::default(),
            changed: watch::channel(0).0,
            task: Mutex::default(),
            opening: tokio::sync::Mutex::new(()),
            control: tokio::sync::Mutex::new(()),
            waiters: AtomicUsize::new(0),
            login_within,
            hold_within,
        })
    }
    pub fn status(&self) -> ComputerPipStatus {
        lock(&self.state).status.clone()
    }
    pub fn generation(&self) -> u64 {
        lock(&self.state).generation
    }
    pub fn stop_keys(&self, keys: &str) {
        if let Some(commands) = &lock(&self.state).commands {
            let _ = commands.send(PipCommand::StopKeys { keys: keys.to_owned() });
        }
    }
    pub fn taken_over(&self, session: SessionId, turn: TurnTaskId) -> bool {
        lock(&self.state).blocked.contains(&(session, turn))
    }
    pub fn owns(&self, session: SessionId, turn: TurnTaskId) -> bool {
        let state = lock(&self.state);
        state.owner == Some((session, turn)) && state.held
    }
    pub async fn ensure(&self, stop_keys: &str) -> Result<(), HostError> {
        self.waiters.fetch_add(1, Ordering::SeqCst);
        let _waiting = LoginWait(self);
        let mut changed = self.changed.subscribe();
        {
            let _opening = self.opening.lock().await;
            let phase = self.status().phase;
            if phase == PipPhase::Ready {
                return Ok(());
            }
            if phase != PipPhase::WaitingForLogin {
                if let Some(task) = lock(&self.task).take() {
                    task.abort();
                }
                self.child.close();
                let opened = self.backend.open(self.child.clone(), stop_keys).map_err(|why| {
                    lock(&self.state).status =
                        ComputerPipStatus { phase: PipPhase::Failed, notice: Some(why.clone()) };
                    self.changed.send_modify(|revision| *revision += 1);
                    failure(why)
                })?;
                let (commands, pending) = mpsc::unbounded_channel();
                let generation = {
                    let mut state = lock(&self.state);
                    state.generation += 1;
                    state.owner = None;
                    state.held = false;
                    state.commands = Some(commands);
                    state.status =
                        ComputerPipStatus { phase: PipPhase::WaitingForLogin, notice: None };
                    state.generation
                };
                let task = worker::spawn(
                    opened,
                    pending,
                    Arc::clone(&self.state),
                    self.changed.clone(),
                    self.child.clone(),
                    generation,
                    self.login_within,
                );
                *lock(&self.task) = Some(task);
            }
        }
        tokio::time::timeout(self.login_within + Duration::from_secs(1), async {
            loop {
                let status = self.status();
                match status.phase {
                    PipPhase::Ready => return Ok(()),
                    PipPhase::WaitingForLogin => {}
                    _ => {
                        return Err(failure(status.notice.unwrap_or_else(|| {
                            "小窗没有登录，请重新打开并登录；没有执行".into()
                        })));
                    }
                }
                changed.changed().await.map_err(|_| failure("小窗已关闭；没有执行"))?;
            }
        })
        .await
        .map_err(|_| failure("小窗没有登录，等待已超时；没有执行"))?
    }
    pub async fn hold(&self, caller: Caller) -> Result<(), HostError> {
        let turn = caller.turn.ok_or_else(|| failure("小窗操作缺少回合身份；没有执行"))?;
        let owner = (caller.session, turn);
        let _control = self.control.lock().await;
        let mut changed = self.changed.subscribe();
        let (generation, ticket) = {
            let mut state = lock(&self.state);
            if state.blocked.contains(&owner) {
                return Err(HostError::new(
                    HostErrorCode::TakenOver,
                    "用户已接管小窗，用户交还操作权之前不能再操作；没有执行",
                    Delivery::NotDelivered,
                ));
            }
            if state.status.phase != PipPhase::Ready {
                return Err(failure("小窗没有登录；没有执行"));
            }
            if state.owner.is_some_and(|existing| existing != owner) {
                return Err(HostError::new(
                    HostErrorCode::Busy,
                    "另一段对话正在操控小窗；没有执行",
                    Delivery::NotDelivered,
                ));
            }
            if state.owner == Some(owner) && state.held {
                return Ok(());
            }
            state.owner = Some(owner);
            state.request += 1;
            state
                .commands
                .as_ref()
                .ok_or_else(|| failure("小窗已关闭；没有执行"))?
                .send(PipCommand::Hold { on: true, request: state.request })
                .map_err(|_| failure("小窗已关闭；没有执行"))?;
            (state.generation, state.request)
        };
        let result = tokio::time::timeout(self.hold_within, async {
            loop {
                {
                    let state = lock(&self.state);
                    if state.blocked.contains(&owner) {
                        return Err(HostError::new(
                            HostErrorCode::TakenOver,
                            "用户已接管小窗；没有执行",
                            Delivery::NotDelivered,
                        ));
                    }
                    if state.generation != generation || state.status.phase != PipPhase::Ready {
                        return Err(failure("小窗已断开；没有执行"));
                    }
                    if state.acknowledgement == ticket && state.held {
                        return Ok(());
                    }
                }
                changed.changed().await.map_err(|_| failure("小窗已关闭；没有执行"))?;
            }
        })
        .await
        .unwrap_or_else(|_| Err(failure("小窗输入保护未及时生效；没有执行")));
        if result.is_err() {
            self.release(caller.session, Some(turn)).await;
        }
        result
    }
    pub async fn release(&self, session: SessionId, turn: Option<TurnTaskId>) {
        let pending = {
            let mut state = lock(&self.state);
            if !state
                .owner
                .is_some_and(|owner| owner.0 == session && turn.is_none_or(|turn| owner.1 == turn))
            {
                return;
            }
            state.owner = None;
            state.held = false;
            state.request += 1;
            state.commands.clone().map(|commands| (commands, state.request))
        };
        if let Some((commands, request)) = pending {
            let _ = commands.send(PipCommand::Hold { on: false, request });
        }
    }
    pub fn finished(&self, session: SessionId, turn: TurnTaskId) {
        lock(&self.state).blocked.remove(&(session, turn));
    }
    pub fn stop(&self) {
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        self.child.close();
        let mut state = lock(&self.state);
        state.generation += 1;
        state.status = ComputerPipStatus::default();
        state.commands = None;
        state.held = false;
        state.owner = None;
        self.changed.send_modify(|revision| *revision += 1);
    }
}
impl Drop for PipSession {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Cancellation of the last caller must not leave a login window or startup entry behind.
struct LoginWait<'a>(&'a PipSession);
impl Drop for LoginWait<'_> {
    fn drop(&mut self) {
        if self.0.waiters.fetch_sub(1, Ordering::SeqCst) == 1
            && self.0.status().phase == PipPhase::WaitingForLogin
        {
            self.0.stop();
        }
    }
}
