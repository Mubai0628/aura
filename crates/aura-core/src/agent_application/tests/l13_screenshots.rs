//! A screenshot reaches a model that takes
//! images, as an image right after its tool result; a model that does not is told so in the
//! result and is sent no image. Whether a model takes images is its profile's setting.
use super::*;

/// The first bytes of a JPEG: enough for Core to recognise it, nothing decodes them.
pub(super) const JPEG_BASE64: &str = "/9j/4AAQSkZJRg==";

const SHOOT: Scenario = Scenario::Browser { tool: "browser.screenshot", arguments: "{}" };

/// The fixture's profile, told whether its model takes images.
async fn images(runtime: &crate::RuntimeState, model: ModelProfileId, on: bool) {
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

/// The model's screenshot call, on a profile that takes images or not.
struct Shot {
    _host: crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
    session: SessionId,
    run: AgentRunId,
    requests: Vec<serde_json::Value>,
}

async fn shoot(takes_images: bool) -> Vec<serde_json::Value> {
    shot(takes_images).await.requests
}

async fn shot(takes_images: bool) -> Shot {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(SHOOT).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    images(&runtime, model, takes_images).await;
    services(&runtime).agent_required().browser().note_page(session, "http://127.0.0.1:9/game");
    let (seen, _) = play_host(&runtime, Host::Answers);
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    waiting::wait(&runtime, &request, completed).await;
    assert!(
        seen.lock()
            .expect("seen")
            .iter()
            .any(|command| matches!(command.action, BrowserHostActionV1::Screenshot { .. }))
    );
    let requests = provider.requests.lock().expect("requests").clone();
    Shot { _host: host, runtime, session, run: started.run.agent_run_id, requests }
}

fn messages(request: &Value) -> Vec<serde_json::Value> {
    request["messages"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn a_model_that_takes_images_is_sent_the_screenshot_after_its_result() {
    let requests = shoot(true).await;
    let after = requests.last().expect("the request after the screenshot");
    let messages = messages(after);
    let at = messages.iter().position(|m| m["role"] == "tool").expect("the screenshot's result");
    let result = messages[at]["content"].as_str().expect("result text");
    assert!(result.contains("\"shown_to_model\":true"), "{result}");
    let image = &messages[at + 1];
    assert_eq!(image["role"], "user", "the image follows the result");
    assert!(
        image["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("browser.screenshot"))
    );
    assert_eq!(
        image["content"][1]["image_url"]["url"],
        json!(format!("data:image/jpeg;base64,{JPEG_BASE64}"))
    );
    // The first request had no screenshot to carry.
    assert!(!requests[0].to_string().contains("image_url"));
}

#[tokio::test]
async fn a_model_that_does_not_is_told_so_and_sent_no_image() {
    let requests = shoot(false).await;
    let after = requests.last().expect("the request after the screenshot");
    assert!(!after.to_string().contains("image_url"), "{after}");
    let messages = messages(after);
    let result = messages
        .iter()
        .find(|m| m["role"] == "tool")
        .and_then(|m| m["content"].as_str())
        .expect("the screenshot's result");
    assert!(result.contains("\"shown_to_model\":false") && result.contains("browser.snapshot"));
}

#[tokio::test]
async fn the_conversation_reads_its_own_screenshot_and_no_other_does() {
    let shot = shot(false).await;
    let result = messages(shot.requests.last().expect("the request after the screenshot"))
        .iter()
        .find(|m| m["role"] == "tool")
        .and_then(|m| m["content"].as_str().map(str::to_owned))
        .expect("the screenshot's result");
    let key = "\"image_artifact_id\":\"";
    let at = result.find(key).expect("the screenshot's id") + key.len();
    let id = &result[at..at + 30];
    let image = crate::r03_tests::success(
        &shot.runtime,
        "browser.screenshot.read",
        json!({"session_id": shot.session, "artifact_id": id}),
    )
    .await;
    assert_eq!(image["media_type"], "image/jpeg");
    assert_eq!(image["data_base64"], JPEG_BASE64);
    let elsewhere = crate::r03_tests::request(
        &shot.runtime,
        "browser.screenshot.read",
        json!({"session_id": SessionId::new(), "artifact_id": id}),
    )
    .await;
    assert!(
        elsewhere["error"].to_string().contains("browser.screenshot_unavailable"),
        "{elsewhere}"
    );
}

/// A screenshot is one of the
/// run's own results, so the run may cite it as evidence.
#[tokio::test]
async fn the_run_that_took_a_screenshot_may_cite_it() {
    let shot = shot(false).await;
    let result = messages(shot.requests.last().expect("the request after the screenshot"))
        .iter()
        .find(|m| m["role"] == "tool")
        .and_then(|m| m["content"].as_str().map(str::to_owned))
        .expect("the screenshot's result");
    let key = "\"image_artifact_id\":\"";
    let at = result.find(key).expect("the screenshot's id") + key.len();
    let id: aura_domain_common::ArtifactId = result[at..at + 30].parse().expect("an artifact id");
    let store = services(&shot.runtime).broker.lock().expect("journal");
    aura_persistence_core::artifact::read(store.database.connection(), id, &shot.run.to_string())
        .expect("the run reads its own screenshot");
}
