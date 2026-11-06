//! Core's side of computer control: the settings, the one host, which programs each session is
//! controlling now, and stopping them. A tool reaches the desktop only through
//! [`DesktopControl::run`] and [`DesktopControl::run_for`]. A turn keeps the desktop from its
//! first action to its end: the host holds it between the turn's calls too, until the user
//! takes it over, Core gives it back, or a minute passes without an action.
use crate::launch::HostLauncher;
use crate::link::HostLink;
use aura_config::{ComputerApp, ComputerControlSettingsSnapshot, SettingsFacade};
use aura_desktop_domain::{
    AppEntry, AppIcon, Delivery, HoldEnd, HostCommand, HostError, HostErrorCode, HostOutcome,
    HostResult, ManualApp,
};
use aura_domain_common::{SessionId, ToolCallId, TurnTaskId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::watch;

#[path = "catalog.rs"]
mod catalog;
#[path = "routing.rs"]
mod routing;
#[path = "takeover.rs"]
mod takeover;

/// How long the catalog of programs is reused before the host reads it again.
pub const CATALOG_FOR: Duration = Duration::from_secs(30);
/// The time limit of listing the programs.
pub const CATALOG_LIMIT: Duration = Duration::from_secs(20);
/// The icons' size in pixels: Aura's pages draw them at 16 CSS pixels, twice that on a
/// high-density screen.
pub const ICON_SIZE: u32 = 32;
/// The most icons one request to the host reads, so that each answers well within its limit.
pub const ICONS_PER_READ: usize = 40;
/// The time limit of one read of icons.
pub const ICONS_LIMIT: Duration = Duration::from_secs(20);

/// What the model reads when the user took the desktop over from its turn.
/// The refusal of a turn's action after the user took the desktop over, naming what they did
/// when the host said.
#[must_use]
pub fn taken_over_refusal(cause: Option<&str>) -> String {
    format!("{}；没有执行", aura_desktop_domain::taken_over_message(cause))
}
const BUSY: &str = "另一段对话正在操控电脑，等它结束再操作；没有执行";

/// A program a session is controlling now, as the activity list shows it.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Controlled {
    pub app_id: String,
    pub name: String,
    /// One of the calls has the foreground: it moves the pointer or types.
    pub foreground: bool,
    /// The user took the desktop over from the turn.
    pub taken_over: bool,
    pub mode: aura_desktop_domain::ComputerMode,
}

/// Whose request it is: a tool call of a session, in a turn when it has one. A turn's actions
/// keep the desktop in the turn's hold.
#[derive(Clone, Copy, Debug)]
pub struct Caller {
    pub call: ToolCallId,
    pub session: SessionId,
    pub turn: Option<TurnTaskId>,
}

#[derive(Default)]
struct Calls {
    name: String,
    running: usize,
    foreground: usize,
    turns: BTreeSet<TurnTaskId>,
}

/// A turn that acted on the desktop, and the program of its latest action.
struct Hold {
    session: SessionId,
    turn: TurnTaskId,
    app_id: String,
    name: String,
    foreground: bool,
}

#[derive(Default)]
struct State {
    stopped_turns: BTreeSet<(SessionId, TurnTaskId, String)>,
    controlling: BTreeMap<(SessionId, String), Calls>,
    /// The turns that acted, by their hold, until each ends.
    holds: BTreeMap<String, Hold>,
    /// Turns whose actions failed because the user took the desktop over.
    taken_over: BTreeSet<(SessionId, TurnTaskId)>,
    /// How many times the user gave each running turn the desktop back: each time, a new hold.
    generations: BTreeMap<TurnTaskId, u32>,
    /// Each session's turn that ended taken over, until given back or the next turn runs.
    ended: BTreeMap<SessionId, takeover::Ended>,
    /// The latest turn of each session that runs.
    latest: BTreeMap<SessionId, TurnTaskId>,
    /// Always-allowed programs a session stopped: they ask again in that session.
    stopped: BTreeMap<SessionId, BTreeSet<String>>,
    /// Raised each time a program of a session is stopped; a call watching it ends.
    stops: BTreeMap<(SessionId, String), watch::Sender<u64>>,
    catalog: Option<(Instant, Vec<AppEntry>)>,
    catalog_issue: Option<String>,
    /// Each program's icon once read, a PNG in base64; `None` when Windows has none.
    icons: BTreeMap<String, Option<String>>,
}

pub struct DesktopControl {
    settings: Arc<SettingsFacade>,
    link: HostLink,
    state: Mutex<State>,
    memory: Mutex<crate::calls::Memory>,
    /// One read of icons at a time: a second request finds what the first read.
    icon_reads: tokio::sync::Mutex<()>,
    child: std::sync::OnceLock<Arc<DesktopControl>>,
    mode: aura_desktop_domain::ComputerMode,
    pip: Option<Arc<crate::pip::PipSession>>,
    pip_generation: Mutex<u64>,
}

/// A call on a program, from its start to its end; dropping it ends it.
pub struct Controlling {
    control: Arc<DesktopControl>,
    key: (SessionId, String),
    foreground: bool,
    /// Changes when the user stops the program in this session.
    pub stopped: watch::Receiver<u64>,
}

impl Drop for Controlling {
    fn drop(&mut self) {
        let mut state = self.control.lock();
        if let Some(calls) = state.controlling.get_mut(&self.key) {
            calls.running = calls.running.saturating_sub(1);
            if self.foreground {
                calls.foreground = calls.foreground.saturating_sub(1);
            }
            if calls.running == 0 {
                state.controlling.remove(&self.key);
            }
        }
    }
}

impl DesktopControl {
    #[must_use]
    pub fn new(settings: Arc<SettingsFacade>, launcher: Arc<dyn HostLauncher>) -> Self {
        Self::with_link(settings, HostLink::new(launcher))
    }

    #[must_use]
    pub fn with_link(settings: Arc<SettingsFacade>, link: HostLink) -> Self {
        Self {
            settings,
            link,
            state: Mutex::default(),
            memory: Mutex::default(),
            icon_reads: tokio::sync::Mutex::default(),
            child: std::sync::OnceLock::new(),
            mode: aura_desktop_domain::ComputerMode::Foreground,
            pip: None,
            pip_generation: Mutex::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn memory(&self) -> MutexGuard<'_, crate::calls::Memory> {
        self.memory.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The catalog as the last listing read it, however old; `None` before the first.
    #[must_use]
    pub fn cached_catalog(&self) -> Option<Vec<AppEntry>> {
        self.lock().catalog.as_ref().map(|(_, apps)| apps.clone())
    }

    #[must_use]
    pub fn settings(&self) -> ComputerControlSettingsSnapshot {
        self.settings.computer_settings()
    }

    /// Where the settings are saved.
    #[must_use]
    pub fn settings_facade(&self) -> &SettingsFacade {
        &self.settings
    }

    /// Carries out one request on the desktop; an action keeps it for that request.
    pub async fn run(
        &self,
        command: HostCommand,
        limit: Duration,
    ) -> Result<HostResult, HostError> {
        self.request(command, limit, None).await
    }

    /// Carries out one request of a tool call; an action of a turn keeps the desktop in the
    /// turn's hold. A turn the user took the desktop over from may only read, and while another
    /// turn keeps it, actions wait.
    pub async fn run_for(
        &self,
        command: HostCommand,
        limit: Duration,
        caller: Caller,
    ) -> Result<HostResult, HostError> {
        self.request(command, limit, Some(caller)).await
    }

    async fn request(
        &self,
        command: HostCommand,
        limit: Duration,
        caller: Option<Caller>,
    ) -> Result<HostResult, HostError> {
        self.ensure_target().await?;
        if let Some(pip) = &self.pip
            && command.acts_on().is_some()
        {
            pip.hold(caller.ok_or_else(|| crate::pip::failure("小窗操作缺少回合身份；没有执行"))?)
                .await?;
        }
        let call = caller.map(|caller| caller.call);
        let Some(app) = command.acts_on().cloned() else {
            // The turn that keeps the desktop goes on reading it: the host keeps it meanwhile.
            if let Some(turn) = caller.and_then(|caller| caller.turn) {
                self.link.keep(&self.key(turn)).await;
            }
            return self.send(command, limit, call).await;
        };
        let turn = caller.and_then(|caller| Some((caller.session, caller.turn?)));
        let key = turn.map(|(_, turn)| self.key(turn));
        if let Some((session, turn)) = turn
            && self.taken_over(session, turn)
        {
            return Err(HostError::new(
                HostErrorCode::TakenOver,
                taken_over_refusal(self.takeover_cause(turn).as_deref()),
                Delivery::NotDelivered,
            ));
        }
        if self.link.held().is_some_and(|held| key.as_ref() != Some(&held)) {
            return Err(HostError::new(HostErrorCode::Busy, BUSY, Delivery::NotDelivered));
        }
        if let (Some((session, turn)), Some(key)) = (turn, &key) {
            self.lock().holds.insert(
                key.clone(),
                Hold {
                    session,
                    turn,
                    app_id: app.id.clone(),
                    name: app.name.clone(),
                    foreground: command.takes_foreground(),
                },
            );
        }
        let held = HostCommand::Guarded {
            command: Box::new(command),
            hold: key,
            stop_keys: Some(self.settings().settings.pip_stop_keys),
        };
        let mut result = self.send(held, limit, call).await;
        if let (Err(error), Some((session, turn))) = (&mut result, turn)
            && self.pip.as_ref().is_some_and(|pip| pip.taken_over(session, turn))
        {
            error.code = HostErrorCode::TakenOver;
            error.message = if error.delivery == Delivery::Uncertain {
                "用户已接管小窗，这一步可能执行了部分；请先查看最新画面，不要重放。".into()
            } else {
                "用户已接管小窗；没有执行".into()
            };
        }
        if let (Err(error), Some(turn)) = (&result, turn)
            && error.code == HostErrorCode::TakenOver
        {
            self.lock().taken_over.insert(turn);
        }
        result
    }

    async fn send(
        &self,
        command: HostCommand,
        limit: Duration,
        call: Option<ToolCallId>,
    ) -> Result<HostResult, HostError> {
        match self.link.request_for_call(command, limit, call).await {
            Ok(HostOutcome::Done(result)) => Ok(result),
            Ok(HostOutcome::Failed(error)) => Err(error),
            Err(error) => Err(error.into_host_error()),
        }
    }

    /// Gives back the desktop the session keeps, after the user stopped it.
    async fn release_one(&self, session: SessionId) {
        let keys: Vec<String> = {
            let mut state = self.lock();
            state.ended.remove(&session);
            state
                .holds
                .iter()
                .filter(|(_, hold)| hold.session == session)
                .map(|(key, _)| key.clone())
                .collect()
        };
        for key in keys {
            self.link.release(&key).await;
        }
        if let Some(pip) = &self.pip {
            pip.release(session, None).await;
        }
    }

    /// Whether the program is on the always-allowed list and the session has not stopped it.
    #[must_use]
    pub fn always_allowed(&self, session: SessionId, app_id: &str) -> bool {
        let listed = self.settings().settings.always_allowed.iter().any(|app| app.app_id == app_id);
        listed && !self.lock().stopped.get(&session).is_some_and(|stopped| stopped.contains(app_id))
    }

    /// A call of the session on the program begins.
    pub fn begin(
        self: &Arc<Self>,
        session: SessionId,
        app: &AppEntry,
        foreground: bool,
    ) -> Controlling {
        self.begin_turn(session, app, foreground, None)
    }

    pub fn begin_turn(
        self: &Arc<Self>,
        session: SessionId,
        app: &AppEntry,
        foreground: bool,
        turn: Option<TurnTaskId>,
    ) -> Controlling {
        let key = (session, app.id.clone());
        let mut state = self.lock();
        let calls = state.controlling.entry(key.clone()).or_default();
        calls.name.clone_from(&app.name);
        calls.running += 1;
        calls.foreground += usize::from(foreground);
        calls.turns.extend(turn);
        let stopped =
            state.stops.entry(key.clone()).or_insert_with(|| watch::channel(0).0).subscribe();
        Controlling { control: Arc::clone(self), key, foreground, stopped }
    }

    /// The programs the session is controlling now: those its calls run on, and the program
    /// of its turn's latest action while the desktop is kept for the turn, or after the user
    /// took it over until the user gives it back or the session's next turn runs.
    #[must_use]
    pub fn controlling(&self, session: SessionId) -> Vec<Controlled> {
        let held = self.link.held();
        let state = self.lock();
        let mut shown: Vec<Controlled> = state
            .controlling
            .iter()
            .filter(|((owner, _), _)| *owner == session)
            .map(|((_, app_id), calls)| Controlled {
                app_id: app_id.clone(),
                name: calls.name.clone(),
                foreground: calls.foreground > 0,
                taken_over: false,
                mode: self.mode,
            })
            .collect();
        for (key, hold) in state.holds.iter().filter(|(_, hold)| hold.session == session) {
            let taken_over = state.taken_over.contains(&(session, hold.turn))
                || self.link.hold_end(key) == Some(HoldEnd::TakenOver)
                || self.pip.as_ref().is_some_and(|pip| pip.taken_over(session, hold.turn));
            if !taken_over
                && held.as_deref() != Some(key.as_str())
                && !self.pip.as_ref().is_some_and(|pip| pip.owns(session, hold.turn))
            {
                continue;
            }
            let row = Controlled {
                app_id: hold.app_id.clone(),
                name: hold.name.clone(),
                foreground: hold.foreground,
                taken_over,
                mode: self.mode,
            };
            takeover::list(&mut shown, row);
        }
        if let Some(row) = takeover::ended_row(&state, session, self.mode) {
            takeover::list(&mut shown, row);
        }
        if let Some(child) = self.child.get() {
            shown.extend(child.controlling(session));
        }
        shown
    }

    /// The user stopped the program in this session: its calls end, and an always-allowed
    /// program asks again in this session. The session's own answers are withdrawn by the
    /// caller, in the approval records.
    pub fn stop(&self, session: SessionId, app_id: &str) {
        if let Some(child) = self.child.get() {
            child.stop(session, app_id);
        }
        let mut state = self.lock();
        let turns = state
            .controlling
            .get(&(session, app_id.to_owned()))
            .map(|calls| calls.turns.clone())
            .unwrap_or_default();
        for turn in turns {
            state.stopped_turns.insert((session, turn, app_id.to_owned()));
        }
        state.stopped.entry(session).or_default().insert(app_id.to_owned());
        if let Some(stops) = state.stops.get(&(session, app_id.to_owned())) {
            stops.send_modify(|count| *count += 1);
        }
    }

    #[must_use]
    pub fn turn_stopped(&self, session: SessionId, turn: TurnTaskId, app_id: &str) -> bool {
        self.lock().stopped_turns.contains(&(session, turn, app_id.to_owned()))
    }

    /// Adds the program to the always-allowed list of the settings.
    pub fn always_allow(&self, app: ComputerApp) -> Result<(), String> {
        for _ in 0..3 {
            let snapshot = self.settings();
            let Some(revision) = snapshot.revision else {
                return Err(snapshot.issue.unwrap_or_else(|| "电脑操控设置不可用".into()));
            };
            let mut settings = snapshot.settings;
            if settings.always_allowed.iter().any(|known| known.app_id == app.app_id) {
                return Ok(());
            }
            settings.always_allowed.push(app.clone());
            match self.settings.update_computer_settings(revision, &settings) {
                Ok(_) => return Ok(()),
                Err(aura_config::SettingsFacadeError::RevisionConflict { .. }) => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("电脑操控设置一直在变，没能写进“总是允许”".into())
    }

    /// Ends the host; the next request starts it again.
    async fn stop_host_one(&self) {
        self.link.stop().await;
        if let Some(pip) = &self.pip {
            pip.stop();
        }
    }

    #[must_use]
    pub fn steps_done_for(&self, call: ToolCallId, app_id: &str) -> u32 {
        self.link.steps_done_for(call, app_id)
    }

    /// Starts another host program from now on: a test plays the host.
    #[cfg(any(test, feature = "test-hooks"))]
    pub async fn use_launcher(&self, launcher: Arc<dyn HostLauncher>) {
        self.link.use_launcher(launcher).await;
        self.refresh_catalog();
    }
}

fn unexpected(doing: &str, result: &HostResult) -> HostError {
    HostError::new(
        aura_desktop_domain::HostErrorCode::Internal,
        format!("{doing}时收到了别的结果：{result:?}"),
        aura_desktop_domain::Delivery::NotDelivered,
    )
}
