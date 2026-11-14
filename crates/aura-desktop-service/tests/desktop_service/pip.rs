//! Scripted window and desktop processes: no login, startup entry or desktop input.
use aura_desktop_domain::{
    ComputerMode, DESKTOP_PROTOCOL_VERSION, Delivery, HostCommand, HostErrorCode, HostMessage,
    HostOutcome, HostRequest, HostResult,
    pip::{PIP_PROTOCOL_VERSION, PipCommand, PipMessage},
};
use aura_desktop_service::{
    Caller, DesktopControl, HostEvent, HostLauncher, HostProcess,
    child::ChildHost,
    pip::{Opened, PipBackend, PipSession, StartupEntry},
};
use aura_domain_common::{SessionId, ToolCallId, TurnTaskId};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Default)]
struct Backend {
    opens: AtomicUsize,
    cleanup: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    events: Mutex<Option<mpsc::UnboundedSender<HostEvent>>>,
    commands: Arc<Mutex<Vec<PipCommand>>>,
    ack: bool,
    login: bool,
    verified: u32,
}
impl Backend {
    fn ready() -> Arc<Self> {
        Arc::new(Self { ack: true, login: true, verified: 3, ..Self::default() })
    }
    fn say(&self, message: PipMessage) {
        self.events
            .lock()
            .expect("events")
            .as_ref()
            .expect("window opened")
            .send(HostEvent::Line(serde_json::to_vec(&message).expect("message")))
            .expect("connected");
    }
}
struct Entry {
    removed: bool,
    count: Arc<AtomicUsize>,
}
impl StartupEntry for Entry {
    fn remove(&mut self) -> Result<(), String> {
        if !self.removed {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.removed = true;
        }
        Ok(())
    }
}
struct Window {
    events: mpsc::UnboundedSender<HostEvent>,
    commands: Arc<Mutex<Vec<PipCommand>>>,
    ack: bool,
    stops: Arc<AtomicUsize>,
}
impl HostProcess for Window {
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()> {
        let command: PipCommand = serde_json::from_slice(&line)?;
        self.commands.lock().expect("commands").push(command.clone());
        if let PipCommand::Hold { on, request } = command {
            self.events.send(HostEvent::Line(serde_json::to_vec(&PipMessage::Holding {
                on,
                request: if self.ack { request } else { 0 },
            })?))?;
        }
        Ok(())
    }
    fn stop(&mut self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
}
impl PipBackend for Backend {
    fn open(&self, _: ChildHost, _: &str) -> Result<Opened, String> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let (events, received) = mpsc::unbounded_channel();
        *self.events.lock().expect("events") = Some(events.clone());
        self.say(PipMessage::Ready { protocol: PIP_PROTOCOL_VERSION });
        if self.login {
            self.say(PipMessage::SignedIn { session: 3 });
        }
        let verified = self.verified;
        let login = self.login;
        Ok(Opened {
            window: Box::new(Window {
                events,
                commands: Arc::clone(&self.commands),
                ack: self.ack,
                stops: Arc::clone(&self.stops),
            }),
            events: received,
            login: Box::pin(async move {
                if login { Ok(verified) } else { std::future::pending().await }
            }),
            startup: Box::new(Entry { removed: false, count: Arc::clone(&self.cleanup) }),
        })
    }
}
fn caller() -> Caller {
    Caller { session: SessionId::new(), turn: Some(TurnTaskId::new()), call: ToolCallId::new() }
}
fn session(backend: Arc<Backend>) -> Arc<PipSession> {
    PipSession::with_times(
        backend,
        ChildHost::new(),
        Duration::from_millis(80),
        Duration::from_millis(80),
    )
}

struct CannotOpen;
impl PipBackend for CannotOpen {
    fn open(&self, _: ChildHost, _: &str) -> Result<Opened, String> {
        Err("临时启动项不可用；没有执行".into())
    }
}

#[tokio::test]
async fn opening_failure_is_visible_and_stop_key_changes_reach_an_existing_window() {
    let unavailable = PipSession::new(Arc::new(CannotOpen), ChildHost::new());
    unavailable.ensure("Ctrl+Alt+Q").await.expect_err("cannot open");
    assert_eq!(unavailable.status().phase, aura_desktop_domain::PipPhase::Failed);
    assert!(unavailable.status().notice.expect("reason").contains("临时启动项"));
    let backend = Backend::ready();
    let pip = session(Arc::clone(&backend));
    pip.ensure("Ctrl+Alt+Q").await.expect("ready");
    pip.stop_keys("Ctrl+Alt+P");
    tokio::time::timeout(Duration::from_secs(1), async {
        while !backend
            .commands
            .lock()
            .expect("commands")
            .contains(&PipCommand::StopKeys { keys: "Ctrl+Alt+P".into() })
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("updated");
    backend.say(PipMessage::StopKeysTaken { keys: "Ctrl+Alt+P".into() });
    tokio::time::timeout(Duration::from_secs(1), async {
        while pip.status().notice.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("warning");
    backend.say(PipMessage::StopKeysReady);
    tokio::time::timeout(Duration::from_secs(1), async {
        while pip.status().notice.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("resolved");
}

#[tokio::test]
async fn simultaneous_calls_share_one_window_and_turns_do_not_close_it() {
    let backend = Backend::ready();
    let pip = session(Arc::clone(&backend));
    let (left, right) = tokio::join!(pip.ensure("Ctrl+Alt+Q"), pip.ensure("Ctrl+Alt+Q"));
    left.expect("ready");
    right.expect("ready");
    assert_eq!(backend.opens.load(Ordering::SeqCst), 1);
    assert_eq!(backend.cleanup.load(Ordering::SeqCst), 1);
    let owner = caller();
    pip.hold(owner).await.expect("held");
    assert!(pip.owns(owner.session, owner.turn.expect("turn")));
    assert_eq!(pip.hold(caller()).await.expect_err("busy").code, HostErrorCode::Busy);
    pip.release(owner.session, owner.turn).await;
    pip.hold(caller()).await.expect("next turn");
    assert_eq!(backend.opens.load(Ordering::SeqCst), 1);
    assert_eq!(backend.stops.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn takeover_latches_for_that_turn_and_a_new_turn_can_acquire() {
    let backend = Backend::ready();
    let pip = session(Arc::clone(&backend));
    pip.ensure("Ctrl+Alt+Q").await.expect("ready");
    let owner = caller();
    pip.hold(owner).await.expect("held");
    backend.say(PipMessage::TakenOver);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !pip.taken_over(owner.session, owner.turn.expect("turn")) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("takeover");
    assert_eq!(pip.hold(owner).await.expect_err("old turn refused").code, HostErrorCode::TakenOver);
    pip.hold(caller()).await.expect("new turn");
}

#[tokio::test]
async fn login_timeout_and_wrong_session_never_make_a_child_desktop_ready_and_clean_up() {
    for backend in [
        Arc::new(Backend::default()),
        Arc::new(Backend { ack: true, login: true, verified: 4, ..Backend::default() }),
    ] {
        let pip = session(Arc::clone(&backend));
        assert_eq!(
            pip.ensure("Ctrl+Alt+Q").await.expect_err("not admitted").delivery,
            Delivery::NotDelivered
        );
        assert_eq!(backend.cleanup.load(Ordering::SeqCst), 1);
        assert!(backend.stops.load(Ordering::SeqCst) > 0);
    }
}

#[tokio::test]
async fn an_old_input_acknowledgement_does_not_authorize_a_new_action() {
    let backend = Arc::new(Backend { ack: false, login: true, verified: 3, ..Backend::default() });
    let pip = session(backend);
    pip.ensure("Ctrl+Alt+Q").await.expect("ready");
    assert_eq!(
        pip.hold(caller()).await.expect_err("stale ack refused").delivery,
        Delivery::NotDelivered
    );
}

#[tokio::test]
async fn cancelling_the_last_login_wait_removes_the_temporary_startup_entry() {
    let backend = Arc::new(Backend::default());
    let pip = PipSession::with_times(
        backend.clone(),
        ChildHost::new(),
        Duration::from_secs(60),
        Duration::from_millis(80),
    );
    let waiting = tokio::spawn({
        let pip = pip.clone();
        async move { pip.ensure("Ctrl+Alt+Q").await }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while backend.opens.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("opened");
    waiting.abort();
    let _ = waiting.await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(backend.cleanup.load(Ordering::SeqCst), 1);
    assert_eq!(pip.status().phase, aura_desktop_domain::PipPhase::Closed);
}

struct Desktop(Arc<AtomicUsize>);
struct DesktopProcess(mpsc::UnboundedSender<HostEvent>);
impl HostLauncher for Desktop {
    fn start(
        &self,
        events: mpsc::UnboundedSender<HostEvent>,
    ) -> Result<Box<dyn HostProcess>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        events
            .send(HostEvent::Line(
                serde_json::to_vec(&HostMessage::Ready { protocol: DESKTOP_PROTOCOL_VERSION })
                    .expect("ready"),
            ))
            .expect("connected");
        Ok(Box::new(DesktopProcess(events)))
    }
}
impl HostProcess for DesktopProcess {
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()> {
        let request: HostRequest = serde_json::from_slice(&line)?;
        self.0.send(HostEvent::Line(serde_json::to_vec(&HostMessage::Reply {
            id: request.id,
            outcome: HostOutcome::Done(HostResult::Catalog { apps: vec![] }),
        })?))?;
        Ok(())
    }
    fn stop(&mut self) {}
}

#[tokio::test]
async fn child_requests_never_start_the_foreground_host_even_when_login_fails() {
    let temp = tempfile::tempdir().expect("fixture");
    let settings = aura_config::SettingsFacade::open(aura_config::AuraPaths::from_roots(
        temp.path().join("config"),
        temp.path().join("data"),
    ))
    .expect("settings");
    let mut configured = settings.computer_settings().settings;
    configured.enabled = true;
    settings.update_computer_settings(1, &configured).expect("enable");
    let foreground = Arc::new(AtomicUsize::new(0));
    let child = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(Backend::default());
    let root = DesktopControl::with_child(
        settings,
        Arc::new(Desktop(Arc::clone(&foreground))),
        Arc::new(Desktop(Arc::clone(&child))),
        session(backend),
    );
    let selected = root.select(ComputerMode::ChildSession).expect("child");
    selected
        .run(HostCommand::Catalog { manual: vec![] }, Duration::from_secs(1))
        .await
        .expect_err("login required");
    assert_eq!(foreground.load(Ordering::SeqCst), 0);
    assert_eq!(child.load(Ordering::SeqCst), 0);
    assert!(selected.select(ComputerMode::Foreground).is_err());
}

#[tokio::test]
async fn refs_do_not_alias_across_desktops_or_after_reopening_the_child_window() {
    let temp = tempfile::tempdir().expect("fixture");
    let settings = aura_config::SettingsFacade::open(aura_config::AuraPaths::from_roots(
        temp.path().join("config"),
        temp.path().join("data"),
    ))
    .expect("settings");
    let mut configured = settings.computer_settings().settings;
    configured.enabled = true;
    settings.update_computer_settings(1, &configured).expect("enable");
    let backend = Backend::ready();
    let root = DesktopControl::with_child(
        settings,
        Arc::new(Desktop(Arc::default())),
        Arc::new(Desktop(Arc::default())),
        session(Arc::clone(&backend)),
    );
    let child = root.select(ComputerMode::ChildSession).expect("child");
    let owner = SessionId::new();
    let shot = || aura_desktop_service::KnownShot {
        app_id: "fixture".into(),
        map: aura_desktop_domain::ScreenMap {
            left: 0,
            top: 0,
            width: 1,
            height: 1,
            screen_width: 1,
            screen_height: 1,
        },
        windows: vec![],
    };
    let foreground = root.remember_shot(owner, shot());
    child.catalog().await.expect("child catalog");
    let previous = child.remember_shot(owner, shot());
    let turn = caller();
    let app = aura_desktop_domain::AppEntry {
        id: "fixture".into(),
        name: "Fixture".into(),
        kind: aura_desktop_domain::AppKind::Manual,
        path: None,
    };
    child
        .run_for(HostCommand::Launch { app, timeout_ms: 1 }, Duration::from_secs(1), turn)
        .await
        .expect("scripted action");
    backend.say(PipMessage::TakenOver);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !child.taken_over(turn.session, turn.turn.expect("turn")) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("takeover");
    assert!(
        root.controlling(turn.session)
            .iter()
            .any(|app| app.mode == ComputerMode::ChildSession && app.taken_over),
        "the UI must keep showing a taken-over child desktop until its turn ends"
    );
    assert_ne!(previous, foreground, "a foreground image must never resolve on the child desktop");
    assert!(child.shot(owner, &foreground).is_none());
    assert!(root.shot(owner, &previous).is_none());
    backend.say(PipMessage::Closed { signed_out: true });
    tokio::time::timeout(Duration::from_secs(1), async {
        while child.pip_status().expect("pip").phase == aura_desktop_domain::PipPhase::Ready {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("closed");
    child.catalog().await.expect("new opening");
    let current = child.remember_shot(owner, shot());
    assert_ne!(current, previous, "an old opening's refs cannot alias the new host");
    assert!(child.shot(owner, &previous).is_none());
}

#[tokio::test]
async fn the_user_gives_the_child_desktop_back_and_the_same_turn_holds_it_again() {
    let temp = tempfile::tempdir().expect("fixture");
    let settings = aura_config::SettingsFacade::open(aura_config::AuraPaths::from_roots(
        temp.path().join("config"),
        temp.path().join("data"),
    ))
    .expect("settings");
    let mut configured = settings.computer_settings().settings;
    configured.enabled = true;
    settings.update_computer_settings(1, &configured).expect("enable");
    let backend = Backend::ready();
    let root = DesktopControl::with_child(
        settings,
        Arc::new(Desktop(Arc::default())),
        Arc::new(Desktop(Arc::default())),
        session(Arc::clone(&backend)),
    );
    let child = root.select(ComputerMode::ChildSession).expect("child");
    let turn = caller();
    let (owner, id) = (turn.session, turn.turn.expect("turn"));
    let launch = || HostCommand::Launch {
        app: aura_desktop_domain::AppEntry {
            id: "fixture".into(),
            name: "Fixture".into(),
            kind: aura_desktop_domain::AppKind::Manual,
            path: None,
        },
        timeout_ms: 1,
    };
    let holds = || {
        let commands = backend.commands.lock().expect("commands");
        commands
            .iter()
            .filter(|command| matches!(command, PipCommand::Hold { on: true, .. }))
            .count()
    };
    root.turn_runs(owner, id);
    child.run_for(launch(), Duration::from_secs(1), turn).await.expect("scripted action");
    backend.say(PipMessage::TakenOver);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !child.taken_over(owner, id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("takeover");
    let refused = child.run_for(launch(), Duration::from_secs(1), turn).await.expect_err("refused");
    assert_eq!(refused.code, HostErrorCode::TakenOver);
    assert!(refused.message.contains("用户交还操作权之前"), "{}", refused.message);
    let held = holds();

    root.give_back(owner).await;
    assert!(!child.taken_over(owner, id));
    assert!(!root.controlling(owner).iter().any(|app| app.taken_over));
    child.run_for(launch(), Duration::from_secs(1), turn).await.expect("the same turn acts again");
    assert_eq!(holds(), held + 1, "the child desktop is held for the turn again");
}

#[tokio::test]
async fn a_turn_that_ended_with_the_child_desktop_taken_over_stays_listed_until_given_back() {
    let temp = tempfile::tempdir().expect("fixture");
    let settings = aura_config::SettingsFacade::open(aura_config::AuraPaths::from_roots(
        temp.path().join("config"),
        temp.path().join("data"),
    ))
    .expect("settings");
    let mut configured = settings.computer_settings().settings;
    configured.enabled = true;
    settings.update_computer_settings(1, &configured).expect("enable");
    let backend = Backend::ready();
    let root = DesktopControl::with_child(
        settings,
        Arc::new(Desktop(Arc::default())),
        Arc::new(Desktop(Arc::default())),
        session(Arc::clone(&backend)),
    );
    let child = root.select(ComputerMode::ChildSession).expect("child");
    let turn = caller();
    let (owner, id) = (turn.session, turn.turn.expect("turn"));
    let app = aura_desktop_domain::AppEntry {
        id: "fixture".into(),
        name: "Fixture".into(),
        kind: aura_desktop_domain::AppKind::Manual,
        path: None,
    };
    root.turn_runs(owner, id);
    child
        .run_for(HostCommand::Launch { app, timeout_ms: 1 }, Duration::from_secs(1), turn)
        .await
        .expect("scripted action");
    backend.say(PipMessage::TakenOver);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !child.taken_over(owner, id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("takeover");
    root.turn_ended(owner, id).await;
    let listed = root.controlling(owner);
    assert!(
        listed.iter().any(|app| app.mode == ComputerMode::ChildSession && app.taken_over),
        "{listed:?}"
    );
    root.give_back(owner).await;
    assert!(root.controlling(owner).is_empty());
}
