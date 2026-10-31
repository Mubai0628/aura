//! A request that failed before any answer — credentials refused, balance gone,
//! rate limited, the server failing, no connection — is asked again at once through the provider's
//! next backup connection, when this run's current streak has not tried it yet. Once an answer has
//! started, nothing switches: what was shown stays the model's.
use super::*;
use aura_model_domain::{ModelFailureCategory, ModelProfileConfig, NormalizedModelFailure};

pub(super) fn switches(failure: &NormalizedModelFailure) -> bool {
    if failure.partial_output_ref.is_some() {
        return false;
    }
    match failure.category {
        ModelFailureCategory::Authentication
        | ModelFailureCategory::Authorization
        | ModelFailureCategory::RateLimit
        | ModelFailureCategory::QuotaOrBalance
        | ModelFailureCategory::Transport => true,
        ModelFailureCategory::Protocol => {
            failure.provider_status.is_some_and(|status| status == 408 || status >= 500)
        }
        _ => false,
    }
}

impl AgentApplication {
    /// Whether the provider has a connection this run's current streak has not failed on yet,
    /// `failing` included.
    pub(super) async fn untried_connection(
        &self,
        run_id: AgentRunId,
        profile: &ModelProfileConfig,
        failing: aura_domain_common::EndpointId,
    ) -> anyhow::Result<bool> {
        let config = self.p05.model_config.snapshot()?;
        let order = aura_model_config::connection_order(&config.registry, profile);
        let mut failed = self
            .store
            .read(StoreStep::run(run_id, "agent.drive.failed_connections"), move |db| {
                aura_persistence_v2::model::transient_retry::failed_connections(db, run_id)
            })
            .await?;
        failed.push(failing);
        Ok(aura_model_config::next_connection(&order, &failed).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_model_domain::ModelRetryClass;

    fn failure(category: ModelFailureCategory, status: Option<u16>) -> NormalizedModelFailure {
        NormalizedModelFailure {
            category,
            code: "x".into(),
            message: "x".into(),
            retry_class: ModelRetryClass::NotRetryable,
            provider_status: status,
            provider_request_id: None,
            provider_error_ref: None,
            partial_output_ref: None,
            transport_attempt_id: None,
        }
    }

    #[test]
    fn a_failure_before_any_answer_switches_and_nothing_else_does() {
        for (category, status) in [
            (ModelFailureCategory::Authentication, Some(401)),
            (ModelFailureCategory::Authorization, Some(403)),
            (ModelFailureCategory::QuotaOrBalance, Some(402)),
            (ModelFailureCategory::RateLimit, Some(429)),
            (ModelFailureCategory::Transport, None),
            (ModelFailureCategory::Protocol, Some(503)),
            (ModelFailureCategory::Protocol, Some(408)),
        ] {
            assert!(switches(&failure(category, status)), "{category:?} {status:?}");
        }
        for (category, status) in [
            (ModelFailureCategory::InvalidRequest, Some(400)),
            (ModelFailureCategory::ContextWindowExceeded, Some(400)),
            (ModelFailureCategory::ContentFilter, None),
            (ModelFailureCategory::StreamInterrupted, None),
            (ModelFailureCategory::Protocol, Some(404)),
        ] {
            assert!(!switches(&failure(category, status)), "{category:?} {status:?}");
        }
        let mut started = failure(ModelFailureCategory::RateLimit, Some(429));
        started.partial_output_ref = Some(aura_domain_common::ArtifactId::new());
        assert!(!switches(&started), "an answer had started");
    }
}
