#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use aura_domain_common::{ApprovalRequestId, ContentHash, CorrelationId, UtcTimestamp};
use aura_permission_domain::{
    ApprovalRequest, ApprovalRequestDraft, ApprovalResolution, ResolveApprovalInput,
    SecuritySource, TrustFingerprintInput,
};
use aura_persistence::{PersistenceCoordinator, PersistenceError};
use aura_security_contract::TrustState;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApprovalError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("approval drafts cannot be aggregated because their security boundaries differ")]
    IncompatibleAggregation,
    #[error("failed to serialize trust fingerprint input: {0}")]
    FingerprintSerialization(#[from] serde_json::Error),
}

#[allow(async_fn_in_trait)]
pub trait ApprovalCoordinator {
    async fn request(
        &self,
        draft: ApprovalRequestDraft,
    ) -> Result<ApprovalRequestId, ApprovalError>;

    async fn resolve(
        &self,
        input: ResolveApprovalInput,
    ) -> Result<ApprovalResolution, ApprovalError>;
}

#[derive(Clone)]
pub struct PersistentApprovalCoordinator {
    persistence: PersistenceCoordinator,
}

impl PersistentApprovalCoordinator {
    #[must_use]
    pub const fn new(persistence: PersistenceCoordinator) -> Self {
        Self { persistence }
    }

    pub fn list_pending(&self) -> Result<Vec<aura_permission_domain::ApprovalView>, ApprovalError> {
        Ok(self.persistence.pending_approvals()?)
    }

    pub fn details(
        &self,
        approval_id: ApprovalRequestId,
    ) -> Result<ApprovalRequest, ApprovalError> {
        Ok(self.persistence.approval_details(approval_id)?)
    }
}

impl ApprovalCoordinator for PersistentApprovalCoordinator {
    async fn request(
        &self,
        draft: ApprovalRequestDraft,
    ) -> Result<ApprovalRequestId, ApprovalError> {
        let correlation_id = draft.identity.correlation_id;
        Ok(self.persistence.create_approval_request(draft, correlation_id)?.id)
    }

    async fn resolve(
        &self,
        input: ResolveApprovalInput,
    ) -> Result<ApprovalResolution, ApprovalError> {
        Ok(self
            .persistence
            .resolve_approval(
                input.approval_id,
                input.expected_request_revision,
                input.decision,
                CorrelationId::new(),
            )?
            .resolution)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TrustFingerprintService;

impl TrustFingerprintService {
    pub fn fingerprint(&self, input: &TrustFingerprintInput) -> Result<ContentHash, ApprovalError> {
        let mut canonical = serde_json::to_value(input)?;
        strip_presentation_fields(&mut canonical);
        canonicalize(&mut canonical);
        Ok(ContentHash::blake3(&serde_json::to_vec(&canonical)?))
    }

    #[must_use]
    pub fn effective_trust(
        &self,
        source: &SecuritySource,
        configured_state: &TrustState,
        current_fingerprint: &ContentHash,
    ) -> TrustState {
        if !source.security_source_ref().requires_trust() {
            return configured_state.clone();
        }
        match configured_state {
            TrustState::Trusted { fingerprint, trusted_at }
                if fingerprint == current_fingerprint =>
            {
                TrustState::Trusted { fingerprint: fingerprint.clone(), trusted_at: *trusted_at }
            }
            TrustState::SystemTrusted => TrustState::SystemTrusted,
            TrustState::Trusted { .. } | TrustState::Untrusted => TrustState::Untrusted,
            TrustState::Revoked => TrustState::Revoked,
        }
    }

    #[must_use]
    pub fn trusted(fingerprint: ContentHash) -> TrustState {
        TrustState::Trusted { fingerprint, trusted_at: UtcTimestamp::now() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ApprovalAggregator;

impl ApprovalAggregator {
    #[must_use]
    pub fn can_aggregate(left: &ApprovalRequestDraft, right: &ApprovalRequestDraft) -> bool {
        left.identity.workspace_id == right.identity.workspace_id
            && left.identity.session_id == right.identity.session_id
            && left.identity.turn_task_id == right.identity.turn_task_id
            && left.source == right.source
            && left.source_fingerprint == right.source_fingerprint
            && left.capabilities == right.capabilities
            && left.scopes == right.scopes
            && left.risk_flags == right.risk_flags
            && left.allowed_decisions == right.allowed_decisions
            && left.permission_snapshot_id == right.permission_snapshot_id
    }

    pub fn aggregate(
        mut left: ApprovalRequestDraft,
        right: ApprovalRequestDraft,
    ) -> Result<ApprovalRequestDraft, ApprovalError> {
        if !Self::can_aggregate(&left, &right) {
            return Err(ApprovalError::IncompatibleAggregation);
        }
        left.operation_ids.extend(right.operation_ids);
        left.operation_fingerprints.extend(right.operation_fingerprints);
        left.actual_operation_summary =
            format!("{}; {}", left.actual_operation_summary, right.actual_operation_summary);
        Ok(left)
    }
}

fn strip_presentation_fields(value: &mut Value) {
    const PRESENTATION_FIELDS: &[&str] =
        &["display_name", "description", "icon", "icon_url", "logo", "summary"];
    match value {
        Value::Object(object) => {
            for field in PRESENTATION_FIELDS {
                object.remove(*field);
            }
            for child in object.values_mut() {
                strip_presentation_fields(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_presentation_fields(child);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn canonicalize(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let mut sorted = BTreeMap::new();
            for (key, mut child) in std::mem::take(object) {
                canonicalize(&mut child);
                sorted.insert(key, child);
            }
            object.extend(sorted);
        }
        Value::Array(values) => {
            for child in values {
                canonicalize(child);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use aura_permission_domain::TrustFingerprintInput;
    use serde_json::json;

    use super::*;

    fn input(manifest: Value) -> TrustFingerprintInput {
        TrustFingerprintInput {
            source: SecuritySource::Plugin { plugin_id: "example".to_owned() },
            manifest,
            schema: None,
            runtime_entry: Some("plugin.exe".to_owned()),
            content_hashes: vec![ContentHash::blake3(b"runtime")],
            origin: None,
            capabilities: vec!["workspace.read".to_owned()],
            resource_policies: vec!["workspace-only".to_owned()],
        }
    }

    #[test]
    fn presentation_only_changes_do_not_reset_trust() {
        let service = TrustFingerprintService;
        let first = service
            .fingerprint(&input(json!({"entry":"plugin.exe","description":"one"})))
            .expect("fingerprint");
        let second = service
            .fingerprint(&input(json!({"description":"two","entry":"plugin.exe"})))
            .expect("fingerprint");
        assert_eq!(first, second);
    }

    #[test]
    fn runtime_or_capability_changes_reset_trust() {
        let service = TrustFingerprintService;
        let first =
            service.fingerprint(&input(json!({"entry":"plugin.exe"}))).expect("fingerprint");
        let mut changed = input(json!({"entry":"other.exe"}));
        changed.capabilities.push("network".to_owned());
        let second = service.fingerprint(&changed).expect("fingerprint");
        assert_ne!(first, second);
    }
}
