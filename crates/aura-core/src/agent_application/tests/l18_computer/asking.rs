//! A program asks once for the session and an always-allowed one not at all, until the
//! user's Stop withdraws the answer.
use super::*;

const LIST_READ_ACT: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.apps", "{}"),
        ("computer.snapshot", r#"{"app":"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"}"#),
        (
            "computer.act",
            r#"{"app":"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App","steps":[{"action":"set_value","ref":"e2","value":"你好"},{"action":"click","ref":"e3"}]}"#,
        ),
    ],
};

#[tokio::test]
async fn a_program_asks_once_for_the_session_and_is_then_read_and_acted_on_without_asking() {
    let mut desktop = Desktop::new(LIST_READ_ACT, PermissionMode::WorkspaceReadWrite, as_is).await;
    switch(&desktop.runtime, true).await;
    desktop.start().await;
    let (id, approval) = desktop.question().await;
    assert_eq!(decisions(&approval), ["allow_session", "deny"]);
    assert!(desktop.acted().is_empty(), "nothing reaches the program before the answer");
    let listed = desktop.returned(0);
    assert!(
        listed.contains(NOTEPAD)
            && listed.contains(r#""running":true"#)
            && listed.contains(r#""allowed":false"#),
        "{listed}"
    );
    desktop.answer(id, &approval, "allow_session").await;
    let view = desktop
        .wait(|view| completed(view) || view.pending_approval_ids.iter().any(|o| *o != id))
        .await;
    assert!(
        completed(&view),
        "the session's answer covers the act: {:?}",
        view.pending_approval_ids
    );
    let read = desktop.returned(1);
    assert!(
        read.contains(r#"e2 edit \"文字\" [set_value, focus]"#)
            && read.contains(r#"e3 button \"保存\" [click]"#),
        "{read}"
    );
    let found = elements();
    assert_eq!(
        desktop.acted(),
        [
            HostCommand::Elements {
                app: notepad(),
                window: None,
                root: None,
                max_elements: 400,
                max_depth: MAX_DEPTH,
                timeout_ms: 10_000,
            },
            HostCommand::Act {
                app: notepad(),
                window: "w1312".into(),
                steps: vec![
                    ActStep::SetValue { element: found[1].key.clone(), value: "你好".into() },
                    ActStep::Click { element: found[2].key.clone() },
                ],
                timeout_ms: 20_000,
            },
        ],
        "each ref is the element it stood for"
    );
    assert!(desktop.returned(2).contains(r#""steps_done":2"#));
    assert_eq!(desktop.side_effects("computer.act"), [r#""fully_applied""#]);
    assert!(desktop.allowed_in_session().contains(NOTEPAD));
    // The user's Stop withdraws the session's answer: the next call asks again.
    let stopped = crate::r03_tests::success(
        &desktop.runtime,
        "computer.stop",
        json!({"session_id": desktop.session, "app_id": NOTEPAD}),
    )
    .await;
    assert_eq!(stopped["apps"], json!([]));
    assert!(desktop.allowed_in_session().is_empty());
}

const READ: Scenario = Scenario::Sequence {
    calls: &[("computer.snapshot", r#"{"app":"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"}"#)],
};

#[tokio::test]
async fn an_always_allowed_program_does_not_ask_until_the_session_stops_it() {
    let mut desktop = Desktop::new(READ, PermissionMode::ReadOnly, as_is).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    let view = desktop.wait(|view| completed(view) || asking(view)).await;
    assert!(completed(&view), "nothing asked, even in ReadOnly");
    assert_eq!(desktop.acted().len(), 1);
    let control = control(&desktop.runtime);
    assert!(control.always_allowed(desktop.session, NOTEPAD));
    crate::r03_tests::success(
        &desktop.runtime,
        "computer.stop",
        json!({"session_id": desktop.session, "app_id": NOTEPAD}),
    )
    .await;
    assert!(!control.always_allowed(desktop.session, NOTEPAD), "this session asks again");
    let settings =
        crate::r03_tests::success(&desktop.runtime, "computer.settings.get", json!({})).await;
    assert_eq!(
        settings["settings"]["always_allowed"],
        json!([{"app_id": NOTEPAD, "name": "记事本"}]),
        "the list stays for other sessions"
    );
}
