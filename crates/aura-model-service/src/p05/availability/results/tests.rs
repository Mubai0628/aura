//! A Provider that refuses the probe by a setting of the user's account there, the region, the
//! use of request data or a plan's list of clients, accepted the key: the test says which
//! setting, as a session does, instead of that the key was refused. A refusal naming none is
//! still the key's.
use aura_domain_common::{
    EndpointId, ModelProfileId, NormalizedIssueCode, ProviderId, Revision, UtcTimestamp,
};
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelAvailabilityCheckNotRunReason, ModelAvailabilityCheckResult, ModelAvailabilityTestOutcome,
    ModelAvailabilityTestResult, ModelContextWindowConfig, ModelDiscoveryMode, ModelEndpointConfig,
    ModelEndpointTimeouts, ModelProfileConfig, ModelProfileOrigin, ModelRequestDefaults,
};

use super::{ProbeReply, availability_result_for_response};

fn endpoint_and_profile() -> (ModelEndpointConfig, ModelProfileConfig) {
    let endpoint_id = EndpointId::new();
    let profile_id = ModelProfileId::new();
    let now = UtcTimestamp::now();
    let endpoint = ModelEndpointConfig {
        id: endpoint_id,
        revision: Revision::INITIAL,
        provider_id: ProviderId::new(),
        display_name: "Fixture".to_owned(),
        slug: "fixture".parse().expect("valid Endpoint slug"),
        api_format: ModelApiFormat::OpenAiChatCompletions,
        base_url: "https://api.example.test/v1".to_owned(),
        discovery_mode: ModelDiscoveryMode::Disabled,
        chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
        chat_instruction_role: Some(ChatInstructionRole::System),
        auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        timeouts: ModelEndpointTimeouts {
            connect_ms: 5_000,
            first_event_ms: 10_000,
            stream_idle_ms: 10_000,
            total_ms: Some(30_000),
        },
        max_parallel_model_requests: 1,
        model_order: vec![profile_id],
        compatibility: None,
        created_at: now,
        updated_at: now,
    };
    let profile = ModelProfileConfig {
        id: profile_id,
        revision: Revision::INITIAL,
        endpoint_id,
        provider_model_name: "fixture-model".to_owned(),
        display_name: "Fixture model".to_owned(),
        notes: None,
        origin: ModelProfileOrigin::Manual,
        enabled: true,
        available_for_subagents: false,
        catalog: None,
        context: ModelContextWindowConfig {
            catalog_window_tokens: None,
            user_override_window_tokens: None,
            user_override_revision: 0,
            runtime_corrected_window_tokens: None,
            runtime_correction_basis: None,
            runtime_effective_window_tokens: 32_000,
            historical_snapshot_max_tokens: 1_024,
        },
        request_defaults: ModelRequestDefaults {
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            chat_output_token_field: None,
            chat_instruction_role: None,
            reasoning_effort: None,
            reasoning_efforts: None,
            image_input: None,
            allow_speed_modes: false,
            speed_mode: None,
        },
        created_at: now,
        updated_at: now,
    };
    (endpoint, profile)
}

fn tested(status: u16, body: &str) -> ModelAvailabilityTestResult {
    let (endpoint, profile) = endpoint_and_profile();
    availability_result_for_response(
        &endpoint,
        &profile,
        UtcTimestamp::now(),
        ProbeReply { status, body: body.as_bytes(), elapsed_ms: 10 },
    )
}

fn failed(code: &str, status: u16) -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::Failed {
        error_code: NormalizedIssueCode::new(code).expect("a registered code"),
        provider_status: Some(status),
    }
}

#[test]
fn a_setting_of_the_users_account_is_named_and_the_key_counts_as_accepted() {
    for (status, body, code) in [
        (
            403,
            r#"{"error":{"message":"This Go model requires Global regions. Select Global in your workspace's Privacy settings to use it."}}"#,
            "provider.account_region",
        ),
        (
            404,
            r#"{"error":{"message":"No endpoints found matching your data policy (Paid model training)","code":404}}"#,
            "provider.account_data_policy",
        ),
        (
            403,
            r#"{"error":{"type":"access_terminated_error","message":"Kimi For Coding is currently only available for Coding Agents such as Kimi CLI, Claude Code, Roo Code, Kilo Code, etc."}}"#,
            "provider.client_not_allowed",
        ),
    ] {
        let result = tested(status, body);
        assert_eq!(result.outcome, ModelAvailabilityTestOutcome::Unavailable, "{code}");
        assert_eq!(
            result.authentication,
            ModelAvailabilityCheckResult::Passed { latency_ms: None }
        );
        assert_eq!(result.model_generation, failed(code, status));
        let summary = result.provider_error.expect("the Provider's error");
        assert_eq!((summary.issue_code.as_str(), summary.provider_status), (code, Some(status)));
    }
}

#[test]
fn a_refusal_naming_no_setting_is_still_the_keys() {
    for status in [401, 403] {
        let result = tested(status, r#"{"error":{"message":"Invalid API key provided."}}"#);
        assert_eq!(result.authentication, failed("authentication.rejected", status));
        assert_eq!(
            result.model_generation,
            ModelAvailabilityCheckResult::NotRun {
                reason_code: ModelAvailabilityCheckNotRunReason::BlockedByPreviousStage,
            }
        );
    }
    let result = tested(400, r#"{"error":{"message":"Unknown parameter: foo"}}"#);
    assert_eq!(result.model_generation, failed("provider.request_rejected", 400));
}
