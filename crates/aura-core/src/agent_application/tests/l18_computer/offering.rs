//! Which runs get the computer tools: every step while computer control is on, and only a
//! model the settings name or, when they name none, one that takes images.
use super::*;

#[tokio::test]
async fn the_computer_tools_are_in_every_step_while_computer_control_is_on_and_never_otherwise() {
    let mut desktop = Desktop::new(Scenario::Text, PermissionMode::WorkspaceReadWrite, as_is).await;
    desktop.start().await;
    desktop.wait(completed).await;
    let off = desktop.provider.requests.lock().expect("requests").last().cloned().expect("asked");
    assert!(offered(&off).is_empty(), "off: none of them");
    switch(&desktop.runtime, true).await;
    desktop.start().await;
    desktop.wait(completed).await;
    let on = desktop.provider.requests.lock().expect("requests").last().cloned().expect("asked");
    assert_eq!(
        offered(&on),
        [
            "computer.apps",
            "computer.open",
            "computer.snapshot",
            "computer.screenshot",
            "computer.act",
            "computer.input",
            "computer.window"
        ]
    );
    assert!(desktop.played.sent().is_empty(), "offering the tools touches nothing");
}

/// The fixture's profile, told whether its model takes images.
async fn takes_images(runtime: &crate::RuntimeState, model: ModelProfileId, on: bool) {
    let success = crate::r03_tests::success;
    let revision =
        success(runtime, "models.settings.get_snapshot", json!({})).await["models_revision"]
            .clone();
    success(
        runtime,
        "models.profile.update",
        json!({"expected_revision":revision,"model_profile_id":model,"display_name":"Fixture",
            "notes":null,"enabled":true,"request_defaults":{"max_output_tokens":4096,
            "temperature":null,"top_p":null,"chat_output_token_field":null,
            "chat_instruction_role":null,"image_input":on},
            "historical_snapshot_max_tokens":1024}),
    )
    .await;
}

/// The models 设置 → 电脑操控 names.
async fn name_models(runtime: &crate::RuntimeState, models: Value) {
    let snapshot = crate::r03_tests::success(runtime, "computer.settings.get", json!({})).await;
    let mut settings = snapshot["settings"].clone();
    settings["models"] = models;
    crate::r03_tests::success(
        runtime,
        "computer.settings.update",
        json!({"expected_revision": snapshot["revision"], "settings": settings}),
    )
    .await;
}

impl Desktop {
    /// The computer tools of the latest request to the model.
    fn last_offered(&self) -> Vec<String> {
        let requests = self.provider.requests.lock().expect("requests");
        offered(requests.last().expect("asked"))
    }
}

#[tokio::test]
async fn with_no_model_named_the_tools_go_only_to_a_model_that_takes_images() {
    let mut desktop = Desktop::new(Scenario::Text, PermissionMode::WorkspaceReadWrite, as_is).await;
    switch(&desktop.runtime, true).await;
    takes_images(&desktop.runtime, desktop.model, false).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert!(desktop.last_offered().is_empty(), "no images and no model named: none");
    name_models(&desktop.runtime, json!([desktop.model.to_string()])).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert_eq!(desktop.last_offered().len(), 7, "named in the settings, images or not");
}

#[tokio::test]
async fn a_model_the_settings_do_not_name_gets_no_computer_tools() {
    let mut desktop = Desktop::new(Scenario::Text, PermissionMode::WorkspaceReadWrite, as_is).await;
    switch(&desktop.runtime, true).await;
    name_models(&desktop.runtime, json!(["another-model"])).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert!(desktop.last_offered().is_empty(), "it takes images, but another model is named");
}
