//! The desktop a turn works on: the one its request names, or else the one the settings choose
//! when it starts, kept for the whole turn whatever the settings choose later.
use super::*;

/// Chooses on the settings page the desktop for turns whose request names none.
async fn choose(runtime: &crate::RuntimeState, mode: ComputerMode) {
    let snapshot = crate::r03_tests::success(runtime, "computer.settings.get", json!({})).await;
    let mut settings = snapshot["settings"].clone();
    settings["mode"] = json!(mode);
    crate::r03_tests::success(
        runtime,
        "computer.settings.update",
        json!({"expected_revision": snapshot["revision"], "settings": settings}),
    )
    .await;
}

#[tokio::test]
async fn a_turn_that_names_no_desktop_keeps_the_one_the_settings_chose_when_it_started() {
    let mut desktop = Desktop::new(Scenario::Text, PermissionMode::WorkspaceReadWrite, as_is).await;
    desktop.start().await;
    assert_eq!(desktop.wait(completed).await.computer_mode, None, "the user's own desktop");
    choose(&desktop.runtime, ComputerMode::ChildSession).await;
    desktop.start().await;
    assert_eq!(desktop.wait(completed).await.computer_mode, Some(ComputerMode::ChildSession));
    choose(&desktop.runtime, ComputerMode::Foreground).await;
    assert_eq!(
        desktop.wait(completed).await.computer_mode,
        Some(ComputerMode::ChildSession),
        "the turn keeps the desktop it started with"
    );
}

#[tokio::test]
async fn a_turn_that_names_its_desktop_keeps_it_whatever_the_settings_choose() {
    let mut desktop = Desktop::new(Scenario::Text, PermissionMode::WorkspaceReadWrite, as_is).await;
    choose(&desktop.runtime, ComputerMode::ChildSession).await;
    let started = crate::r03_tests::success(
        &desktop.runtime,
        "agent.start",
        json!(AgentStartRequest {
            computer_mode: Some(ComputerMode::Foreground),
            session_id: desktop.session,
            model_profile_id: desktop.model,
            text: "Check the answer and finish".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            speed_mode: None,
            attachments: Vec::new(),
            references: Vec::new(),
        }),
    )
    .await;
    let started: AgentRunProjection = serde_json::from_value(started).expect("started");
    desktop.request = Some(AgentRunRequest {
        session_id: desktop.session,
        agent_run_id: started.run.agent_run_id,
    });
    assert_eq!(desktop.wait(completed).await.computer_mode, Some(ComputerMode::Foreground));
}
