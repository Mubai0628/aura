use std::collections::BTreeMap;

use aura_domain_common::{
    CommandInvocationId, CorrelationId, CredentialId, CredentialRevision, ErrorCategory, ErrorCode,
    Revision, UtcTimestamp,
};
use aura_ipc::{SecretInputOperation, SecretInputSlot};
use aura_model_domain::{
    EndpointAuthScheme, EndpointCreatedWithCredentialResult, EndpointCredentialMutationResult,
    EndpointCredentialState, EndpointCredentialStatus, ModelCredentialSlot,
    ModelEndpointClearCredentialRequest, ModelEndpointConfigureCredentialRequest,
    ModelEndpointCreateWithCredentialRequest, ModelEndpointCreatedPayload,
    ModelEndpointReplaceCredentialRequest, model_ipc_method,
};
use aura_persistence::{
    CreateModelCredentialSlot, ModelCredentialWriteIntent, ModelProjectionDraft, PersistenceError,
    PrepareModelCredentialClear, PrepareModelCredentialWrite,
};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use aura_windows_platform::SecretBytes;
use serde_json::Value;
use thiserror::Error;

use super::{P05CommandDispatch, decode, encode, prepared_snapshot, user_event};
use crate::{
    p05::P05Services,
    p05::config_journal::{ModelConfigCredentialCommandPlan, ModelConfigJournalError},
    p05_credential::{CredentialClearCompletion, P05CredentialJournalError},
};

#[derive(Debug, Error)]
enum CredentialCommandError {
    #[error("models configuration revision changed")]
    ModelsRevisionChanged,
    #[error("model endpoint does not exist")]
    EndpointNotFound,
    #[error("credential command does not match the endpoint authentication scheme")]
    AuthenticationSchemeMismatch,
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Journal(#[from] P05CredentialJournalError),
    #[error(transparent)]
    CredentialStore(#[from] aura_windows_platform::CredentialStoreError),
    #[error(transparent)]
    Projection(#[from] crate::p05::projection::P05ProjectionError),
    #[error(transparent)]
    ConfigJournal(#[from] crate::p05::config_journal::ModelConfigJournalError),
}

struct CredentialWriteCommand {
    expected_models_revision: aura_domain_common::ExpectedRevision,
    endpoint_id: aura_domain_common::EndpointId,
    credential_slot: ModelCredentialSlot,
    credential_id: aura_domain_common::CredentialId,
    expected_row_revision: aura_domain_common::ExpectedRevision,
    intent: ModelCredentialWriteIntent,
}

impl P05Services {
    #[must_use]
    pub fn handles_secret_main_command(method_name: &str) -> bool {
        aura_ipc::routes::core_route(method_name)
            .is_some_and(|r| r.auth == aura_ipc::routes::RouteAuth::SecretInput)
    }

    #[must_use]
    pub fn secret_input_binding(
        method_name: &str,
    ) -> Option<(SecretInputOperation, SecretInputSlot)> {
        let operation = match method_name {
            model_ipc_method::ENDPOINT_CREATE_WITH_CREDENTIAL => {
                SecretInputOperation::ModelsEndpointCreateWithCredential
            }
            model_ipc_method::ENDPOINT_CONFIGURE_CREDENTIAL => {
                SecretInputOperation::ModelsEndpointConfigureCredential
            }
            model_ipc_method::ENDPOINT_REPLACE_CREDENTIAL => {
                SecretInputOperation::ModelsEndpointReplaceCredential
            }
            _ => return None,
        };
        Some((operation, SecretInputSlot::ApiKey))
    }

    pub fn validate_secret_control_params(
        method_name: &str,
        params: &Value,
        correlation_id: CorrelationId,
    ) -> Result<(), IpcErrorEnvelope> {
        match method_name {
            model_ipc_method::ENDPOINT_CREATE_WITH_CREDENTIAL => {
                let _: aura_model_domain::ModelEndpointCreateWithCredentialRequest =
                    decode(params, correlation_id)?;
            }
            model_ipc_method::ENDPOINT_CONFIGURE_CREDENTIAL => {
                let _: ModelEndpointConfigureCredentialRequest = decode(params, correlation_id)?;
            }
            model_ipc_method::ENDPOINT_REPLACE_CREDENTIAL => {
                let _: ModelEndpointReplaceCredentialRequest = decode(params, correlation_id)?;
            }
            _ => {
                return Err(credential_error(
                    CredentialCommandError::EndpointNotFound,
                    correlation_id,
                ));
            }
        }
        Ok(())
    }

    pub fn dispatch_secret_main_command(
        &self,
        method_name: &str,
        params: &Value,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        secret: &SecretBytes,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        match method_name {
            model_ipc_method::ENDPOINT_CONFIGURE_CREDENTIAL => {
                let request: ModelEndpointConfigureCredentialRequest =
                    decode(params, correlation_id)?;
                self.write_credential(
                    CredentialWriteCommand {
                        expected_models_revision: request.expected_models_revision,
                        endpoint_id: request.endpoint_id,
                        credential_slot: request.credential_slot,
                        credential_id: request.credential_id,
                        expected_row_revision: request.expected_row_revision,
                        intent: ModelCredentialWriteIntent::Configure,
                    },
                    command_invocation_id,
                    correlation_id,
                    secret,
                )
            }
            model_ipc_method::ENDPOINT_REPLACE_CREDENTIAL => {
                let request: ModelEndpointReplaceCredentialRequest =
                    decode(params, correlation_id)?;
                self.write_credential(
                    CredentialWriteCommand {
                        expected_models_revision: request.expected_models_revision,
                        endpoint_id: request.endpoint_id,
                        credential_slot: request.credential_slot,
                        credential_id: request.credential_id,
                        expected_row_revision: request.expected_row_revision,
                        intent: ModelCredentialWriteIntent::Replace,
                    },
                    command_invocation_id,
                    correlation_id,
                    secret,
                )
            }
            model_ipc_method::ENDPOINT_CREATE_WITH_CREDENTIAL => {
                let request: ModelEndpointCreateWithCredentialRequest =
                    decode(params, correlation_id)?;
                self.create_endpoint_with_credential(
                    request,
                    command_invocation_id,
                    correlation_id,
                    secret,
                )
            }
            _ => unreachable!("SecretInput dispatch checked the method name"),
        }
    }

    pub(super) fn dispatch_clear_credential(
        &self,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let request: ModelEndpointClearCredentialRequest = decode(params, correlation_id)?;
        let command_invocation_id = command_invocation_id.ok_or_else(|| IpcErrorEnvelope {
            code: ErrorCode::new("ipc.command_invocation_id_required")
                .expect("static ErrorCode is valid"),
            category: ErrorCategory::Validation,
            message: "这次请求缺少操作编号，Aura Core 没有执行它。请重新打开设置后再试。"
                .to_owned(),
            retryable: false,
            correlation_id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        })?;
        let snapshot = self.model_config.snapshot().map_err(|_| {
            credential_error(CredentialCommandError::EndpointNotFound, correlation_id)
        })?;
        if snapshot.revision != request.expected_models_revision.value() {
            return Err(credential_error(
                CredentialCommandError::ModelsRevisionChanged,
                correlation_id,
            ));
        }
        let endpoint = snapshot.registry.endpoints.get(&request.endpoint_id).ok_or_else(|| {
            credential_error(CredentialCommandError::EndpointNotFound, correlation_id)
        })?;
        if endpoint.auth_scheme == EndpointAuthScheme::None {
            return Err(credential_error(
                CredentialCommandError::AuthenticationSchemeMismatch,
                correlation_id,
            ));
        }

        let mut projection_factory =
            |_: &aura_persistence::CredentialMutationJournalRecord,
             status: EndpointCredentialStatus| {
                self.credential_projection_drafts(&snapshot, endpoint.id, status)
                    .map_err(|error| P05CredentialJournalError::Projection(error.to_string()))
            };
        let completion = self
            .credential_journal
            .clear(
                PrepareModelCredentialClear {
                    command_invocation_id,
                    endpoint_id: endpoint.id,
                    credential_id: request.credential_id,
                    credential_slot: request.credential_slot,
                    expected_row_revision: request.expected_row_revision,
                    models_revision: Revision::new(snapshot.revision),
                    correlation_id,
                },
                &mut projection_factory,
            )
            .map_err(|error| credential_error(error.into(), correlation_id))?;
        let (changed, status, projection_changes) = match completion {
            CredentialClearCompletion::NoChange(status) => (false, status, Vec::new()),
            CredentialClearCompletion::Committed(completion) => (
                true,
                completion
                    .metadata_commit
                    .credential_record
                    .renderer_status()
                    .map_err(|error| credential_error(error.into(), correlation_id))?,
                completion.metadata_commit.projection_changes,
            ),
        };
        Ok(P05CommandDispatch {
            value: encode(
                EndpointCredentialMutationResult {
                    endpoint_id: endpoint.id,
                    changed,
                    models_revision: Revision::new(snapshot.revision),
                    credential_status: status,
                },
                correlation_id,
            )?,
            projection_changes,
        })
    }

    fn write_credential(
        &self,
        command: CredentialWriteCommand,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        secret: &SecretBytes,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let snapshot = self.model_config.snapshot().map_err(|_| {
            credential_error(CredentialCommandError::EndpointNotFound, correlation_id)
        })?;
        if snapshot.revision != command.expected_models_revision.value() {
            return Err(credential_error(
                CredentialCommandError::ModelsRevisionChanged,
                correlation_id,
            ));
        }
        let endpoint = snapshot.registry.endpoints.get(&command.endpoint_id).ok_or_else(|| {
            credential_error(CredentialCommandError::EndpointNotFound, correlation_id)
        })?;
        if endpoint.auth_scheme == EndpointAuthScheme::None
            || command.credential_slot != ModelCredentialSlot::ApiKey
        {
            return Err(credential_error(
                CredentialCommandError::AuthenticationSchemeMismatch,
                correlation_id,
            ));
        }
        let binding = self
            .credential_storage_binding(endpoint, command.credential_id)
            .map_err(|error| credential_error(error.into(), correlation_id))?;
        let mut projection_factory =
            |_: &aura_persistence::CredentialMutationJournalRecord,
             status: EndpointCredentialStatus| {
                self.credential_projection_drafts(&snapshot, endpoint.id, status)
                    .map_err(|error| P05CredentialJournalError::Projection(error.to_string()))
            };
        let completion = self
            .credential_journal
            .configure_secret(
                PrepareModelCredentialWrite {
                    command_invocation_id,
                    endpoint_id: command.endpoint_id,
                    credential_id: command.credential_id,
                    credential_slot: command.credential_slot,
                    expected_row_revision: command.expected_row_revision,
                    intent: command.intent,
                    binding,
                    models_revision: Revision::new(snapshot.revision),
                    correlation_id,
                },
                secret,
                &mut projection_factory,
            )
            .map_err(|error| credential_error(error.into(), correlation_id))?;
        let status = completion
            .metadata_commit
            .credential_record
            .renderer_status()
            .map_err(|error| credential_error(error.into(), correlation_id))?;
        Ok(P05CommandDispatch {
            value: encode(
                EndpointCredentialMutationResult {
                    endpoint_id: command.endpoint_id,
                    changed: true,
                    models_revision: Revision::new(snapshot.revision),
                    credential_status: status,
                },
                correlation_id,
            )?,
            projection_changes: completion.metadata_commit.projection_changes,
        })
    }

    fn create_endpoint_with_credential(
        &self,
        request: ModelEndpointCreateWithCredentialRequest,
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        secret: &SecretBytes,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        if request.auth_scheme == EndpointAuthScheme::None {
            return Err(credential_error(
                CredentialCommandError::AuthenticationSchemeMismatch,
                correlation_id,
            ));
        }
        let expected_revision = request.expected_revision;
        let input = request.into_input();
        let completion = self
            .config_journal
            .execute_with_credential(
                &self.credential_journal,
                command_invocation_id,
                correlation_id,
                secret,
                |parent_journal_id, _child_journal_id| {
                    let prepared = self.model_config.prepare_update(
                        parent_journal_id.to_string(),
                        expected_revision.value(),
                        |registry| {
                            aura_model_config::create_endpoint(
                                registry,
                                input,
                                UtcTimestamp::now(),
                            )
                        },
                    )?;
                    let endpoint = prepared
                        .candidate_registry
                        .endpoints
                        .get(&prepared.value)
                        .cloned()
                        .ok_or(aura_model_config::ModelConfigError::InternalInvariant(
                            "created Endpoint is absent from the candidate registry",
                        ))?;
                    if endpoint.auth_scheme == EndpointAuthScheme::None {
                        return Err(ModelConfigJournalError::ModelConfig(
                            aura_model_config::ModelConfigError::InternalInvariant(
                                "credential-bearing endpoint was normalized to auth none",
                            ),
                        ));
                    }
                    let credential_id = CredentialId::new();
                    let binding = self.credential_storage_binding(&endpoint, credential_id)?;
                    let status = EndpointCredentialStatus {
                        endpoint_id: endpoint.id,
                        slot_name: ModelCredentialSlot::ApiKey,
                        credential_id,
                        state: EndpointCredentialState::Configured,
                        active_credential_revision: Some(CredentialRevision::INITIAL),
                        last_committed_credential_revision: Some(CredentialRevision::INITIAL),
                        row_revision: Revision::new(2),
                        error_code: None,
                    };
                    status.validate().map_err(|_| {
                        ModelConfigJournalError::ModelConfig(
                            aura_model_config::ModelConfigError::InternalInvariant(
                                "credential-bearing endpoint status violates the frozen Contract",
                            ),
                        )
                    })?;
                    let status_overrides =
                        BTreeMap::from([(endpoint.id, status.clone())]);
                    let snapshot = prepared_snapshot(&prepared);
                    let model_settings = self
                        .projections
                        .model_settings(&snapshot, &status_overrides)?;
                    let endpoint_projection = self.projections.endpoint_detail(
                        &snapshot,
                        endpoint.id,
                        &status_overrides,
                    )?;
                    if endpoint_projection.authentication.active_credential.as_ref()
                        != Some(&status)
                        || endpoint_projection.authentication.dormant_credential.is_some()
                    {
                        return Err(ModelConfigJournalError::ModelConfig(
                            aura_model_config::ModelConfigError::InternalInvariant(
                                "credential-bearing endpoint projection violates the frozen Contract",
                            ),
                        ));
                    }
                    let prepared_bundle = prepared.prepared_bundle.ok_or(
                        aura_model_config::ModelConfigError::InternalInvariant(
                            "created Endpoint mutation lacks a prepared bundle",
                        ),
                    )?;
                    Ok(ModelConfigCredentialCommandPlan {
                        prepared_bundle: Box::new(prepared_bundle),
                        expected_models_revision: Revision::new(expected_revision.value()),
                        reserved_models_revision: prepared.candidate_revision,
                        credential_slot_creation: CreateModelCredentialSlot {
                            credential_id,
                            endpoint_id: endpoint.id,
                            binding,
                        },
                        metadata_events: vec![user_event(
                            "ModelEndpointCreated",
                            correlation_id,
                            Some(parent_journal_id.to_string()),
                            &ModelEndpointCreatedPayload {
                                endpoint,
                                credential_status: Some(status),
                                models_revision: prepared.candidate_revision,
                            },
                        )?],
                        projection_drafts: vec![
                            ModelProjectionDraft::ModelSettings(model_settings),
                            ModelProjectionDraft::EndpointDetail(Box::new(
                                endpoint_projection.clone(),
                            )),
                        ],
                        success_result: serde_json::to_value(
                            EndpointCreatedWithCredentialResult {
                                models_revision: prepared.candidate_revision,
                                endpoint_projection,
                            },
                        )?,
                    })
                },
            )
            .map_err(|error| credential_error(error.into(), correlation_id))?;
        Ok(P05CommandDispatch {
            value: completion.success_result,
            projection_changes: completion.projection_changes,
        })
    }

    fn credential_projection_drafts(
        &self,
        snapshot: &aura_model_config::ModelConfigSnapshot,
        endpoint_id: aura_domain_common::EndpointId,
        status: EndpointCredentialStatus,
    ) -> Result<
        Vec<aura_persistence::ModelProjectionDraft>,
        crate::p05::projection::P05ProjectionError,
    > {
        self.projections.credential_mutation_drafts(snapshot, endpoint_id, status)
    }
}

fn credential_error(
    error: CredentialCommandError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    let error = match error {
        CredentialCommandError::ConfigJournal(error) => {
            return super::journal_error(error, correlation_id);
        }
        // The credential journal wraps the persistence and storage errors it
        // observed. Unwrapping them keeps the *first* failing layer visible: a
        // state or revision conflict must reach its specific code instead of the
        // unclassified fallback, and a Windows Credential Manager failure must
        // be distinguishable from a Core-side refusal.
        CredentialCommandError::Journal(P05CredentialJournalError::Persistence(error)) => {
            CredentialCommandError::Persistence(error)
        }
        CredentialCommandError::Journal(P05CredentialJournalError::CredentialStore(error)) => {
            CredentialCommandError::CredentialStore(error)
        }
        error => error,
    };
    let error = match error {
        // A store failure is reported from the layer that failed, with a
        // bounded, non-secret diagnostic. The Key itself never reaches it.
        CredentialCommandError::CredentialStore(error) => {
            let mut details = BTreeMap::new();
            details
                .insert("stage".to_owned(), Value::String("windows_credential_store".to_owned()));
            details.insert(
                "store_error".to_owned(),
                Value::String(error.to_string().chars().take(200).collect()),
            );
            return IpcErrorEnvelope {
                code: ErrorCode::new("credential.store_unavailable")
                    .expect("static ErrorCode is valid"),
                category: ErrorCategory::Unavailable,
                message: "Windows 凭据管理器拒绝了这次操作，API Key 没有改变；请重试。".to_owned(),
                retryable: false,
                correlation_id,
                terminal: IpcRequestTerminal::Failed,
                details,
            };
        }
        error => error,
    };
    let (code, category, message, retryable) = match error {
        CredentialCommandError::ModelsRevisionChanged => (
            "models.configuration.revision_conflict",
            ErrorCategory::Conflict,
            "模型设置已在别处更新，请刷新后再试。",
            true,
        ),
        CredentialCommandError::EndpointNotFound => (
            "models.endpoint.not_found",
            ErrorCategory::NotFound,
            "这个连接已不存在，请刷新模型设置。",
            false,
        ),
        CredentialCommandError::AuthenticationSchemeMismatch => (
            "credential.authentication_scheme_mismatch",
            ErrorCategory::Validation,
            "这次凭据操作与连接保存的认证方式不一致，请刷新后重试。",
            false,
        ),
        CredentialCommandError::Persistence(PersistenceError::CredentialRowRevisionConflict) => (
            "credential.row_revision_conflict",
            ErrorCategory::Conflict,
            "凭据状态已经变化，请刷新后再试。",
            true,
        ),
        CredentialCommandError::Persistence(PersistenceError::CredentialStateConflict) => (
            "credential.state_conflict",
            ErrorCategory::Conflict,
            "这个凭据当前的状态不允许这项操作，请刷新模型设置后重试。",
            false,
        ),
        CredentialCommandError::Journal(P05CredentialJournalError::ReconciliationRequired {
            ..
        }) => (
            "credential.reconciliation_required",
            ErrorCategory::Unavailable,
            "这个凭据需要先完成恢复处理才能继续使用，请到模型设置中查看。",
            false,
        ),
        CredentialCommandError::Journal(P05CredentialJournalError::Aborted { .. }) => (
            "credential.mutation_aborted",
            ErrorCategory::Unavailable,
            "凭据操作在提交前被中止，没有改变任何内容，请重试。",
            false,
        ),
        _ => (
            "credential.operation_failed",
            ErrorCategory::Unavailable,
            "凭据操作没有完成，请刷新模型设置后重试。",
            false,
        ),
    };
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static ErrorCode is valid"),
        category,
        message: message.to_owned(),
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CorrelationId, EndpointId, ErrorCategory};
    use aura_model_config::{ModelConfigError, ModelConfigValidationError};

    use super::{CredentialCommandError, credential_error};
    use crate::p05::config_journal::ModelConfigJournalError;

    #[test]
    fn credential_bearing_create_preserves_endpoint_validation_errors() {
        let error = CredentialCommandError::ConfigJournal(ModelConfigJournalError::ModelConfig(
            ModelConfigError::Validation(ModelConfigValidationError::MissingChatOutputTokenField {
                endpoint_id: EndpointId::new(),
            }),
        ));
        let envelope = credential_error(error, CorrelationId::new());

        assert_eq!(envelope.code.as_str(), "models.endpoint.chat_output_token_field_required");
        assert_eq!(envelope.category, ErrorCategory::Validation);
        assert_eq!(
            envelope.details.get("field").and_then(serde_json::Value::as_str),
            Some("chat_output_token_field")
        );
    }
}
