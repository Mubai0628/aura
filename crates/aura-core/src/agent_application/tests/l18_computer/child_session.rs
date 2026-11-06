//! A turn whose computer control works in the 小窗. The Shell still runs on the user's own desktop,
//! so a script that would open a page or a browser there does not run, and the task state tells
//! the model where its computer tools work.
use super::*;

/// Computer control on, in the 小窗 for the turns that name no desktop.
async fn in_the_child_session(runtime: &crate::RuntimeState) {
    let snapshot = crate::r03_tests::success(runtime, "computer.settings.get", json!({})).await;
    let mut settings = snapshot["settings"].clone();
    settings["enabled"] = json!(true);
    settings["mode"] = json!(ComputerMode::ChildSession);
    crate::r03_tests::success(
        runtime,
        "computer.settings.update",
        json!({"expected_revision": snapshot["revision"], "settings": settings}),
    )
    .await;
}

/// The latest request's task state, the message after the conversation.
fn state(desktop: &Desktop) -> String {
    let request =
        desktop.provider.requests.lock().expect("requests").last().cloned().expect("asked");
    let messages = request["messages"].as_array().expect("messages");
    messages.last().and_then(|state| state["content"].as_str()).expect("the state").to_owned()
}

/// Opens a page only when run, and is never run past its condition: if the guard missed it,
/// nothing would open on the desktop of the machine running the test.
const OPENS_A_PAGE: &str =
    r#"{"script":"if ($false) { Start-Process chrome.exe https://example.invalid/ }"}"#;
const PRINTS: &str = r#"{"script":"Write-Output still-here"}"#;

#[tokio::test]
async fn a_script_that_would_open_a_page_on_the_users_desktop_does_not_run_from_a_child_session_turn()
 {
    const CALLS: Scenario = Scenario::Sequence {
        calls: &[("shell.powershell", OPENS_A_PAGE), ("shell.powershell", PRINTS)],
    };
    let mut desktop = Desktop::new(CALLS, PermissionMode::WorkspaceReadWrite, as_is).await;
    in_the_child_session(&desktop.runtime).await;
    desktop.start().await;
    assert_eq!(desktop.wait(completed).await.computer_mode, Some(ComputerMode::ChildSession));
    let refused = desktop.returned(0);
    assert!(refused.contains("computer.child_session_desktop"), "{refused}");
    assert!(refused.contains("computer.open"), "{refused}");
    let ran = desktop.returned(1);
    assert!(ran.contains("still-here"), "a script that opens nothing still runs: {ran}");
}

#[tokio::test]
async fn the_task_state_says_where_the_computer_tools_work_only_for_a_child_session_turn() {
    let mut desktop = Desktop::new(Scenario::Text, PermissionMode::WorkspaceReadWrite, as_is).await;
    switch(&desktop.runtime, true).await;
    desktop.start().await;
    desktop.wait(completed).await;
    let own = state(&desktop);
    assert!(own.contains("Current-Task-State"), "{own}");
    assert!(!own.contains("computer_control"), "the user's own desktop needs no note: {own}");

    in_the_child_session(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    let child = state(&desktop);
    assert!(child.contains("\"computer_control\""), "{child}");
    assert!(child.contains("computer.open"), "{child}");
    assert!(
        child.contains("even where a command would be quicker"),
        "the 小窗 the user chose comes before the cheaper command line: {child}"
    );
    assert!(
        child.contains("refuses a script that would open a page, a browser or a folder window"),
        "the note claims only the scripts Aura refuses: {child}"
    );
}
