//! Which provider failures pass on their own, and how long a run waits before it asks
//! the failed step again (`aura_persistence_v2::model::transient_retry`). Decided by the
//! transport's own fields, never by message text.
use std::time::Duration;

use aura_domain_common::{AgentRunId, ModelStepId};
use aura_model_domain::{ModelFailureCategory, ModelRetryClass, NormalizedModelFailure};
use aura_model_transport::ModelCancellationToken;

/// Steps asked again in a row before the turn stops and says why.
pub(super) const MAX_RETRIES: u32 = 2;

/// Steps a sub-agent asks again in a row after its provider limited the rate: several
/// sub-agents send to the same provider at once, and none of them has a user watching it.
pub(super) const RATE_LIMIT_RETRIES: u32 = 4;

/// The longest a provider's own Retry-After is waited out.
const RETRY_AFTER_MAX: Duration = Duration::from_secs(300);

/// Codes a provider or its gateway sends for a failure that passes on its own.
const PASSING: &[&str] = &[
    "server_is_overloaded",
    "overloaded_error",
    "service_unavailable",
    "server_error",
    "internal_server_error",
    "request_timeout",
];

/// Whether the provider failed the step for a reason that passes on its own.
pub(super) fn passing(failure: &NormalizedModelFailure) -> bool {
    match failure.category {
        // Only an incomplete stream without durable partial output may be asked for again.
        ModelFailureCategory::StreamInterrupted => failure.partial_output_ref.is_none(),
        ModelFailureCategory::Protocol => {
            PASSING.contains(&failure.code.as_str())
                || failure.provider_status.is_some_and(|status| status == 408 || status >= 500)
        }
        _ => false,
    }
}

/// Whether the provider refused the step for its rate of requests (not for quota or balance).
pub(super) fn rate_limited(failure: &NormalizedModelFailure) -> bool {
    failure.category == ModelFailureCategory::RateLimit
}

/// How many steps in a row the run asks again after `failure`; none when it does not.
pub(super) fn retries(failure: &NormalizedModelFailure, sub_agent: bool) -> Option<u32> {
    if passing(failure) {
        return Some(MAX_RETRIES);
    }
    (sub_agent && rate_limited(failure)).then_some(RATE_LIMIT_RETRIES)
}

/// Waits before asking again after `streak` earlier retries; a stop ends the wait at once. A
/// rate limit is waited out longer each time, or for the provider's own Retry-After.
pub(super) async fn wait(
    streak: u32,
    failure: &NormalizedModelFailure,
    cancel: &ModelCancellationToken,
) {
    let delay = if cfg!(any(test, feature = "test-hooks")) {
        Duration::from_millis(50)
    } else if rate_limited(failure) {
        let schedule = Duration::from_secs(15 << streak.min(3));
        match failure.retry_class {
            ModelRetryClass::TransparentRetry { retry_after_ms: Some(after) } => {
                Duration::from_millis(after).min(RETRY_AFTER_MAX).max(schedule)
            }
            _ => schedule,
        }
    } else {
        Duration::from_secs(if streak == 0 { 5 } else { 20 })
    };
    tokio::select! {
        () = tokio::time::sleep(delay) => {}
        () = cancel.cancelled() => {}
    }
}

/// One line in Core's log for each step asked again: the conversation keeps only the answer that
/// came after it, so this line is what tells later which failure it was. The provider's request id
/// matches the gateway's own records; nothing the model or the provider wrote is in the line.
/// `streak` is how many steps in a row were asked again before this one, or none when the step
/// goes to a backup connection instead.
pub(super) fn log_asked_again(
    run_id: AgentRunId,
    step: ModelStepId,
    failure: &NormalizedModelFailure,
    streak: Option<u32>,
) {
    aura_core_base::append_core_log(
        "model.step_asked_again",
        serde_json::json!({
            "agent_run_id": run_id,
            "model_step_id": step,
            "code": failure.code,
            "category": failure.category,
            "provider_status": failure.provider_status,
            "provider_request_id": failure.provider_request_id,
            "transport_attempt_id": failure.transport_attempt_id,
            "streak": streak,
            "backup_connection": streak.is_none(),
        }),
    );
}
