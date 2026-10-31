//! Which provider failures pass on their own, and how long a run waits before it asks
//! the failed step again (`aura_persistence_v2::model::transient_retry`). Decided by the
//! transport's own fields, never by message text.
use std::time::Duration;

use aura_model_domain::{ModelFailureCategory, NormalizedModelFailure};
use aura_model_transport::ModelCancellationToken;

/// Steps asked again in a row before the turn stops and says why.
pub(super) const MAX_RETRIES: u32 = 2;

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
        // The stream broke after it began; nothing of it was committed.
        ModelFailureCategory::StreamInterrupted => true,
        ModelFailureCategory::Protocol => {
            PASSING.contains(&failure.code.as_str())
                || failure.provider_status.is_some_and(|status| status == 408 || status >= 500)
        }
        _ => false,
    }
}

/// Waits before asking again after `streak` earlier retries; a stop ends the wait at once.
pub(super) async fn wait(streak: u32, cancel: &ModelCancellationToken) {
    let seconds = if streak == 0 { 5 } else { 20 };
    let delay = if cfg!(any(test, feature = "test-hooks")) {
        Duration::from_millis(50)
    } else {
        Duration::from_secs(seconds)
    };
    tokio::select! {
        () = tokio::time::sleep(delay) => {}
        () = cancel.cancelled() => {}
    }
}
