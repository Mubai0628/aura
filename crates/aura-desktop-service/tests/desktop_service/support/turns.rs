//! A desktop control over the stand-in host, and the turns' requests the tests send through it.
use super::{Answer, Launcher, Stand};
use aura_config::{AuraPaths, SettingsFacade};
use aura_desktop_domain::*;
use aura_desktop_service::{Caller, Controlled, DesktopControl, HostLink};
use aura_domain_common::{SessionId, TurnTaskId};
use std::sync::Arc;
use std::time::Duration;

pub const NOTEPAD: &str = "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App";
pub const LIMIT: Duration = Duration::from_millis(300);

pub fn notepad() -> AppEntry {
    AppEntry { id: NOTEPAD.into(), name: "记事本".into(), kind: AppKind::Packaged, path: None }
}

pub struct Setup {
    pub _temp: tempfile::TempDir,
    pub settings: Arc<SettingsFacade>,
    pub stand: Arc<Stand>,
    pub control: Arc<DesktopControl>,
}

pub fn setup(stand: Arc<Stand>) -> Setup {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let settings = SettingsFacade::open(paths).expect("settings");
    let link = HostLink::with_times(
        Arc::new(Launcher(Arc::clone(&stand))),
        Duration::from_millis(300),
        Duration::from_millis(100),
    );
    let control = Arc::new(DesktopControl::with_link(Arc::clone(&settings), link));
    Setup { _temp: temp, settings, stand, control }
}

pub fn quiet() -> Setup {
    setup(Stand::new(|_| Answer::Silent))
}

pub fn typing() -> HostCommand {
    HostCommand::Input {
        app: notepad(),
        steps: vec![InputStep::Type { text: "x".into() }],
        window: None,
        clipboard: false,
        system_keys: false,
    }
}

/// How the stand-in host answers when a test does not say otherwise.
pub fn ordinary(command: &HostCommand) -> Answer {
    Answer::Now(HostOutcome::Done(match command {
        HostCommand::Guarded { command, .. } => return ordinary(command),
        HostCommand::Release { .. } => HostResult::Released,
        HostCommand::Input { steps, .. } => HostResult::Input { steps_done: steps.len() as u32 },
        _ => HostResult::Displays { monitors: vec![] },
    }))
}

pub fn caller(session: SessionId, turn: TurnTaskId) -> Caller {
    Caller { call: aura_domain_common::ToolCallId::new(), session, turn: Some(turn) }
}

/// The turn's typing in hold `hold`.
pub fn kept_as(hold: &str) -> HostCommand {
    HostCommand::Guarded {
        command: Box::new(typing()),
        hold: Some(hold.to_owned()),
        stop_keys: Some(aura_desktop_domain::pip::PIP_STOP_KEYS.into()),
    }
}

/// The turn's typing in its first hold.
pub fn kept(turn: TurnTaskId) -> HostCommand {
    kept_as(&turn.to_string())
}

pub fn notepad_controlled(taken_over: bool) -> Controlled {
    Controlled {
        app_id: NOTEPAD.into(),
        name: "记事本".into(),
        foreground: true,
        taken_over,
        mode: aura_desktop_domain::ComputerMode::Foreground,
    }
}

/// The host says the user took hold `hold` over.
pub fn taken_over(hold: &str) -> HostMessage {
    HostMessage::HoldEnded {
        hold: hold.to_owned(),
        reason: HoldEnd::TakenOver,
        cause: Some("点了鼠标".into()),
    }
}

/// Waits up to a second for `done`.
pub async fn until(done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "not in time");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
