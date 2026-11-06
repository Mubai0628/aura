//! What an availability test found: the result of each check, from the probe's reply or from
//! the failure that kept it from one.
use aura_domain_common::{NormalizedIssueCode, UtcTimestamp};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    EndpointAuthScheme, ModelApiFormat, ModelAvailabilityCheckNotRunReason,
    ModelAvailabilityCheckResult, ModelAvailabilityTestOutcome, ModelAvailabilityTestResult,
    ModelEndpointConfig, ModelProfileConfig,
};
use aura_model_transport::{
    ModelAdapterError, ModelGenerationProbeProtocol, provider_reply_account_setting,
};
use aura_provider_profile::AccountSetting;

use super::super::{
    configuration_transport::{ConfigurationTransportFailure, ConfigurationTransportResponse},
    provider_error::summarize_provider_error,
};

pub(super) fn provider_status_from_result(result: &ModelAvailabilityTestResult) -> Option<u16> {
    match &result.model_generation {
        ModelAvailabilityCheckResult::Failed { provider_status, .. } => *provider_status,
        ModelAvailabilityCheckResult::Passed { .. }
        | ModelAvailabilityCheckResult::NotRun { .. } => None,
    }
}

pub(super) fn issue_code(value: &str) -> NormalizedIssueCode {
    NormalizedIssueCode::new(value).expect("P05 uses only registered normalized issue codes")
}

fn validate_success_body(api_format: ModelApiFormat, body: &[u8]) -> Result<(), ModelAdapterError> {
    match api_format {
        ModelApiFormat::OpenAiResponses => {
            OpenAiResponsesAdapter.validate_generation_probe_success_body(body)
        }
        ModelApiFormat::OpenAiChatCompletions => {
            OpenAiChatCompletionsAdapter.validate_generation_probe_success_body(body)
        }
        ModelApiFormat::AnthropicMessages => {
            AnthropicMessagesAdapter.validate_generation_probe_success_body(body)
        }
    }
}

/// What the probe got back over HTTP.
#[derive(Clone, Copy)]
pub(super) struct ProbeReply<'a> {
    pub(super) status: u16,
    pub(super) body: &'a [u8],
    pub(super) elapsed_ms: u64,
}

impl<'a> From<&'a ConfigurationTransportResponse> for ProbeReply<'a> {
    fn from(response: &'a ConfigurationTransportResponse) -> Self {
        Self { status: response.status, body: &response.body, elapsed_ms: response.elapsed_ms }
    }
}

pub(super) fn availability_result_for_response(
    endpoint: &ModelEndpointConfig,
    profile: &ModelProfileConfig,
    started_at: UtcTimestamp,
    reply: ProbeReply<'_>,
) -> ModelAvailabilityTestResult {
    let completed_at = UtcTimestamp::now();
    let passed = passed_check();
    let blocked = blocked_check();
    let tls = tls_passed_or_not_applicable(endpoint);
    let setting = (!(200..300).contains(&reply.status))
        .then(|| provider_reply_account_setting(reply.body))
        .flatten();
    let (authentication, model_generation) = match (setting, reply.status) {
        // The Provider knew the account: one of its settings refused the model.
        (Some(setting), status) => (
            authentication_passed_or_not_required(endpoint),
            failed_check(account_setting_issue(setting), Some(status)),
        ),
        (None, 401 | 403) => (failed_check("authentication.rejected", Some(reply.status)), blocked),
        (None, status) if !(200..300).contains(&status) => (
            ModelAvailabilityCheckResult::NotRun {
                reason_code: ModelAvailabilityCheckNotRunReason::AuthenticationNotConfirmed,
            },
            failed_check(provider_status_issue(status), Some(status)),
        ),
        (None, _) => {
            let authentication = authentication_passed_or_not_required(endpoint);
            let generation = validate_success_body(endpoint.api_format, reply.body).map_or_else(
                |error| failed_check(adapter_failure_issue(&error), Some(reply.status)),
                |()| passed_check_with_latency(reply.elapsed_ms),
            );
            (authentication, generation)
        }
    };
    let mut result = result(
        endpoint,
        profile,
        started_at,
        completed_at,
        AvailabilityChecks {
            config: passed.clone(),
            network: passed.clone(),
            tls,
            http: passed,
            authentication,
            model_generation,
        },
        Vec::new(),
    );
    if !(200..300).contains(&reply.status) {
        let issue =
            setting.map_or_else(|| provider_status_issue(reply.status), account_setting_issue);
        result.provider_error =
            Some(summarize_provider_error(issue_code(issue), Some(reply.status), Some(reply.body)));
    }
    result
}

pub(super) fn availability_result_for_transport_failure(
    endpoint: &ModelEndpointConfig,
    profile: &ModelProfileConfig,
    started_at: UtcTimestamp,
    failure: &ConfigurationTransportFailure,
) -> ModelAvailabilityTestResult {
    let completed_at = UtcTimestamp::now();
    let blocked = blocked_check();
    let issue = failure.issue_code.as_str();
    let provider_status = failure.failure.failure.provider_status;

    let checks = if failure.http_response_received {
        let status = provider_status.expect("observed HTTP failure records provider status");
        let (authentication, model_generation) = match status {
            401 | 403 => (failed_check("authentication.rejected", Some(status)), blocked.clone()),
            status if !(200..300).contains(&status) => (
                ModelAvailabilityCheckResult::NotRun {
                    reason_code: ModelAvailabilityCheckNotRunReason::AuthenticationNotConfirmed,
                },
                failed_check(provider_status_issue(status), Some(status)),
            ),
            _ => {
                (authentication_passed_or_not_required(endpoint), failed_check(issue, Some(status)))
            }
        };
        (
            passed_check(),
            passed_check(),
            tls_passed_or_not_applicable(endpoint),
            passed_check(),
            authentication,
            model_generation,
        )
    } else if issue.starts_with("credential.")
        || issue.starts_with("configuration.")
        || issue == "internal.unexpected"
    {
        (
            failed_check(issue, None),
            blocked.clone(),
            blocked.clone(),
            blocked.clone(),
            blocked.clone(),
            blocked,
        )
    } else if issue.starts_with("tls.") {
        (
            passed_check(),
            passed_check(),
            failed_check(issue, None),
            blocked.clone(),
            blocked.clone(),
            blocked,
        )
    } else if issue == "network.redirect_blocked" {
        (
            passed_check(),
            passed_check(),
            tls_passed_or_not_applicable(endpoint),
            failed_check(issue, None),
            blocked.clone(),
            blocked,
        )
    } else {
        (
            passed_check(),
            failed_check(issue, None),
            blocked.clone(),
            blocked.clone(),
            blocked.clone(),
            blocked,
        )
    };
    let mut result = result(
        endpoint,
        profile,
        started_at,
        completed_at,
        AvailabilityChecks {
            config: checks.0,
            network: checks.1,
            tls: checks.2,
            http: checks.3,
            authentication: checks.4,
            model_generation: checks.5,
        },
        Vec::new(),
    );
    if failure.http_response_received {
        result.provider_error =
            Some(summarize_provider_error(failure.issue_code.clone(), provider_status, None));
    }
    result
}

pub(super) struct AvailabilityChecks {
    pub(super) config: ModelAvailabilityCheckResult,
    pub(super) network: ModelAvailabilityCheckResult,
    pub(super) tls: ModelAvailabilityCheckResult,
    pub(super) http: ModelAvailabilityCheckResult,
    pub(super) authentication: ModelAvailabilityCheckResult,
    pub(super) model_generation: ModelAvailabilityCheckResult,
}

pub(super) fn result(
    endpoint: &ModelEndpointConfig,
    profile: &ModelProfileConfig,
    started_at: UtcTimestamp,
    completed_at: UtcTimestamp,
    checks: AvailabilityChecks,
    warnings: Vec<NormalizedIssueCode>,
) -> ModelAvailabilityTestResult {
    let outcome = if checks.model_generation.is_passed() {
        ModelAvailabilityTestOutcome::Available
    } else {
        ModelAvailabilityTestOutcome::Unavailable
    };
    ModelAvailabilityTestResult {
        outcome,
        config: checks.config,
        network: checks.network,
        tls: checks.tls,
        http: checks.http,
        authentication: checks.authentication,
        model_generation: checks.model_generation,
        tested_endpoint_id: endpoint.id,
        tested_model_profile_id: profile.id,
        tested_model_name: profile.provider_model_name.clone(),
        started_at,
        completed_at,
        warnings,
        provider_error: None,
    }
}

fn passed_check() -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::Passed { latency_ms: None }
}

fn passed_check_with_latency(latency_ms: u64) -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::Passed { latency_ms: Some(latency_ms) }
}

pub(super) fn blocked_check() -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::NotRun {
        reason_code: ModelAvailabilityCheckNotRunReason::BlockedByPreviousStage,
    }
}

pub(super) fn failed_check(
    code: &str,
    provider_status: Option<u16>,
) -> ModelAvailabilityCheckResult {
    ModelAvailabilityCheckResult::Failed {
        error_code: NormalizedIssueCode::new(code)
            .expect("P05 availability uses only registered normalized issue codes"),
        provider_status,
    }
}

fn tls_passed_or_not_applicable(endpoint: &ModelEndpointConfig) -> ModelAvailabilityCheckResult {
    if endpoint.base_url.starts_with("http://") {
        ModelAvailabilityCheckResult::NotRun {
            reason_code: ModelAvailabilityCheckNotRunReason::NotApplicableForScheme,
        }
    } else {
        passed_check()
    }
}

fn authentication_passed_or_not_required(
    endpoint: &ModelEndpointConfig,
) -> ModelAvailabilityCheckResult {
    if endpoint.auth_scheme == EndpointAuthScheme::None {
        ModelAvailabilityCheckResult::NotRun {
            reason_code: ModelAvailabilityCheckNotRunReason::NoAuthenticationRequired,
        }
    } else {
        passed_check()
    }
}

/// The check code a setting of the user's account at the Provider is reported under.
const fn account_setting_issue(setting: AccountSetting) -> &'static str {
    match setting {
        AccountSetting::Region => "provider.account_region",
        AccountSetting::DataPolicy => "provider.account_data_policy",
        AccountSetting::ClientNotAllowed => "provider.client_not_allowed",
    }
}

pub(super) const fn provider_status_issue(status: u16) -> &'static str {
    if status == 429 {
        "provider.rate_limited"
    } else if status >= 500 {
        "provider.server_error"
    } else {
        "provider.request_rejected"
    }
}

fn adapter_failure_issue(error: &ModelAdapterError) -> &'static str {
    if error.code.ends_with("visible_text_missing") {
        "provider.visible_text_missing"
    } else {
        "provider.invalid_response"
    }
}

#[cfg(test)]
mod tests;
