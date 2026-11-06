use aura_desktop_domain::pip::{PipCommand, PipMessage};
use aura_desktop_pip::pip::state::{Effect, Event, Pip, Status};

fn connected() -> Pip {
    let mut pip = Pip::new();
    pip.handle(Event::Start { existing: None });
    pip.handle(Event::SignedIn { session: Some(3) });
    pip
}

#[test]
fn the_control_banner_names_the_child_window() {
    assert_eq!(Status::Controlling.text(), "正在操控小窗");
    assert_eq!(Status::TakenOver.text(), "你已接管小窗");
}

#[test]
fn an_existing_session_needs_consent_and_successful_signout_before_connecting() {
    let mut pip = Pip::new();
    assert_eq!(pip.handle(Event::Start { existing: Some(3) }), vec![Effect::AskSignOutExisting(3)]);
    let mut declined = Pip::new();
    declined.handle(Event::Start { existing: Some(3) });
    assert_eq!(
        declined.handle(Event::ExistingAnswered { sign_out: false }),
        vec![Effect::CloseKeeping]
    );
    let effects = pip.handle(Event::ExistingAnswered { sign_out: true });
    assert!(effects.contains(&Effect::SignOut(3)));
    assert!(!effects.contains(&Effect::Connect), "wait for Windows to confirm the sign-out");
    assert!(pip.handle(Event::ExistingSignedOut { success: true }).contains(&Effect::Connect));
}

#[test]
fn failed_signout_or_input_protection_never_acknowledges_control() {
    for event in [Event::ExistingSignedOut { success: false }, Event::ProtectionFailed] {
        let mut pip = connected();
        let effects = pip.handle(event);
        assert!(
            effects.iter().any(|effect| matches!(effect, Effect::Say(PipMessage::Failed { .. })))
        );
        assert!(effects.contains(&Effect::CloseKeeping));
        assert!(!effects.contains(&Effect::Connect));
        assert!(pip.handle(Event::Command(PipCommand::Hold { on: true, request: 1 })).is_empty());
    }
}

#[test]
fn late_login_after_disconnect_cannot_reenable_input_or_report_ready() {
    let mut pip = connected();
    pip.handle(Event::Command(PipCommand::Hold { on: true, request: 1 }));
    pip.handle(Event::Disconnected { reason: 4 });
    assert!(!pip.holding());
    assert!(!pip.signed_in());
    assert!(pip.handle(Event::SignedIn { session: Some(3) }).is_empty());
    assert!(!pip.signed_in());
}

#[test]
fn holding_gates_input_and_each_takeover_only_reports_once() {
    for event in [Event::ClickedIn, Event::StopKeys, Event::StopButton] {
        let mut pip = connected();
        let effects = pip.handle(Event::Command(PipCommand::Hold { on: true, request: 1 }));
        assert!(pip.holding());
        assert!(effects.contains(&Effect::PassInput(false)));
        assert!(effects.contains(&Effect::Watch(true)));
        let effects = pip.handle(event.clone());
        assert!(!pip.holding());
        assert_eq!(effects.first(), Some(&Effect::Say(PipMessage::TakenOver)));
        assert!(effects.contains(&Effect::PassInput(true)));
        assert!(effects.contains(&Effect::Status(Status::TakenOver)));
        assert!(pip.handle(event).is_empty());
    }
}

#[test]
fn login_remains_usable_and_reconnect_does_not_restore_the_old_turns_hold() {
    let mut pip = Pip::new();
    pip.handle(Event::Start { existing: None });
    assert!(pip.handle(Event::Command(PipCommand::Hold { on: true, request: 1 })).is_empty());
    assert!(pip.handle(Event::ClickedIn).is_empty());
    assert!(pip.handle(Event::SignedIn { session: Some(3) }).contains(&Effect::PassInput(false)));
    pip.handle(Event::Disconnected { reason: 4 });
    assert!(pip.handle(Event::ReconnectButton).contains(&Effect::Connect));
    assert!(pip.handle(Event::SignedIn { session: Some(3) }).contains(&Effect::PassInput(true)));
    assert!(!pip.holding());
}

#[test]
fn close_is_terminal_and_a_lost_core_does_not_destroy_unsaved_work() {
    for event in [Event::CloseConfirmed, Event::Command(PipCommand::Close)] {
        let mut pip = connected();
        assert!(pip.handle(event).contains(&Effect::SignOutAndClose));
        assert!(pip.handle(Event::ReconnectButton).is_empty());
        assert!(pip.handle(Event::SignedIn { session: Some(3) }).is_empty());
    }
    let mut pip = connected();
    assert!(pip.handle(Event::CoreGone).contains(&Effect::CloseKeeping));
}
