//! Built-in tool settings commands on the V1 tool authority: the tool switch,
//! instance configuration, credential binding and pins. The P07 Draft,
//! package and long-operation commands are retired; packages live in `package_services`.
use std::collections::BTreeMap;

use aura_core_broker::broker::completion::DispatchOutcome;
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{
    CommandInvocationId, ContentHash, CorrelationId, ErrorCategory, ErrorCode, Revision,
    UtcTimestamp,
};
use aura_network_control::{NetworkControlError, SecretScanner};
use aura_persistence::{
    PersistenceCoordinator, PersistenceError, PrepareToolMutationCommandV1,
    SessionToolPinChangeRequest, ToolDefinitionEnabledChangeRequestV1,
    ToolInstanceConfigurationSettingsChangeRequestV1, ToolInstanceCredentialClearRequestV1,
    ToolInstanceCredentialSettingsChangeRequestV1, ToolMutationCommandJournalRecordV1,
    ToolMutationCommandOutcomeV1, ToolPinTargetV1, WorkspaceToolPinChangeRequest,
};
use aura_projection_contract::ProjectionChange;
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use aura_tool_contract::{ToolDraftErrorCode, ToolRegistryErrorCode, ToolRuntimeErrorCode};
use aura_tool_domain::{
    SessionToolPinClearRequestV1, SessionToolPinSetRequestV1,
    TOOL_INSTANCE_BIND_CREDENTIAL_METHOD_V1, TOOL_INSTANCE_CLEAR_CREDENTIAL_METHOD_V1,
    TOOL_INSTANCE_UPDATE_CONFIGURATION_METHOD_V1, TOOL_PIN_SESSION_CLEAR_METHOD_V1,
    TOOL_PIN_SESSION_SET_METHOD_V1, TOOL_PIN_WORKSPACE_CLEAR_METHOD_V1,
    TOOL_PIN_WORKSPACE_SET_METHOD_V1, TOOL_SET_ENABLED_METHOD_V1,
    ToolInstanceBindCredentialRequestV1, ToolInstanceClearCredentialRequestV1,
    ToolInstanceUpdateConfigurationRequestV1, ToolMutationResultV1, ToolSetEnabledRequestV1,
    WorkspaceToolPinClearRequestV1, WorkspaceToolPinSetRequestV1,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

pub struct ToolMutationDispatchV1 {
    pub value: Value,
    pub projection_changes: Vec<ProjectionChange>,
}

impl DispatchOutcome for crate::p07_commands::ToolMutationDispatchV1 {
    fn reply_value(&self) -> Option<serde_json::Value> {
        Some(self.value.clone())
    }
}

struct ToolMutationEffectV1 {
    result: ToolMutationResultV1,
    projection_changes: Vec<ProjectionChange>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ToolMutationControlServiceV1;

impl ToolMutationControlServiceV1 {
    #[must_use]
    pub fn handles_main_command(method_name: &str) -> bool {
        aura_ipc::routes::owned_by(method_name, aura_ipc::routes::RouteOwner::Tool)
            && aura_ipc::routes::side_effecting(method_name)
    }

    pub fn dispatch_main_command(
        self,
        persistence: &PersistenceCoordinator,
        method_name: &str,
        params: &Value,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        authenticated_identity_hash: ContentHash,
    ) -> Result<ToolMutationDispatchV1, IpcErrorEnvelope> {
        let prepared = persistence
            .prepare_tool_mutation_command_v1(PrepareToolMutationCommandV1 {
                command_invocation_id,
                method_name: method_name.to_owned(),
                request: params.clone(),
                authenticated_identity_hash,
                correlation_id,
                prepared_at: UtcTimestamp::now(),
            })
            .map_err(|error| map_command_journal_error(error, correlation_id))?;
        let journal = prepared.record().clone();
        if let Some(outcome) = journal.outcome.clone() {
            return dispatch_from_outcome(outcome);
        }

        let effect = self.execute(persistence, method_name, &journal);
        let outcome = match &effect {
            Ok(effect) => ToolMutationCommandOutcomeV1::Succeeded { result: effect.result.clone() },
            Err(error) => ToolMutationCommandOutcomeV1::Failed { error: error.clone() },
        };
        persistence
            .complete_tool_mutation_command_v1(
                command_invocation_id,
                journal.request_hash.clone(),
                outcome,
                UtcTimestamp::now(),
            )
            .map_err(|error| map_command_journal_error(error, correlation_id))?;

        match effect {
            Ok(effect) => Ok(ToolMutationDispatchV1 {
                value: serde_json::to_value(effect.result)
                    .map_err(|_| journal_failed(correlation_id))?,
                projection_changes: effect.projection_changes,
            }),
            Err(error) => Err(error),
        }
    }

    fn execute(
        self,
        persistence: &PersistenceCoordinator,
        method_name: &str,
        journal: &ToolMutationCommandJournalRecordV1,
    ) -> Result<ToolMutationEffectV1, IpcErrorEnvelope> {
        let correlation_id = journal.seed.original_correlation_id;
        match method_name {
            TOOL_SET_ENABLED_METHOD_V1 => {
                let request: ToolSetEnabledRequestV1 = decode(&journal.request, correlation_id)?;
                let commit = persistence
                    .commit_tool_definition_enabled_change(ToolDefinitionEnabledChangeRequestV1 {
                        tool_id: request.tool_id,
                        expected_entity_revision: request.expected_entity_revision,
                        enabled: request.enabled,
                        changed_at: journal.seed.issued_at,
                        correlation_id,
                    })
                    .map_err(|error| map_persistence_error(error, correlation_id))?;
                Ok(ToolMutationEffectV1 {
                    result: ToolMutationResultV1::ToolCommitted {
                        tool_id: request.tool_id,
                        entity_revision: commit.target_entity_revision,
                    },
                    projection_changes: commit.projection_changes,
                })
            }
            TOOL_INSTANCE_UPDATE_CONFIGURATION_METHOD_V1 => {
                let request: ToolInstanceUpdateConfigurationRequestV1 =
                    decode(&journal.request, correlation_id)?;
                update_instance_configuration(persistence, request, journal, correlation_id)
            }
            TOOL_INSTANCE_BIND_CREDENTIAL_METHOD_V1 => {
                let request: ToolInstanceBindCredentialRequestV1 =
                    decode(&journal.request, correlation_id)?;
                let authenticated_owner = persistence
                    .current_tool_principal()
                    .map_err(|error| map_persistence_error(error, correlation_id))?;
                let commit = persistence
                    .commit_tool_instance_credential_settings_change(
                        ToolInstanceCredentialSettingsChangeRequestV1 {
                            authenticated_owner,
                            tool_instance_id: request.tool_instance_id,
                            credential_slot_key: request.credential_slot_key.clone(),
                            placement: request.placement.clone(),
                            expected_entity_revision: request.expected_entity_revision,
                            expected_binding_revision: request.expected_binding_revision,
                            expected_candidate_set_revision: request
                                .expected_candidate_set_revision,
                            credential_id: request.credential_id,
                            credential_revision: request.credential_revision,
                            workspace_id: request.workspace_id,
                            session_id: request.session_id,
                            changed_at: journal.seed.issued_at,
                            correlation_id,
                        },
                    )
                    .map_err(|error| map_persistence_error(error, correlation_id))?;
                Ok(instance_effect(
                    request.tool_instance_id,
                    commit.target_entity_revision,
                    commit.projection_changes,
                ))
            }
            TOOL_INSTANCE_CLEAR_CREDENTIAL_METHOD_V1 => {
                let request: ToolInstanceClearCredentialRequestV1 =
                    decode(&journal.request, correlation_id)?;
                let commit = persistence
                    .commit_tool_instance_credential_clear(ToolInstanceCredentialClearRequestV1 {
                        tool_instance_id: request.tool_instance_id,
                        expected_entity_revision: request.expected_entity_revision,
                        expected_binding_revision: request.expected_binding_revision,
                        changed_at: journal.seed.issued_at,
                        correlation_id,
                    })
                    .map_err(|error| map_persistence_error(error, correlation_id))?;
                Ok(instance_effect(
                    request.tool_instance_id,
                    commit.target_entity_revision,
                    commit.projection_changes,
                ))
            }
            TOOL_PIN_WORKSPACE_SET_METHOD_V1 => {
                let request: WorkspaceToolPinSetRequestV1 =
                    decode(&journal.request, correlation_id)?;
                workspace_pin_set(persistence, request, journal.seed.issued_at, correlation_id)
            }
            TOOL_PIN_WORKSPACE_CLEAR_METHOD_V1 => {
                let request: WorkspaceToolPinClearRequestV1 =
                    decode(&journal.request, correlation_id)?;
                workspace_pin_clear(persistence, request, journal.seed.issued_at, correlation_id)
            }
            TOOL_PIN_SESSION_SET_METHOD_V1 => {
                let request: SessionToolPinSetRequestV1 = decode(&journal.request, correlation_id)?;
                session_pin_set(persistence, request, journal.seed.issued_at, correlation_id)
            }
            TOOL_PIN_SESSION_CLEAR_METHOD_V1 => {
                let request: SessionToolPinClearRequestV1 =
                    decode(&journal.request, correlation_id)?;
                session_pin_clear(persistence, request, journal.seed.issued_at, correlation_id)
            }
            _ => Err(safe_error(
                ToolRegistryErrorCode::SettingsInvalid.error_code(),
                ErrorCategory::Validation,
                "The Tool settings method is outside the closed mutation set.",
                correlation_id,
            )),
        }
    }
}

fn dispatch_from_outcome(
    outcome: ToolMutationCommandOutcomeV1,
) -> Result<ToolMutationDispatchV1, IpcErrorEnvelope> {
    match outcome {
        ToolMutationCommandOutcomeV1::Succeeded { result } => Ok(ToolMutationDispatchV1 {
            value: serde_json::to_value(result)
                .map_err(|_| journal_failed(CorrelationId::new()))?,
            projection_changes: Vec::new(),
        }),
        ToolMutationCommandOutcomeV1::Failed { error } => Err(error),
    }
}

fn decode<T: DeserializeOwned>(
    value: &Value,
    correlation_id: CorrelationId,
) -> Result<T, IpcErrorEnvelope> {
    serde_json::from_value(value.clone()).map_err(|_| {
        safe_error(
            ToolRegistryErrorCode::SettingsInvalid.error_code(),
            ErrorCategory::Validation,
            "The Tool settings request does not match its method contract.",
            correlation_id,
        )
    })
}

fn instance_effect(
    tool_instance_id: aura_domain_common::ToolInstanceId,
    entity_revision: Revision,
    projection_changes: Vec<ProjectionChange>,
) -> ToolMutationEffectV1 {
    ToolMutationEffectV1 {
        result: ToolMutationResultV1::InstanceCommitted { tool_instance_id, entity_revision },
        projection_changes,
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolInstanceConfigurationSecretScanEvidenceV1<'a> {
    kind: &'static str,
    version: u8,
    tool_instance_id: aura_domain_common::ToolInstanceId,
    schema_hash: &'a CanonicalHashV1,
    configuration_hash: &'a CanonicalHashV1,
    scanner: &'static str,
    result: &'static str,
}

fn update_instance_configuration(
    persistence: &PersistenceCoordinator,
    request: ToolInstanceUpdateConfigurationRequestV1,
    journal: &ToolMutationCommandJournalRecordV1,
    correlation_id: CorrelationId,
) -> Result<ToolMutationEffectV1, IpcErrorEnvelope> {
    SecretScanner
        .scan_json_value("tool.instance_configuration", &request.configuration)
        .map_err(|error| map_secret_scan_error(error, correlation_id))?;
    let configuration_hash = canonical_sha256(&request.configuration).map_err(|_| {
        safe_error(
            ToolRegistryErrorCode::SettingsInvalid.error_code(),
            ErrorCategory::Validation,
            "The Tool Instance configuration is not canonicalizable.",
            correlation_id,
        )
    })?;
    let secret_scan_evidence_hash =
        canonical_sha256(&ToolInstanceConfigurationSecretScanEvidenceV1 {
            kind: "aura.p07.tool_instance_configuration_secret_scan",
            version: 1,
            tool_instance_id: request.tool_instance_id,
            schema_hash: &request.expected_schema_hash,
            configuration_hash: &configuration_hash,
            scanner: "aura.p04.secret_scanner.v1",
            result: "clean",
        })
        .map_err(|_| journal_failed(correlation_id))?;
    let commit = persistence
        .commit_tool_instance_configuration_settings_change(
            ToolInstanceConfigurationSettingsChangeRequestV1 {
                tool_instance_id: request.tool_instance_id,
                expected_entity_revision: request.expected_entity_revision,
                expected_configuration_revision: request.expected_configuration_revision,
                expected_schema_hash: request.expected_schema_hash,
                configuration: request.configuration,
                secret_scan_evidence_hash,
                changed_at: journal.seed.issued_at,
                correlation_id,
            },
        )
        .map_err(|error| map_persistence_error(error, correlation_id))?;
    Ok(instance_effect(
        request.tool_instance_id,
        commit.target_entity_revision,
        commit.projection_changes,
    ))
}

fn workspace_pin_set(
    persistence: &PersistenceCoordinator,
    request: WorkspaceToolPinSetRequestV1,
    changed_at: UtcTimestamp,
    correlation_id: CorrelationId,
) -> Result<ToolMutationEffectV1, IpcErrorEnvelope> {
    let commit = persistence
        .commit_workspace_tool_pin_change(WorkspaceToolPinChangeRequest {
            workspace_id: request.workspace_id,
            tool_id: request.tool_id,
            expected_revision: request.expected_entity_revision,
            target: Some(ToolPinTargetV1 {
                tool_version: request.tool_version,
                state: aura_tool_contract::ToolPinResolutionStateV1::Resolved,
                safe_reason: None,
            }),
            changed_at,
            correlation_id,
        })
        .map_err(|error| map_persistence_error(error, correlation_id))?;
    let entity_revision = pin_result_revision(request.expected_entity_revision, commit.changed)?;
    Ok(ToolMutationEffectV1 {
        result: ToolMutationResultV1::WorkspacePinCommitted {
            workspace_id: request.workspace_id,
            entity_revision,
        },
        projection_changes: commit.projection_changes,
    })
}

fn workspace_pin_clear(
    persistence: &PersistenceCoordinator,
    request: WorkspaceToolPinClearRequestV1,
    changed_at: UtcTimestamp,
    correlation_id: CorrelationId,
) -> Result<ToolMutationEffectV1, IpcErrorEnvelope> {
    let commit = persistence
        .commit_workspace_tool_pin_change(WorkspaceToolPinChangeRequest {
            workspace_id: request.workspace_id,
            tool_id: request.tool_id,
            expected_revision: Some(request.expected_entity_revision),
            target: None,
            changed_at,
            correlation_id,
        })
        .map_err(|error| map_persistence_error(error, correlation_id))?;
    let entity_revision = next_revision(request.expected_entity_revision)?;
    Ok(ToolMutationEffectV1 {
        result: ToolMutationResultV1::WorkspacePinCommitted {
            workspace_id: request.workspace_id,
            entity_revision,
        },
        projection_changes: commit.projection_changes,
    })
}

fn session_pin_set(
    persistence: &PersistenceCoordinator,
    request: SessionToolPinSetRequestV1,
    changed_at: UtcTimestamp,
    correlation_id: CorrelationId,
) -> Result<ToolMutationEffectV1, IpcErrorEnvelope> {
    let workspace_id = workspace_for_session(persistence, request.session_id, correlation_id)?;
    let commit = persistence
        .commit_session_tool_pin_change(SessionToolPinChangeRequest {
            session_id: request.session_id,
            workspace_id,
            tool_id: request.tool_id,
            expected_revision: request.expected_entity_revision,
            target: Some(ToolPinTargetV1 {
                tool_version: request.tool_version,
                state: aura_tool_contract::ToolPinResolutionStateV1::Resolved,
                safe_reason: None,
            }),
            changed_at,
            correlation_id,
        })
        .map_err(|error| map_persistence_error(error, correlation_id))?;
    let entity_revision = pin_result_revision(request.expected_entity_revision, commit.changed)?;
    Ok(ToolMutationEffectV1 {
        result: ToolMutationResultV1::SessionPinCommitted {
            session_id: request.session_id,
            entity_revision,
        },
        projection_changes: commit.projection_changes,
    })
}

fn session_pin_clear(
    persistence: &PersistenceCoordinator,
    request: SessionToolPinClearRequestV1,
    changed_at: UtcTimestamp,
    correlation_id: CorrelationId,
) -> Result<ToolMutationEffectV1, IpcErrorEnvelope> {
    let workspace_id = workspace_for_session(persistence, request.session_id, correlation_id)?;
    let commit = persistence
        .commit_session_tool_pin_change(SessionToolPinChangeRequest {
            session_id: request.session_id,
            workspace_id,
            tool_id: request.tool_id,
            expected_revision: Some(request.expected_entity_revision),
            target: None,
            changed_at,
            correlation_id,
        })
        .map_err(|error| map_persistence_error(error, correlation_id))?;
    let entity_revision = next_revision(request.expected_entity_revision)?;
    Ok(ToolMutationEffectV1 {
        result: ToolMutationResultV1::SessionPinCommitted {
            session_id: request.session_id,
            entity_revision,
        },
        projection_changes: commit.projection_changes,
    })
}

fn workspace_for_session(
    persistence: &PersistenceCoordinator,
    session_id: aura_domain_common::SessionId,
    correlation_id: CorrelationId,
) -> Result<aura_domain_common::WorkspaceId, IpcErrorEnvelope> {
    persistence
        .workspace_session_projection()
        .map_err(|error| map_persistence_error(error, correlation_id))?
        .sessions
        .into_iter()
        .find(|session| session.session_id == session_id)
        .map(|session| session.workspace_id)
        .ok_or_else(|| {
            safe_error(
                ToolRegistryErrorCode::PinUnresolved.error_code(),
                ErrorCategory::NotFound,
                "The Session is unavailable for Tool pinning.",
                correlation_id,
            )
        })
}

fn pin_result_revision(
    expected: Option<Revision>,
    changed: bool,
) -> Result<Revision, IpcErrorEnvelope> {
    match (expected, changed) {
        (None, true) => Ok(Revision::INITIAL),
        (Some(revision), true) => next_revision(revision),
        (Some(revision), false) => Ok(revision),
        (None, false) => Err(journal_failed(CorrelationId::new())),
    }
}

fn next_revision(revision: Revision) -> Result<Revision, IpcErrorEnvelope> {
    revision
        .value()
        .checked_add(1)
        .map(Revision::new)
        .ok_or_else(|| journal_failed(CorrelationId::new()))
}

fn map_command_journal_error(
    error: PersistenceError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    match error {
        PersistenceError::ToolMutationCommandIdempotencyConflict => safe_error(
            ToolDraftErrorCode::IdempotencyConflict.error_code(),
            ErrorCategory::Conflict,
            "The CommandInvocationId is already bound to a different Tool mutation request.",
            correlation_id,
        ),
        PersistenceError::ToolMutationCommandJournalStateConflict
        | PersistenceError::ToolMutationCommandJournalIntegrity => journal_failed(correlation_id),
        other => map_persistence_error(other, correlation_id),
    }
}

fn map_persistence_error(
    error: PersistenceError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    match error {
        PersistenceError::ToolEntityNotFound => safe_error(
            ToolDraftErrorCode::NotFound.error_code(),
            ErrorCategory::NotFound,
            "The requested Tool entity was not found.",
            correlation_id,
        ),
        PersistenceError::ToolEntityRevisionConflict => safe_error(
            ToolDraftErrorCode::RevisionConflict.error_code(),
            ErrorCategory::Conflict,
            "The requested Tool entity revision is stale.",
            correlation_id,
        ),
        PersistenceError::ToolEntityStateConflict => safe_error(
            ToolDraftErrorCode::InvalidTransition.error_code(),
            ErrorCategory::Conflict,
            "The Tool entity state does not permit this operation.",
            correlation_id,
        ),
        PersistenceError::CredentialRecordNotFound
        | PersistenceError::CredentialUnavailable
        | PersistenceError::CredentialBindingMismatch => safe_error(
            ToolRuntimeErrorCode::CredentialBindingInvalid.error_code(),
            ErrorCategory::Conflict,
            "The Tool credential binding is unavailable or stale.",
            correlation_id,
        ),
        PersistenceError::ToolMutationCommandIdempotencyConflict
        | PersistenceError::ToolMutationCommandJournalStateConflict
        | PersistenceError::ToolMutationCommandJournalIntegrity => {
            map_command_journal_error(error, correlation_id)
        }
        _ => journal_failed(correlation_id),
    }
}

fn map_secret_scan_error(
    _error: NetworkControlError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    safe_error(
        ToolRegistryErrorCode::SettingsInvalid.error_code(),
        ErrorCategory::Permission,
        "Tool Instance configuration must not contain credential material.",
        correlation_id,
    )
}

fn journal_failed(correlation_id: CorrelationId) -> IpcErrorEnvelope {
    safe_error(
        ToolDraftErrorCode::JournalFailed.error_code(),
        ErrorCategory::Internal,
        "The Tool mutation Journal could not be completed safely.",
        correlation_id,
    )
}

fn safe_error(
    code: ErrorCode,
    category: ErrorCategory,
    message: &str,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code,
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
    use std::collections::BTreeSet;

    use aura_tool_domain::{TOOL_ACTIVITY_PAGE_METHOD_V1, TOOL_MUTATION_METHODS_V1};

    use super::ToolMutationControlServiceV1;

    #[test]
    fn dispatch_recognizes_exactly_the_closed_eight_mutations() {
        let unique = TOOL_MUTATION_METHODS_V1.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(TOOL_MUTATION_METHODS_V1.len(), 8);
        assert_eq!(unique.len(), 8);
        for method_name in TOOL_MUTATION_METHODS_V1 {
            assert!(
                ToolMutationControlServiceV1::handles_main_command(method_name),
                "missing tool settings dispatch for {method_name}"
            );
        }
        assert!(!ToolMutationControlServiceV1::handles_main_command(TOOL_ACTIVITY_PAGE_METHOD_V1));
        // Retired the P07 Draft, package and long-operation commands.
        for retired in ["tools.draft.create", "tools.package.set_trust", "tools.operation.cancel"] {
            assert!(!ToolMutationControlServiceV1::handles_main_command(retired), "{retired}");
        }
    }
}
