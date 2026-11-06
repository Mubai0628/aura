//! Arranging a program's window: the call reaches the host as it was resolved, a window that
//! already was as asked changes nothing, and one that stays open when asked to close is said to
//! be open.
use super::*;

fn already_minimized(command: &HostCommand) -> Option<HostOutcome> {
    matches!(command, HostCommand::Window { .. }).then(|| {
        HostOutcome::Done(HostResult::Window {
            window: "w1312".into(),
            changed: false,
            windows: vec![WindowInfo { minimized: true, ..window() }],
        })
    })
}

const MINIMIZE: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.window", r#"{"app":"记事本","window":"1312","action":"minimize"}"#),
        ("computer.window", r#"{"app":"记事本","window":"w1312","action":"minimize"}"#),
    ],
};

#[tokio::test]
async fn a_window_already_as_asked_changes_nothing_and_a_wrong_ref_reaches_nothing() {
    let mut desktop = Desktop::new(MINIMIZE, PermissionMode::FullAccess, already_minimized).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert!(desktop.returned(0).contains("computer.window"), "{}", desktop.returned(0));
    assert_eq!(
        desktop.acted(),
        [HostCommand::Window {
            app: notepad(),
            window: Some("w1312".into()),
            action: WindowAction::Minimize,
        }]
    );
    let minimized = desktop.returned(1);
    assert!(minimized.contains(r#""changed":false"#), "{minimized}");
    assert_eq!(desktop.side_effects("computer.window"), [r#""no_effect""#]);
}

/// Notepad asks whether to save: its question is a window of its own, in front.
fn asks_to_save(command: &HostCommand) -> Option<HostOutcome> {
    matches!(command, HostCommand::Window { .. }).then(|| {
        let question = WindowInfo { window: "w2024".into(), title: "记事本".into(), ..window() };
        HostOutcome::Done(HostResult::Window {
            window: "w1312".into(),
            changed: true,
            windows: vec![question, window()],
        })
    })
}

const CLOSE: Scenario =
    Scenario::Sequence { calls: &[("computer.window", r#"{"app":"记事本","action":"close"}"#)] };

#[tokio::test]
async fn a_window_that_stays_open_when_asked_to_close_is_said_to_be_open() {
    let mut desktop = Desktop::new(CLOSE, PermissionMode::FullAccess, asks_to_save).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert_eq!(
        desktop.acted(),
        [HostCommand::Window { app: notepad(), window: None, action: WindowAction::Close }]
    );
    let closed = desktop.returned(0);
    assert!(closed.contains("窗口还开着") && closed.contains("w2024"), "{closed}");
    assert_eq!(desktop.side_effects("computer.window"), [r#""fully_applied""#]);
}
