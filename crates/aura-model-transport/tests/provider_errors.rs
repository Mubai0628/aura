//! A Provider's error reply means the same to every connected adapter: each reply below is
//! read by the Responses, Chat Completions and Anthropic Messages adapters alike, so a new
//! Provider's wording is added once, to the shared table, and not three times.
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, ModelExecutionSnapshotId, ModelProfileId,
    ModelStepId, PromptBundleSnapshotId, ProviderId, UtcTimestamp,
};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelExecutionSnapshot, ModelFailureCategory, ModelRetryClass, NormalizedModelFailure,
    ResolvedModelRequestOptions,
};
use aura_model_transport::{
    ModelAdapterError, ModelProtocolDecoder, ProviderErrorResponse, ProviderStreamFrame,
};
use serde_json::{Value, json};

use ModelFailureCategory::{
    Authentication, Authorization, ContentFilter, ContextWindowExceeded, InvalidRequest, Protocol,
    QuotaOrBalance, RateLimit, Transport, UnsupportedModel, UnsupportedParameter,
};

/// OpenCode Go, for a model its workspace's regions do not allow (seen by the owner).
const REGION: &str = "Upstream request failed: This Go model requires Global regions. Select Global in your workspace's Privacy settings to use it.";
/// OpenCode Go names the switch so in its documentation; the reply's own wording is not known.
const TRAINING: &str = "This Go model requires Allow paid endpoints that train on request data. Turn it on in your workspace's Privacy settings to use it.";
/// OpenRouter, when the account's privacy settings leave no endpoint for the model.
const DATA_POLICY: &str = "No endpoints found matching your data policy (Free model training). Configure: https://openrouter.ai/settings/privacy";
/// Kimi For Coding, to a client its plan does not list (HTTP 403 `access_terminated_error`).
const CLIENT: &str = "Kimi For Coding is currently only available for Coding Agents such as Kimi CLI, Claude Code, Roo Code, Kilo Code, etc.";

fn adapters() -> [(&'static str, ModelApiFormat, &'static dyn ModelProtocolDecoder); 3] {
    [
        ("responses", ModelApiFormat::OpenAiResponses, &OpenAiResponsesAdapter),
        ("chat", ModelApiFormat::OpenAiChatCompletions, &OpenAiChatCompletionsAdapter),
        ("anthropic", ModelApiFormat::AnthropicMessages, &AnthropicMessagesAdapter),
    ]
}

fn reply(status: u16, body: &Value) -> ProviderErrorResponse {
    reply_bytes(status, body.to_string().as_bytes())
}

fn reply_bytes(status: u16, body: &[u8]) -> ProviderErrorResponse {
    ProviderErrorResponse {
        status,
        headers: Vec::new(),
        body: body.to_vec(),
        provider_request_id: None,
        raw_error_artifact_id: None,
    }
}

fn failures(response: &ProviderErrorResponse) -> Vec<(&'static str, NormalizedModelFailure)> {
    adapters().map(|(name, _, adapter)| (name, adapter.classify_http_error(response))).into()
}

#[track_caller]
fn expect(response: &ProviderErrorResponse, category: ModelFailureCategory, sent_again: bool) {
    let body = String::from_utf8_lossy(&response.body).into_owned();
    for (name, failure) in failures(response) {
        assert_eq!(failure.category, category, "{name} {} {body}", response.status);
        assert_eq!(
            matches!(failure.retry_class, ModelRetryClass::TransparentRetry { .. }),
            sent_again,
            "{name} {} {body}",
            response.status
        );
        assert_eq!(failure.provider_status, Some(response.status), "{name}");
    }
}

#[test]
fn an_account_setting_is_its_own_kind_at_any_status_and_is_not_sent_again() {
    // A gateway reports the Provider's refusal under its own status, a 5xx among them:
    // sending the same request again cannot pass a setting of the user's account.
    let region = json!({"error": {"message": REGION}});
    for status in [400, 403, 404, 429, 500, 502, 503] {
        expect(&reply(status, &region), Authorization, false);
        for (name, failure) in failures(&reply(status, &region)) {
            assert_eq!(failure.code, "provider_account_region", "{name} {status}");
            assert_eq!(failure.message, REGION, "{name} {status}");
        }
    }
    for (status, body, code) in [
        (403, json!({"error": {"message": TRAINING}}), "provider_account_data_policy"),
        (
            404,
            json!({"error": {"message": DATA_POLICY, "code": 404}}),
            "provider_account_data_policy",
        ),
        (
            403,
            json!({"error": {"type": "access_terminated_error", "message": CLIENT}}),
            "provider_client_not_allowed",
        ),
        (
            500,
            json!({"error": {"code": "server_error", "message": CLIENT}}),
            "provider_client_not_allowed",
        ),
    ] {
        expect(&reply(status, &body), Authorization, false);
        for (name, failure) in failures(&reply(status, &body)) {
            assert_eq!(failure.code, code, "{name} {status} {body}");
        }
    }
}

#[test]
fn the_status_decides_before_the_words() {
    // A gateway's rewritten `server_error` under a 4xx is the request's own fault.
    let rewritten = json!({"error": {
        "code": "server_error",
        "type": "server_error",
        "message": "The server had an error while processing your request."
    }});
    expect(&reply(400, &rewritten), InvalidRequest, false);
    expect(&reply(401, &rewritten), Authentication, false);
    expect(&reply(403, &rewritten), Authorization, false);
    // A server failure is one whatever its words; 501 and 505 do not pass on their own.
    let long =
        json!({"error": {"message": "This model's maximum context length is 131072 tokens."}});
    for status in [500, 502, 503, 504, 507, 520, 522, 524, 529] {
        expect(&reply(status, &long), Transport, true);
    }
    expect(&reply(408, &json!({})), Transport, true);
    for status in [501, 505] {
        expect(&reply(status, &json!({})), InvalidRequest, false);
    }
    expect(
        &reply(402, &json!({"error": {"message": "Insufficient Balance"}})),
        QuotaOrBalance,
        false,
    );
    // Payment Required means it, whatever the reply says.
    expect(&reply(402, &json!({})), QuotaOrBalance, false);
    let rate = json!({"error": {"code": "rate_limit_exceeded", "message": "Rate limit reached for requests"}});
    expect(&reply(429, &rate), RateLimit, true);
    let quota = json!({"error": {"code": "insufficient_quota", "message": "You exceeded your current quota."}});
    expect(&reply(429, &quota), QuotaOrBalance, false);
    expect(&reply(403, &quota), QuotaOrBalance, false);
    // xAI, out of credits; Zhipu, out of balance; a plan's usage limit (Aura's own wording).
    for (status, body) in [
        (
            403,
            json!({"code": "permission denied", "error": "Your team has either used all available credits or reached its monthly spending limit."}),
        ),
        (429, json!({"error": {"code": "1113", "message": "余额不足或无可用资源包,请充值。"}})),
        (429, json!({"error": {"message": "You have reached the usage limit of your plan."}})),
    ] {
        expect(&reply(status, &body), QuotaOrBalance, false);
    }
}

#[test]
fn the_words_of_a_refused_request_name_its_kind() {
    for (body, category) in [
        // DeepSeek
        (
            json!({"error": {"type": "invalid_request_error", "message": "This model's maximum context length is 131072 tokens. However, you requested 140000 tokens."}}),
            ContextWindowExceeded,
        ),
        // Anthropic, for a Chat or Responses gateway in front of it too
        (
            json!({"type": "error", "error": {"type": "invalid_request_error", "message": "prompt is too long: 210000 tokens > 200000 maximum"}}),
            ContextWindowExceeded,
        ),
        (
            json!({"error": {"code": "context_length_exceeded", "message": "Input exceeds the context window of this model."}}),
            ContextWindowExceeded,
        ),
        (
            json!({"type": "error", "error": {"type": "invalid_request_error", "message": "Your credit balance is too low to access the Anthropic API."}}),
            QuotaOrBalance,
        ),
        // Google's compatible endpoint and xAI name a bad key under 400.
        (
            json!({"error": {"code": 400, "message": "API key not valid. Please pass a valid API key.", "status": "INVALID_ARGUMENT"}}),
            Authentication,
        ),
        (
            json!({"error": {"code": "invalid-argument", "message": "Incorrect API key provided: xai-****."}}),
            Authentication,
        ),
        (
            json!({"error": {"code": "content_filter", "message": "The request was rejected because it was considered high risk."}}),
            ContentFilter,
        ),
        (
            json!({"error": {"code": "content_policy_violation", "message": "Your request was rejected by the safety system."}}),
            ContentFilter,
        ),
        (
            json!({"error": {"code": "model_not_found", "type": "invalid_request_error", "message": "The model `fixture-x` does not exist."}}),
            UnsupportedModel,
        ),
        (
            json!({"error": {"code": "unsupported_parameter", "param": "top_p", "message": "Unsupported parameter: 'top_p' is not supported with this model."}}),
            UnsupportedParameter,
        ),
        (
            json!({"error": {"code": "invalid_value", "message": "Unsupported value: 'top_p' must be at most 1."}}),
            InvalidRequest,
        ),
        (json!({"error": {"message": "Invalid JSON in request body."}}), InvalidRequest),
    ] {
        expect(&reply(400, &body), category, false);
    }
}

#[test]
fn a_reply_is_read_in_the_layouts_providers_answer_with() {
    for body in [
        json!({"error": REGION}),
        json!({"message": REGION}),
        json!({"detail": REGION}),
        json!({"Message": REGION}),
        json!({"error": {"code": "forbidden", "detail": REGION}}),
        json!([{"error": {"code": 400, "message": REGION, "status": "FAILED_PRECONDITION"}}]),
    ] {
        for (name, failure) in failures(&reply(400, &body)) {
            assert_eq!(failure.code, "provider_account_region", "{name} {body}");
            assert_eq!(failure.message, REGION, "{name} {body}");
        }
    }
    // Each protocol keeps its own code field first.
    let both = json!({"error": {"code": "fixture_code", "type": "fixture_type", "message": "Fixture refusal."}});
    let codes: Vec<_> = failures(&reply(400, &both))
        .into_iter()
        .map(|(name, failure)| (name, failure.code))
        .collect();
    assert_eq!(
        codes,
        [
            ("responses", "fixture_code".to_owned()),
            ("chat", "fixture_code".to_owned()),
            ("anthropic", "fixture_type".to_owned()),
        ]
    );
    // A body that is not JSON keeps the protocol's own code and message; the status decides.
    for (name, failure) in failures(&reply_bytes(502, b"<html><body>Bad gateway</body></html>")) {
        assert_eq!(failure.category, Transport, "{name}");
        assert!(failure.code.ends_with("_http_error"), "{name} {}", failure.code);
        assert!(failure.message.ends_with("request failed"), "{name} {}", failure.message);
    }
    let mut limited = reply(
        429,
        &json!({"error": {"code": "rate_limit_exceeded", "message": "Rate limit reached."}}),
    );
    limited.headers.push(("Retry-After".to_owned(), "7".to_owned()));
    for (name, failure) in failures(&limited) {
        assert_eq!(
            failure.retry_class,
            ModelRetryClass::TransparentRetry { retry_after_ms: Some(7_000) },
            "{name}"
        );
    }
}

fn snapshot(api_format: ModelApiFormat) -> ModelExecutionSnapshot {
    let chat = api_format == ModelApiFormat::OpenAiChatCompletions;
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Fixture".to_owned(),
        endpoint_display_name: "fixture".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Fixture / fixture / fixture-model".to_owned(),
        api_format,
        chat_output_token_field: chat.then_some(ChatOutputTokenField::MaxTokens),
        chat_instruction_role: chat.then_some(ChatInstructionRole::System),
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.example.test/v1".to_owned(),
        resolved_request_url: "https://api.example.test/v1/fixture".to_owned(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        endpoint_config_revision: 1,
        model_config_revision: 1,
        runtime_effective_window_tokens: 256_000,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions {
            effective_max_output_tokens: 512,
            temperature: None,
            top_p: None,
            reasoning_effort: None,
            reasoning_by_effort: false,
            image_input: false,
            speed_mode: None,
        },
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: ContentHash::blake3(b"fixture-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn stream_error(
    adapter: &dyn ModelProtocolDecoder,
    api_format: ModelApiFormat,
    data: &Value,
) -> ModelAdapterError {
    let mut decoder = adapter.create_stream_decoder(&snapshot(api_format));
    decoder
        .push_frame(ProviderStreamFrame {
            data: data.to_string(),
            ..ProviderStreamFrame::default()
        })
        .expect_err("the Provider reported an error")
}

/// Each protocol's in-stream error frame, carrying `code` and `message`.
fn error_frames(code: &str, message: &str) -> [Value; 3] {
    [
        json!({"type": "error", "code": code, "message": message, "sequence_number": 2}),
        json!({"error": {"code": code, "message": message}}),
        json!({"type": "error", "error": {"type": code, "message": message}}),
    ]
}

#[test]
fn an_account_setting_reported_inside_a_stream_is_not_asked_again() {
    // `server_error` alone would have the step asked again; the words say it cannot pass.
    for ((name, format, adapter), frame) in
        adapters().into_iter().zip(error_frames("server_error", REGION))
    {
        let error = stream_error(adapter, format, &frame);
        assert_eq!(error.category, Authorization, "{name}");
        assert_eq!(error.code, "provider_account_region", "{name}");
        assert_eq!(error.message, REGION, "{name}");
    }
    // Any other error stays the protocol's, under the Provider's own code.
    for ((name, format, adapter), frame) in
        adapters().into_iter().zip(error_frames("overloaded_error", "Overloaded"))
    {
        let error = stream_error(adapter, format, &frame);
        assert_eq!((error.category, error.code.as_str()), (Protocol, "overloaded_error"), "{name}");
        assert_eq!(error.message, "Overloaded", "{name}");
    }
    // A Responses `error` event's own `type` names the event, not a code.
    let error = stream_error(
        &OpenAiResponsesAdapter,
        ModelApiFormat::OpenAiResponses,
        &json!({"type": "error", "code": null, "message": "Overloaded", "sequence_number": 2}),
    );
    assert_eq!((error.category, error.code.as_str()), (Protocol, "openai_responses_stream_error"));
}

#[test]
fn a_failed_responses_stream_keeps_the_providers_reason() {
    let failed = |error: Value| {
        json!({
            "type": "response.failed",
            "sequence_number": 3,
            "response": {"id": "resp_fixture", "status": "failed", "error": error}
        })
    };
    let error = stream_error(
        &OpenAiResponsesAdapter,
        ModelApiFormat::OpenAiResponses,
        &failed(
            json!({"code": "server_error", "message": "The server had an error processing your request."}),
        ),
    );
    assert_eq!((error.category, error.code.as_str()), (Protocol, "server_error"));
    assert_eq!(error.message, "The server had an error processing your request.");
    let error = stream_error(
        &OpenAiResponsesAdapter,
        ModelApiFormat::OpenAiResponses,
        &failed(json!({"code": "server_error", "message": REGION})),
    );
    assert_eq!((error.category, error.code.as_str()), (Authorization, "provider_account_region"));
    let error = stream_error(
        &OpenAiResponsesAdapter,
        ModelApiFormat::OpenAiResponses,
        &failed(Value::Null),
    );
    assert_eq!((error.category, error.code.as_str()), (Protocol, "openai_responses_stream_error"));
}
