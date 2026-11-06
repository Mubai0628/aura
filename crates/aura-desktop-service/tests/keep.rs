//! A turn keeps the desktop while its model thinks and reads: before a read of the turn that holds
//! the desktop, the host is told to keep the turn's hold, so the banner and the watch on the
//! user's input stay until the turn ends or waits for the user.
mod support;

use aura_config::{AuraPaths, SettingsFacade};
use aura_desktop_domain::*;
use aura_desktop_service::{Caller, DesktopControl, HostLink};
use aura_domain_common::{SessionId, ToolCallId, TurnTaskId};
use std::sync::Arc;
use std::time::Duration;
use support::{Answer, Launcher, Stand};

const LIMIT: Duration = Duration::from_millis(300);

fn typing() -> HostCommand {
    HostCommand::Input {
        app: AppEntry {
            id: "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".into(),
            name: "记事本".into(),
            kind: AppKind::Packaged,
            path: None,
        },
        steps: vec![InputStep::Type { text: "x".into() }],
        window: None,
        clipboard: false,
        system_keys: false,
    }
}

fn answer(command: &HostCommand) -> Answer {
    Answer::Now(HostOutcome::Done(match command {
        HostCommand::Guarded { command, .. } => return answer(command),
        HostCommand::Keep { .. } => HostResult::Kept { kept: true },
        HostCommand::Release { .. } => HostResult::Released,
        HostCommand::Input { steps, .. } => HostResult::Input { steps_done: steps.len() as u32 },
        _ => HostResult::Displays { monitors: vec![] },
    }))
}

fn control(stand: &Arc<Stand>) -> (tempfile::TempDir, Arc<DesktopControl>) {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let settings = SettingsFacade::open(paths).expect("settings");
    let link = HostLink::with_times(
        Arc::new(Launcher(Arc::clone(stand))),
        Duration::from_millis(300),
        Duration::from_millis(100),
    );
    (temp, Arc::new(DesktopControl::with_link(settings, link)))
}

fn caller(session: SessionId, turn: TurnTaskId) -> Caller {
    Caller { call: ToolCallId::new(), session, turn: Some(turn) }
}

fn keeps(stand: &Stand, turn: TurnTaskId) -> usize {
    let keep = HostCommand::Keep { hold: turn.to_string() };
    stand.sent().iter().filter(|command| **command == keep).count()
}

#[tokio::test]
async fn a_turns_reads_keep_the_desktop_it_holds_and_nothing_else_does() {
    let stand = Stand::new(answer);
    let (_temp, control) = control(&stand);
    let (session, turn, other) = (SessionId::new(), TurnTaskId::new(), TurnTaskId::new());
    let read = |turn| control.run_for(HostCommand::Displays, LIMIT, caller(session, turn));
    read(turn).await.expect("a read before the turn acted");
    control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    read(turn).await.expect("a read while the turn holds the desktop");
    read(other).await.expect("another turn's read");
    control.run(HostCommand::Displays, LIMIT).await.expect("a read of no turn");
    let sent = stand.sent();
    assert_eq!(keeps(&stand, turn), 1, "{sent:?}");
    let at =
        sent.iter().position(|command| *command == HostCommand::Keep { hold: turn.to_string() });
    let at = at.expect("kept");
    assert!(matches!(sent[at - 1], HostCommand::Guarded { .. }), "after its action: {sent:?}");
    assert_eq!(sent[at + 1], HostCommand::Displays, "just before its read");
    control.turn_waits(turn).await;
    read(turn).await.expect("a read while the turn waits for the user");
    assert_eq!(keeps(&stand, turn), 1, "nothing is kept while the turn waits");
}

#[tokio::test]
async fn a_hold_the_host_no_longer_keeps_is_not_kept_again() {
    let stand = Stand::new(|command| match command {
        HostCommand::Keep { .. } => {
            Answer::Now(HostOutcome::Done(HostResult::Kept { kept: false }))
        }
        other => answer(other),
    });
    let (_temp, control) = control(&stand);
    let (session, turn) = (SessionId::new(), TurnTaskId::new());
    control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    for _ in 0..2 {
        control.run_for(HostCommand::Displays, LIMIT, caller(session, turn)).await.expect("read");
    }
    assert_eq!(keeps(&stand, turn), 1);
    assert!(control.controlling(session).is_empty(), "no program is shown as controlled");
    control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("its next action");
    control.run_for(HostCommand::Displays, LIMIT, caller(session, turn)).await.expect("read");
    assert_eq!(keeps(&stand, turn), 2, "its next action took the desktop again");
}
