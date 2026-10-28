use super::*;

pub(super) async fn create_session(
    runtime: &crate::RuntimeState,
    workspace_id: WorkspaceId,
    mode: PermissionMode,
) -> SessionId {
    let created = crate::r03_tests::success(
        runtime,
        "session.create",
        json!({"workspace_id":workspace_id,"title":"Agent"}),
    )
    .await;
    let session: SessionId = serde_json::from_value(created["session_id"].clone())
        .expect("Agent test setup and persisted assertion");
    let state = crate::r03_tests::success(
        runtime,
        "permission.get_session_state",
        json!({"session_id":session}),
    )
    .await;
    crate::r03_tests::success(
        runtime,
        "permission.set_session_mode",
        json!({"session_id":session,"expected_revision":state["revision"],"mode":mode}),
    )
    .await;
    session
}

/// The model's requests no provider has counted go byte for byte, as a fixture sized in bytes
/// expects, instead of by character class.
pub(super) fn count_by_bytes(model: ModelProfileId) {
    crate::agent_application::estimators::UNMEASURED_BY_BYTES
        .lock()
        .expect("estimator choice")
        .insert(model);
}

/// The model's requests no provider has counted go by character class, as in the product, even
/// where its fixture was sized in bytes.
pub(super) fn count_by_character_class(model: ModelProfileId) {
    crate::agent_application::estimators::UNMEASURED_BY_BYTES
        .lock()
        .expect("estimator choice")
        .remove(&model);
}

pub(super) async fn profile(runtime: &crate::RuntimeState, url: &str) -> ModelProfileId {
    profile_for(runtime, url, ModelApiFormat::OpenAiChatCompletions).await
}

/// The same configured Provider speaking raw OpenAI Responses.
pub(super) async fn profile_responses(runtime: &crate::RuntimeState, url: &str) -> ModelProfileId {
    profile_for(runtime, url, ModelApiFormat::OpenAiResponses).await
}

pub(super) async fn profile_for(
    runtime: &crate::RuntimeState,
    url: &str,
    api_format: ModelApiFormat,
) -> ModelProfileId {
    profile_for_with(
        runtime,
        url,
        api_format,
        ModelEndpointTimeouts {
            connect_ms: 5000,
            first_event_ms: 10000,
            stream_idle_ms: 10000,
            total_ms: Some(30000),
        },
    )
    .await
}

/// The same configured Provider with endpoint budgets the caller chooses.
///
/// The fault cases need a **real** first-event timeout inside a test run, so the wait is the
/// endpoint's own configured budget rather than a test sleep. Nothing else about the profile
/// changes, and no product default is touched.
pub(super) async fn profile_for_with(
    runtime: &crate::RuntimeState,
    url: &str,
    api_format: ModelApiFormat,
    timeouts: ModelEndpointTimeouts,
) -> ModelProfileId {
    let success = crate::r03_tests::success;
    let chat = api_format == ModelApiFormat::OpenAiChatCompletions;
    let provider=success(runtime,"models.provider.create",json!({"expected_revision":"1","input":{"display_name":"Agent fixture","source":{"type":"custom","catalog_provider_id":null}}})).await;
    let endpoint = success(
        runtime,
        "models.endpoint.create",
        json!(ModelEndpointCreateRequest {
            expected_revision: serde_json::from_value(provider["revision"].clone())
                .expect("Agent test setup and persisted assertion"),
            provider_id: serde_json::from_value(provider["value"]["id"].clone())
                .expect("Agent test setup and persisted assertion"),
            display_name: "Configured local provider".into(),
            api_format,
            base_url: url.into(),
            discovery_mode: ModelDiscoveryMode::Disabled,
            chat_output_token_field: chat.then_some(ChatOutputTokenField::MaxTokens),
            chat_instruction_role: chat.then_some(ChatInstructionRole::System),
            auth_scheme: EndpointAuthScheme::None,
            openai_organization_id: None,
            openai_project_id: None,
            anthropic_workspace_id: None,
            timeouts,
            max_parallel_model_requests: 2,
        }),
    )
    .await;
    let model=success(runtime,"models.profile.create_manual",json!({"expected_revision":endpoint["revision"],"input":{"endpoint_id":endpoint["value"]["id"],"provider_model_name":"fixture","display_name":"Fixture","notes":null,"context_window_tokens":256000}})).await;
    let configured=success(runtime,"models.profile.update",json!({"expected_revision":model["revision"],"model_profile_id":model["value"]["id"],"display_name":"Fixture","notes":null,"enabled":true,"request_defaults":{"max_output_tokens":4096,"temperature":null,"top_p":null,"chat_output_token_field":null,"chat_instruction_role":null},"historical_snapshot_max_tokens":1024})).await;
    let model = serde_json::from_value(configured["value"]["id"].clone())
        .expect("Agent test setup and persisted assertion");
    if super::provider_fixture::sized_in_bytes(url) {
        count_by_bytes(model);
    }
    model
}
/// The same configured Provider as [`profile`], for a test that configures a **second** one on a
/// runtime that already has a profile: the settings revision is read from the live snapshot
/// instead of assuming the fixture still starts at revision 1.
pub(super) async fn profile_after_existing(
    runtime: &crate::RuntimeState,
    url: &str,
) -> ModelProfileId {
    let snapshot =
        crate::r03_tests::success(runtime, "models.settings.get_snapshot", json!({})).await;
    let revision = snapshot["models_revision"].clone();
    let provider = crate::r03_tests::success(
        runtime,
        "models.provider.create",
        json!({"expected_revision":revision,"input":{"display_name":"Second configured provider","source":{"type":"custom","catalog_provider_id":null}}}),
    )
    .await;
    let endpoint = crate::r03_tests::success(
        runtime,
        "models.endpoint.create",
        json!(ModelEndpointCreateRequest {
            expected_revision: serde_json::from_value(provider["revision"].clone())
                .expect("provider revision"),
            provider_id: serde_json::from_value(provider["value"]["id"].clone())
                .expect("provider id"),
            display_name: "Second configured endpoint".into(),
            api_format: ModelApiFormat::OpenAiChatCompletions,
            base_url: url.into(),
            discovery_mode: ModelDiscoveryMode::Disabled,
            chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
            chat_instruction_role: Some(ChatInstructionRole::System),
            auth_scheme: EndpointAuthScheme::None,
            openai_organization_id: None,
            openai_project_id: None,
            anthropic_workspace_id: None,
            timeouts: ModelEndpointTimeouts {
                connect_ms: 5000,
                first_event_ms: 10000,
                stream_idle_ms: 10000,
                total_ms: Some(30000)
            },
            max_parallel_model_requests: 2,
        }),
    )
    .await;
    let model = crate::r03_tests::success(
        runtime,
        "models.profile.create_manual",
        json!({"expected_revision":endpoint["revision"],"input":{"endpoint_id":endpoint["value"]["id"],"provider_model_name":"fixture","display_name":"Second fixture","notes":null,"context_window_tokens":256000}}),
    )
    .await;
    let configured = crate::r03_tests::success(
        runtime,
        "models.profile.update",
        json!({"expected_revision":model["revision"],"model_profile_id":model["value"]["id"],"display_name":"Second fixture","notes":null,"enabled":true,"request_defaults":{"max_output_tokens":4096,"temperature":null,"top_p":null,"chat_output_token_field":null,"chat_instruction_role":null},"historical_snapshot_max_tokens":1024}),
    )
    .await;
    let model =
        serde_json::from_value(configured["value"]["id"].clone()).expect("model profile id");
    if super::provider_fixture::sized_in_bytes(url) {
        count_by_bytes(model);
    }
    model
}
