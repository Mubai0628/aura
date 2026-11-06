//! Which programs a session may use without asking because they are always allowed, which ones
//! it is controlling now, and stopping them; the catalog of programs and their icons, read once
//! and reused.
mod support;

use aura_config::{ComputerAddedApp, ComputerApp, ComputerControlSettings};
use aura_desktop_domain::*;
use aura_desktop_service::{ICON_SIZE, ICONS_PER_READ};
use aura_domain_common::SessionId;
use std::time::Duration;
use support::turns::*;
use support::{Answer, Stand};

#[test]
fn stop_blocks_the_active_turn_even_if_the_program_is_always_allowed() {
    let setup = quiet();
    let session = SessionId::new();
    let turn = aura_domain_common::TurnTaskId::new();
    let active = setup.control.begin_turn(session, &notepad(), true, Some(turn));
    setup.control.stop(session, NOTEPAD);
    assert!(setup.control.turn_stopped(session, turn, NOTEPAD));
    drop(active);
    assert!(setup.control.turn_stopped(session, turn, NOTEPAD));
    assert!(!setup.control.turn_stopped(session, aura_domain_common::TurnTaskId::new(), NOTEPAD));
}

#[test]
fn safety_inspection_is_bound_to_the_call_and_exact_host_command() {
    let setup = quiet();
    let call = aura_domain_common::ToolCallId::new();
    let command = HostCommand::Input {
        app: notepad(),
        steps: vec![InputStep::Type { text: "one".into() }],
        window: None,
        clipboard: false,
        system_keys: false,
    };
    assert_eq!(setup.control.inspection(call, &command), None);
    setup.control.remember_inspection(call, command.clone(), "password".into());
    assert_eq!(setup.control.inspection(call, &command).as_deref(), Some("password"));
    assert_eq!(setup.control.inspection(aura_domain_common::ToolCallId::new(), &command), None);
    let changed = HostCommand::Input {
        app: notepad(),
        steps: vec![InputStep::Type { text: "two".into() }],
        window: None,
        clipboard: false,
        system_keys: false,
    };
    assert_eq!(setup.control.inspection(call, &changed), None);
}

#[test]
fn an_always_allowed_program_asks_again_only_in_a_session_that_stopped_it() {
    let setup = quiet();
    let (first, second) = (SessionId::new(), SessionId::new());
    assert!(!setup.control.always_allowed(first, NOTEPAD), "nothing is always allowed at first");
    setup
        .control
        .always_allow(ComputerApp { app_id: NOTEPAD.into(), name: "记事本".into() })
        .expect("allowed");
    assert!(
        setup.control.always_allowed(first, NOTEPAD)
            && setup.control.always_allowed(second, NOTEPAD)
    );
    setup.control.stop(first, NOTEPAD);
    assert!(!setup.control.always_allowed(first, NOTEPAD), "the session that stopped it");
    assert!(setup.control.always_allowed(second, NOTEPAD), "another session");
    assert!(!setup.control.always_allowed(second, "Microsoft.Paint_8wekyb3d8bbwe!App"));
}

#[test]
fn always_allowing_a_program_writes_it_into_the_settings_once() {
    let setup = quiet();
    let app = ComputerApp { app_id: NOTEPAD.into(), name: "记事本".into() };
    setup.control.always_allow(app.clone()).expect("allowed");
    setup.control.always_allow(app.clone()).expect("allowed again");
    let snapshot = setup.settings.computer_settings();
    assert_eq!(snapshot.settings.always_allowed, [app]);
    assert_eq!(snapshot.revision, Some(2), "the second time changed nothing");
}

#[test]
fn a_program_is_controlled_while_a_call_runs_on_it_and_stopping_it_tells_the_call() {
    let setup = quiet();
    let (session, other) = (SessionId::new(), SessionId::new());
    let background = setup.control.begin(session, &notepad(), false);
    let mut foreground = setup.control.begin(session, &notepad(), true);
    let shown = setup.control.controlling(session);
    assert_eq!(shown.len(), 1, "one program, whatever the number of calls: {shown:?}");
    assert_eq!((shown[0].app_id.as_str(), shown[0].name.as_str()), (NOTEPAD, "记事本"));
    assert!(shown[0].foreground, "one of its calls has the foreground");
    assert!(setup.control.controlling(other).is_empty(), "another session controls nothing");

    assert!(!foreground.stopped.has_changed().expect("watch"));
    setup.control.stop(session, NOTEPAD);
    assert!(foreground.stopped.has_changed().expect("watch"), "the running call hears of it");
    foreground.stopped.mark_unchanged();
    drop(foreground);
    let shown = setup.control.controlling(session);
    assert!(!shown[0].foreground, "the call with the foreground ended");
    drop(background);
    assert!(setup.control.controlling(session).is_empty());
}

#[tokio::test]
async fn the_catalog_is_read_with_the_added_programs_and_reused_for_a_while() {
    let entry = notepad();
    let listed = entry.clone();
    let setup = setup(Stand::new(move |_| {
        Answer::Now(HostOutcome::Done(HostResult::Catalog { apps: vec![listed.clone()] }))
    }));
    let added = ComputerControlSettings {
        added: vec![ComputerAddedApp { path: "C:\\Tools\\viewer.exe".into(), name: String::new() }],
        ..ComputerControlSettings::default()
    };
    setup.settings.update_computer_settings(1, &added).expect("save");
    assert_eq!(setup.control.catalog().await, Ok(vec![entry.clone()]));
    assert_eq!(setup.control.catalog().await, Ok(vec![entry]));
    let sent = setup.stand.sent();
    assert_eq!(
        sent,
        [HostCommand::Catalog {
            manual: vec![ManualApp { name: String::new(), path: "C:\\Tools\\viewer.exe".into() }]
        }],
        "read once, with the program the user added"
    );
    setup.control.refresh_catalog();
    setup.control.catalog().await.expect("read again");
    assert_eq!(setup.stand.sent().len(), 2);
}

#[tokio::test]
async fn a_failure_of_the_host_reaches_the_caller_as_the_hosts_own_error() {
    let setup = setup(Stand::new(|_| {
        Answer::Now(HostOutcome::Failed(HostError::new(
            HostErrorCode::Stale,
            "编号过期",
            Delivery::NotDelivered,
        )))
    }));
    let failed = setup.control.run(HostCommand::Displays, Duration::from_millis(300)).await;
    assert_eq!(failed.map_err(|error| error.code), Err(HostErrorCode::Stale));
}

fn known(name: &str, acts: bool) -> aura_desktop_service::KnownElement {
    aura_desktop_service::KnownElement {
        window: "w1".into(),
        key: ElementKey {
            runtime_id: vec![1],
            control_type: 50_000,
            automation_id: String::new(),
            name: name.into(),
        },
        rect: Rect { x: 0, y: 0, width: 10, height: 10 },
        acts,
    }
}

#[test]
fn the_latest_read_of_a_program_is_kept_by_its_refs_numbers() {
    let setup = quiet();
    let session = SessionId::new();
    let first = setup.control.remember_elements(
        session,
        NOTEPAD,
        vec![known("a", true), known("b", false)],
    );
    assert_eq!(first, ["e1", "e2"]);
    assert_eq!(setup.control.remember_elements(session, NOTEPAD, vec![known("c", true)]), ["e3"]);
    setup.control.replace_element(session, NOTEPAD, "e3", known("c2", true));
    assert_eq!(setup.control.latest_read(session, NOTEPAD), vec![(3, known("c2", true))]);
    assert_eq!(setup.control.element(session, NOTEPAD, "e1"), Some(known("a", true)), "e1 stands");
    assert!(setup.control.latest_read(SessionId::new(), NOTEPAD).is_empty(), "another session");
    assert!(setup.control.latest_read(session, "other").is_empty(), "another program");
    assert!(setup.control.remember_elements(session, NOTEPAD, vec![]).is_empty());
    assert!(
        setup.control.latest_read(session, NOTEPAD).is_empty(),
        "the latest read found nothing"
    );
}

#[tokio::test]
async fn icons_are_read_once_each_in_small_reads_and_only_for_programs_of_the_catalog() {
    let apps: Vec<AppEntry> = (0..45)
        .map(|n| AppEntry {
            id: format!("app{n}"),
            name: format!("程序{n}"),
            kind: AppKind::Desktop,
            path: None,
        })
        .collect();
    let listed = apps.clone();
    let setup = setup(Stand::new(move |command| {
        Answer::Now(HostOutcome::Done(match command {
            HostCommand::Catalog { .. } => HostResult::Catalog { apps: listed.clone() },
            HostCommand::Icons { apps, .. } => HostResult::Icons {
                icons: apps
                    .iter()
                    .filter(|app| app.id != "app1")
                    .map(|app| AppIcon { app_id: app.id.clone(), png: format!("{}.png", app.id) })
                    .collect(),
            },
            other => panic!("{other:?}"),
        }))
    }));
    let mut ids: Vec<String> = apps.iter().map(|app| app.id.clone()).collect();
    ids.insert(2, r"C:\Windows\System32\cmd.exe".into());
    let icons = setup.control.icons(&ids).await.expect("icons");
    assert_eq!(icons.len(), 44, "app1 has none, and the path is no program of the catalog");
    assert_eq!(icons[0], AppIcon { app_id: "app0".into(), png: "app0.png".into() });
    let again = setup.control.icons(&["app1".into(), "app0".into()]).await.expect("again");
    assert_eq!(again, [AppIcon { app_id: "app0".into(), png: "app0.png".into() }]);
    let reads: Vec<(usize, u32)> = setup
        .stand
        .sent()
        .into_iter()
        .filter_map(|command| match command {
            HostCommand::Icons { apps, size } => Some((apps.len(), size)),
            _ => None,
        })
        .collect();
    assert_eq!(
        reads,
        [(ICONS_PER_READ, ICON_SIZE), (45 - ICONS_PER_READ, ICON_SIZE)],
        "each program is read once, one without an icon too"
    );
}

#[tokio::test]
async fn a_turns_actions_keep_the_desktop_in_the_turns_hold_and_its_reads_only_keep_it_longer() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), aura_domain_common::TurnTaskId::new());
    setup.control.run(typing(), LIMIT).await.expect("an action of no turn");
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.control.run_for(HostCommand::Displays, LIMIT, caller(session, turn)).await.expect("read");
    assert_eq!(
        setup.stand.sent(),
        [
            HostCommand::Guarded {
                command: Box::new(typing()),
                hold: None,
                stop_keys: Some(aura_desktop_domain::pip::PIP_STOP_KEYS.into()),
            },
            kept(turn),
            HostCommand::Keep { hold: turn.to_string() },
            HostCommand::Displays,
        ]
    );
}

#[tokio::test]
async fn the_turns_program_stays_controlled_between_its_calls_until_the_turn_ends() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), aura_domain_common::TurnTaskId::new());
    let call = setup.control.begin_turn(session, &notepad(), true, Some(turn));
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    drop(call);
    assert_eq!(setup.control.controlling(session), [notepad_controlled(false)]);
    assert!(setup.control.controlling(SessionId::new()).is_empty());
    setup.control.turn_ended(session, turn).await;
    assert!(setup.control.controlling(session).is_empty());
    assert_eq!(setup.stand.sent().last(), Some(&HostCommand::Release { hold: turn.to_string() }));
}

#[tokio::test]
async fn a_takeover_between_calls_shows_and_the_turns_next_action_never_reaches_the_desktop() {
    let setup = setup(Stand::new(ordinary));
    setup
        .control
        .always_allow(ComputerApp { app_id: NOTEPAD.into(), name: "记事本".into() })
        .expect("allowed");
    let (session, turn) = (SessionId::new(), aura_domain_common::TurnTaskId::new());
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.stand.say(&HostMessage::HoldEnded {
        hold: turn.to_string(),
        reason: HoldEnd::TakenOver,
        cause: Some("点了鼠标".into()),
    });
    until(|| setup.control.taken_over(session, turn)).await;
    assert_eq!(setup.control.controlling(session), [notepad_controlled(true)]);
    let sent = setup.stand.sent().len();
    let refused =
        setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect_err("refused");
    assert_eq!(
        (refused.code, refused.delivery),
        (HostErrorCode::TakenOver, Delivery::NotDelivered)
    );
    assert!(refused.message.contains("用户已接管电脑（点了鼠标）"), "{}", refused.message);
    assert_eq!(setup.control.takeover_cause(turn).as_deref(), Some("点了鼠标"));
    assert_eq!(setup.stand.sent().len(), sent, "nothing reached the host");
    setup
        .control
        .run_for(HostCommand::Displays, LIMIT, caller(session, turn))
        .await
        .expect("the model may still look");
    assert!(setup.control.always_allowed(session, NOTEPAD), "taking over keeps the answers");
    let sent = setup.stand.sent().len();
    setup.control.turn_ended(session, turn).await;
    assert_eq!(setup.stand.sent().len(), sent, "nothing kept, nothing to give back");
    assert_eq!(
        setup.control.controlling(session),
        [notepad_controlled(true)],
        "listed until the desktop is given back or the next turn runs"
    );
    let next = aura_domain_common::TurnTaskId::new();
    setup.control.turn_runs(session, next);
    assert!(setup.control.controlling(session).is_empty());
    setup.control.run_for(typing(), LIMIT, caller(session, next)).await.expect("the next turn");
}

#[tokio::test]
async fn an_action_the_user_took_over_while_it_ran_ends_the_turns_actions() {
    let setup = setup(Stand::new(|command| match command {
        HostCommand::Guarded { .. } => Answer::Now(HostOutcome::Failed(HostError {
            code: HostErrorCode::TakenOver,
            message: "用户已接管电脑".into(),
            delivery: Delivery::Uncertain,
            steps_done: Some(1),
        })),
        other => ordinary(other),
    }));
    let (session, turn) = (SessionId::new(), aura_domain_common::TurnTaskId::new());
    let error =
        setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect_err("taken");
    assert_eq!(error.steps_done, Some(1));
    assert!(setup.control.taken_over(session, turn));
    let sent = setup.stand.sent().len();
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect_err("refused");
    assert_eq!(setup.stand.sent().len(), sent);
    assert_eq!(setup.control.controlling(session), [notepad_controlled(true)]);
}

#[tokio::test]
async fn a_hold_given_back_after_a_quiet_while_is_taken_again_by_the_turns_next_action() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), aura_domain_common::TurnTaskId::new());
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.stand.say(&HostMessage::HoldEnded {
        hold: turn.to_string(),
        reason: HoldEnd::Idle,
        cause: None,
    });
    until(|| setup.control.controlling(session).is_empty()).await;
    assert!(!setup.control.taken_over(session, turn));
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed again");
    assert_eq!(setup.stand.sent().last(), Some(&kept(turn)));
    assert_eq!(setup.control.controlling(session), [notepad_controlled(false)]);
}

#[tokio::test]
async fn another_conversation_waits_while_a_turn_keeps_the_desktop() {
    let setup = setup(Stand::new(ordinary));
    let (first, second) = (SessionId::new(), SessionId::new());
    let (one, two) = (aura_domain_common::TurnTaskId::new(), aura_domain_common::TurnTaskId::new());
    setup.control.run_for(typing(), LIMIT, caller(first, one)).await.expect("typed");
    let sent = setup.stand.sent().len();
    let busy = setup.control.run_for(typing(), LIMIT, caller(second, two)).await.expect_err("busy");
    assert_eq!((busy.code, busy.delivery), (HostErrorCode::Busy, Delivery::NotDelivered));
    assert!(setup.control.run(typing(), LIMIT).await.is_err(), "an action of no turn waits too");
    assert_eq!(setup.stand.sent().len(), sent, "nothing reached the host");
    setup.control.run_for(HostCommand::Displays, LIMIT, caller(second, two)).await.expect("read");
    setup.control.turn_ended(first, one).await;
    setup.control.run_for(typing(), LIMIT, caller(second, two)).await.expect("its turn now");
}

#[tokio::test]
async fn stopping_a_conversation_gives_the_desktop_back() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), aura_domain_common::TurnTaskId::new());
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.control.stop(session, NOTEPAD);
    setup.control.release(session).await;
    assert_eq!(setup.stand.sent().last(), Some(&HostCommand::Release { hold: turn.to_string() }));
    assert!(setup.control.controlling(session).is_empty());
    let sent = setup.stand.sent().len();
    setup.control.release(SessionId::new()).await;
    assert_eq!(setup.stand.sent().len(), sent, "another conversation keeps nothing");
}

#[tokio::test]
async fn a_turn_waiting_for_the_user_gives_the_desktop_back_until_its_next_action() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), aura_domain_common::TurnTaskId::new());
    let sent = setup.stand.sent().len();
    setup.control.turn_waits(turn).await;
    assert_eq!(setup.stand.sent().len(), sent, "nothing kept, nothing to give back");
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.control.turn_waits(turn).await;
    assert_eq!(setup.stand.sent().last(), Some(&HostCommand::Release { hold: turn.to_string() }));
    assert!(!setup.control.taken_over(session, turn), "waiting is no takeover");
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("approved");
    assert_eq!(setup.stand.sent().last(), Some(&kept(turn)));
}
