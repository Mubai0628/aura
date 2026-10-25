use std::time::Duration;

use aura_model_domain::{ModelRetryClass, NormalizedModelFailure};

pub const MAX_TRANSPORT_ATTEMPTS: u32 = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetryDecision {
    DoNotRetry,
    RetryAfter(Duration),
    CompatibilityAdjustment { parameter: String },
}

#[derive(Clone, Copy, Debug)]
pub struct TransportRetryPolicy {
    base_backoff: Duration,
    max_backoff: Duration,
    max_retry_after: Duration,
    /// False while another connection waits for this request; a temporary failure goes
    /// there at once instead of being retried here.
    retry_temporary: bool,
}

impl TransportRetryPolicy {
    #[must_use]
    pub const fn new(
        base_backoff: Duration,
        max_backoff: Duration,
        max_retry_after: Duration,
    ) -> Self {
        Self { base_backoff, max_backoff, max_retry_after, retry_temporary: true }
    }

    /// The request has a backup connection, so a
    /// temporary failure is not retried on this one (nor waited out for its Retry-After).
    #[must_use]
    pub const fn without_temporary_retries(self) -> Self {
        Self { retry_temporary: false, ..self }
    }

    #[must_use]
    pub const fn jitter_limit(&self) -> Duration {
        self.base_backoff
    }

    #[must_use]
    pub fn decide(
        &self,
        attempt_number: u32,
        failure: &NormalizedModelFailure,
        emitted_valid_event: bool,
        cancelled: bool,
        endpoint_fingerprint_changed: bool,
        jitter: Duration,
    ) -> RetryDecision {
        if attempt_number >= MAX_TRANSPORT_ATTEMPTS
            || emitted_valid_event
            || cancelled
            || endpoint_fingerprint_changed
        {
            return RetryDecision::DoNotRetry;
        }
        match &failure.retry_class {
            ModelRetryClass::NotRetryable => RetryDecision::DoNotRetry,
            ModelRetryClass::CompatibilityAdjustment { parameter } => {
                RetryDecision::CompatibilityAdjustment { parameter: parameter.clone() }
            }
            ModelRetryClass::TransparentRetry { .. } if !self.retry_temporary => {
                RetryDecision::DoNotRetry
            }
            ModelRetryClass::TransparentRetry { retry_after_ms } => {
                let retry_after = retry_after_ms.map(Duration::from_millis);
                let exponent = attempt_number.saturating_sub(1).min(31);
                let multiplier = 1_u32 << exponent;
                let exponential = self
                    .base_backoff
                    .checked_mul(multiplier)
                    .unwrap_or(self.max_backoff)
                    .min(self.max_backoff);
                let delay = retry_after
                    .map(|duration| duration.min(self.max_retry_after))
                    .unwrap_or(exponential)
                    .saturating_add(jitter.min(self.base_backoff));
                RetryDecision::RetryAfter(delay)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use aura_model_domain::{ModelFailureCategory, ModelRetryClass, NormalizedModelFailure};

    use super::{RetryDecision, TransportRetryPolicy};

    fn retryable() -> NormalizedModelFailure {
        NormalizedModelFailure {
            category: ModelFailureCategory::Transport,
            code: "temporary".to_owned(),
            message: "temporary".to_owned(),
            retry_class: ModelRetryClass::TransparentRetry { retry_after_ms: None },
            provider_status: Some(503),
            provider_request_id: None,
            provider_error_ref: None,
            partial_output_ref: None,
            transport_attempt_id: None,
        }
    }

    #[test]
    fn never_retries_after_an_event_or_beyond_the_frozen_budget() {
        let policy = TransportRetryPolicy::new(
            Duration::from_millis(10),
            Duration::from_secs(1),
            Duration::from_secs(2),
        );
        assert_eq!(
            policy.decide(1, &retryable(), true, false, false, Duration::ZERO),
            RetryDecision::DoNotRetry
        );
        assert_eq!(
            policy.decide(3, &retryable(), false, false, false, Duration::ZERO),
            RetryDecision::DoNotRetry
        );
    }

    #[test]
    fn a_request_with_a_backup_connection_is_not_retried_for_a_temporary_failure() {
        let policy = TransportRetryPolicy::new(
            Duration::from_millis(10),
            Duration::from_secs(1),
            Duration::from_secs(2),
        );
        assert!(matches!(
            policy.decide(1, &retryable(), false, false, false, Duration::ZERO),
            RetryDecision::RetryAfter(_)
        ));
        let backed_up = policy.without_temporary_retries();
        assert_eq!(
            backed_up.decide(1, &retryable(), false, false, false, Duration::ZERO),
            RetryDecision::DoNotRetry
        );
        let mut adjustable = retryable();
        adjustable.retry_class =
            ModelRetryClass::CompatibilityAdjustment { parameter: "temperature".to_owned() };
        assert_eq!(
            backed_up.decide(1, &adjustable, false, false, false, Duration::ZERO),
            RetryDecision::CompatibilityAdjustment { parameter: "temperature".to_owned() },
            "a parameter the endpoint refuses is still adjusted on the same connection"
        );
    }
}
