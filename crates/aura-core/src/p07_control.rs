use std::collections::BTreeMap;

use aura_domain_common::{CorrelationId, ErrorCategory, PrincipalId};
use aura_persistence::{
    PersistenceCoordinator, PersistenceError, ToolActivityPageReadErrorV1,
    ToolCredentialCandidateQueryV1, tool_activity_visibility_context_hash_v1,
};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use aura_tool_contract::{ToolActivityPageRequestV1, ToolRegistryErrorCode};
use aura_tool_domain::ToolCredentialCandidatesRequestV1;
use aura_tool_projection::{ToolActivityPageV1, ToolCredentialCandidatesProjectionV1};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ToolReadControlServiceV1;

impl ToolReadControlServiceV1 {
    pub(crate) fn credential_candidates(
        self,
        persistence: &PersistenceCoordinator,
        params: &serde_json::Value,
        authenticated_owner: PrincipalId,
        correlation_id: CorrelationId,
    ) -> Result<ToolCredentialCandidatesProjectionV1, IpcErrorEnvelope> {
        let request = serde_json::from_value::<ToolCredentialCandidatesRequestV1>(params.clone())
            .map_err(|_| {
            safe_error(
                ToolRegistryErrorCode::SnapshotCorrupted,
                ErrorCategory::Validation,
                "The Tool Credential candidate query is invalid.",
                correlation_id,
            )
        })?;
        persistence
            .query_tool_credential_candidates_v1(ToolCredentialCandidateQueryV1 {
                authenticated_owner,
                tool_instance_id: request.tool_instance_id,
                credential_slot_key: request.credential_slot_key,
                placement: request.placement,
                workspace_id: request.workspace_id,
                session_id: request.session_id,
                expected_instance_revision: request.expected_instance_revision,
            })
            .map_err(|error| map_persistence_error(error, correlation_id))
    }

    pub(crate) fn activity_page(
        self,
        persistence: &PersistenceCoordinator,
        params: &serde_json::Value,
        correlation_id: CorrelationId,
    ) -> Result<ToolActivityPageV1, IpcErrorEnvelope> {
        let request =
            serde_json::from_value::<ToolActivityPageRequestV1>(params.clone()).map_err(|_| {
                safe_error(
                    ToolRegistryErrorCode::CursorUnavailable,
                    ErrorCategory::Validation,
                    "The Tool Activity cursor is invalid.",
                    correlation_id,
                )
            })?;
        let binding = persistence
            .resolve_tool_activity_cursor_v1(&request.cursor)
            .map_err(|error| map_persistence_error(error, correlation_id))?
            .ok_or_else(|| {
                safe_error(
                    ToolRegistryErrorCode::CursorUnavailable,
                    ErrorCategory::Conflict,
                    "The Tool Activity cursor is unavailable.",
                    correlation_id,
                )
            })?;

        // The authenticated Main/Core channel represents the current local
        // user. Re-read the authoritative Session list for every page instead
        // of treating possession of the opaque cursor as read authority.
        let session_is_visible = persistence
            .workspace_session_projection()
            .map_err(|error| map_persistence_error(error, correlation_id))?
            .sessions
            .iter()
            .any(|session| session.session_id == binding.session_id);
        if !session_is_visible {
            return Err(safe_error(
                ToolRegistryErrorCode::CursorMismatch,
                ErrorCategory::Permission,
                "The Tool Activity cursor does not match a readable Session.",
                correlation_id,
            ));
        }

        persistence
            .read_tool_activity_page_v1(
                &request.cursor,
                binding.session_id,
                &tool_activity_visibility_context_hash_v1(binding.session_id),
            )
            .map_err(|error| map_activity_error(error, correlation_id))
    }
}

fn map_activity_error(
    error: ToolActivityPageReadErrorV1,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    match error {
        ToolActivityPageReadErrorV1::CursorExpired => safe_error(
            ToolRegistryErrorCode::CursorExpired,
            ErrorCategory::Conflict,
            "The Tool Activity cursor expired because its historical boundary is unavailable.",
            correlation_id,
        ),
        ToolActivityPageReadErrorV1::CursorUnavailable => safe_error(
            ToolRegistryErrorCode::CursorUnavailable,
            ErrorCategory::Conflict,
            "The Tool Activity cursor is unavailable.",
            correlation_id,
        ),
        ToolActivityPageReadErrorV1::CursorMismatch => safe_error(
            ToolRegistryErrorCode::CursorMismatch,
            ErrorCategory::Permission,
            "The Tool Activity cursor does not match the authenticated Session visibility.",
            correlation_id,
        ),
        ToolActivityPageReadErrorV1::CursorCorrupted => safe_error(
            ToolRegistryErrorCode::SnapshotCorrupted,
            ErrorCategory::Internal,
            "The Tool Activity cursor failed integrity verification.",
            correlation_id,
        ),
        ToolActivityPageReadErrorV1::Persistence(error) => {
            map_persistence_error(error, correlation_id)
        }
    }
}

fn map_persistence_error(
    error: PersistenceError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    match error {
        PersistenceError::ToolActivityCursorExpired => safe_error(
            ToolRegistryErrorCode::CursorExpired,
            ErrorCategory::Conflict,
            "The Tool Activity cursor expired because its historical boundary is unavailable.",
            correlation_id,
        ),
        PersistenceError::ToolActivityCursorUnavailable => safe_error(
            ToolRegistryErrorCode::CursorUnavailable,
            ErrorCategory::Conflict,
            "The Tool Activity cursor is unavailable.",
            correlation_id,
        ),
        PersistenceError::ToolActivityCursorMismatch => safe_error(
            ToolRegistryErrorCode::CursorMismatch,
            ErrorCategory::Permission,
            "The Tool Activity cursor does not match the authenticated Session visibility.",
            correlation_id,
        ),
        _ => safe_error(
            ToolRegistryErrorCode::SnapshotCorrupted,
            ErrorCategory::Internal,
            "Tool Activity history could not be read safely.",
            correlation_id,
        ),
    }
}

fn safe_error(
    code: ToolRegistryErrorCode,
    category: ErrorCategory,
    message: &str,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: code.error_code(),
        category,
        message: message.to_owned(),
        retryable: false,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::RuntimeInstanceId;
    use aura_persistence::{PersistenceOptions, PersistenceStartup};
    use serde_json::json;

    use super::*;

    #[test]
    fn invalid_activity_cursor_is_renderer_safe() {
        let root = tempfile::tempdir().expect("temporary data root");
        let PersistenceStartup::Ready(persistence) =
            PersistenceCoordinator::start(PersistenceOptions::new(
                root.path().join("aura.db"),
                root.path().join("artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("persistence startup")
        else {
            panic!("fixture persistence must be ready")
        };
        let error = ToolReadControlServiceV1
            .activity_page(
                &persistence,
                &json!({ "cursor": "contains spaces" }),
                CorrelationId::new(),
            )
            .expect_err("invalid cursor must fail");
        assert_eq!(error.code.as_str(), ToolRegistryErrorCode::CursorUnavailable.as_str());
        assert!(error.details.is_empty());
    }
}
