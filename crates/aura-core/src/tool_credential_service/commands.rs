//! Closed Core adapters. Plaintext is accepted only on a bound SecretInput frame.
use super::{ToolCredentialService, ToolSecretError};
use crate::*;
use aura_persistence::ToolCredentialMutationIntentV1;
use aura_tool_domain::{
    ToolCredentialChangeRequestV1, ToolCredentialCreateRequestV1, ToolCredentialListResultV1,
    ToolCredentialManagementTargetV1, ToolCredentialMutationDispositionV1,
    ToolCredentialMutationResultV1,
};

pub(crate) fn binding(
    method: &str,
) -> Option<(aura_ipc::SecretInputOperation, aura_ipc::SecretInputSlot)> {
    let operation = match method {
        "tools.credential.create" => aura_ipc::SecretInputOperation::ToolsCredentialCreate,
        "tools.credential.rotate" => aura_ipc::SecretInputOperation::ToolsCredentialRotate,
        _ => return None,
    };
    Some((operation, aura_ipc::SecretInputSlot::ApiKey))
}

pub(crate) fn validate(
    method: &str,
    params: &Value,
    correlation: CorrelationId,
) -> std::result::Result<(), IpcErrorEnvelope> {
    let valid = match method {
        "tools.credential.origins" => serde_json::from_value::<
            aura_tool_domain::ToolCredentialOriginsRequestV1,
        >(params.clone())
        .is_ok(),
        "tools.credential.create" => {
            serde_json::from_value::<ToolCredentialCreateRequestV1>(params.clone()).is_ok()
        }
        "tools.credential.rotate" | "tools.credential.clear" | "tools.credential.recover" => {
            serde_json::from_value::<ToolCredentialChangeRequestV1>(params.clone()).is_ok()
        }
        "tools.credential.outcome" => serde_json::from_value::<
            aura_tool_domain::ToolCredentialOutcomeRequestV1,
        >(params.clone())
        .is_ok(),
        "tools.credential.list" => {
            serde_json::from_value::<ToolCredentialManagementTargetV1>(params.clone()).is_ok()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(envelope(
            "tool.credential.invalid_request",
            ErrorCategory::Validation,
            correlation,
            IpcRequestTerminal::Failed,
        ))
    }
}

pub(crate) async fn dispatch(
    state: &RuntimeState,
    request: &JsonRpcRequest,
    secret: Option<SecretBytes>,
) -> std::result::Result<Value, IpcErrorEnvelope> {
    validate(&request.method, &request.params, request.id)?;
    let StartupMode::Ready(services) = &state.startup_mode else {
        return Err(envelope(
            "core.recovery_required",
            ErrorCategory::Unavailable,
            request.id,
            IpcRequestTerminal::Failed,
        ));
    };
    if !matches!(request.method.as_str(), "tools.credential.list" | "tools.credential.outcome") {
        let channel = authenticated_configuration_channel_identity(state).map_err(|_| {
            envelope(
                "tool.credential.not_authorized",
                ErrorCategory::Permission,
                request.id,
                IpcRequestTerminal::Failed,
            )
        })?;
        let p04 = services
            .p04()
            .map_err(|_| crate::MissingCapability::P04SecurityAuthority.envelope(request.id))?
            .clone();
        let invocation = request.command_invocation_id.ok_or_else(|| {
            envelope(
                "ipc.command_invocation_id_required",
                ErrorCategory::Validation,
                request.id,
                IpcRequestTerminal::Failed,
            )
        })?;
        p04.authorize_authenticated_main_tool_mutation(
            &request.method,
            &request.params,
            invocation,
            request.id,
            channel,
        )?;
    }
    let persistence = Arc::clone(&services.persistence);
    let work_request = request.clone();
    crate::secret_boundary::run(
        state,
        request,
        move || {
            execute(&persistence, &work_request, secret.as_ref()).map_err(|e| {
                let uncertain = matches!(
                    e,
                    ToolSecretError::NotObtained
                        | ToolSecretError::IdentityMismatch
                        | ToolSecretError::Missing
                );
                if uncertain
                    && !matches!(
                        work_request.method.as_str(),
                        "tools.credential.list" | "tools.credential.outcome"
                    )
                {
                    crate::command_settlement::outcome_unknown_envelope(
                        work_request.id,
                        work_request.command_invocation_id,
                        "工具凭据操作的结果还没有确认，请刷新查看，不要重复提交。".into(),
                    )
                } else {
                    secret_error(e, work_request.id)
                }
            })
        },
        Value::clone,
    )
    .await
}

fn execute(
    persistence: &Arc<PersistenceCoordinator>,
    request: &JsonRpcRequest,
    secret: Option<&SecretBytes>,
) -> std::result::Result<Value, ToolSecretError> {
    let decode_create = || {
        serde_json::from_value::<ToolCredentialCreateRequestV1>(request.params.clone())
            .map_err(|_| ToolSecretError::NotAuthorized)
    };
    let decode_change = || {
        serde_json::from_value::<ToolCredentialChangeRequestV1>(request.params.clone())
            .map_err(|_| ToolSecretError::NotAuthorized)
    };
    let service = ToolCredentialService::new(Arc::clone(persistence));
    let result = match request.method.as_str() {
        "tools.credential.origins" => service.mutate(
            request.command_invocation_id.ok_or(ToolSecretError::NotAuthorized)?,
            ToolCredentialMutationIntentV1::UpdateOrigins {
                request: serde_json::from_value(request.params.clone())
                    .map_err(|_| ToolSecretError::NotAuthorized)?,
            },
            None,
        )?,
        "tools.credential.outcome" => {
            let outcome = serde_json::from_value(request.params.clone())
                .map_err(|_| ToolSecretError::NotAuthorized)?;
            return serde_json::to_value(persistence.tool_credential_outcome_v1(outcome)?)
                .map_err(|_| ToolSecretError::NotObtained);
        }
        "tools.credential.list" => {
            let target = serde_json::from_value(request.params.clone())
                .map_err(|_| ToolSecretError::NotAuthorized)?;
            let credentials = persistence.list_managed_tool_credentials_v1(target)?;
            let visible: std::collections::BTreeSet<_> =
                credentials.iter().map(|c| c.credential_id).collect();
            let pending_credential_ids = persistence
                .incomplete_tool_credential_mutations_v1()?
                .into_iter()
                .map(|j| j.record.credential_id)
                .filter(|id| visible.contains(id))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            return serde_json::to_value(ToolCredentialListResultV1 {
                credentials,
                pending_credential_ids,
            })
            .map_err(|_| ToolSecretError::NotObtained);
        }
        "tools.credential.create" => service.mutate(
            request.command_invocation_id.ok_or(ToolSecretError::NotAuthorized)?,
            ToolCredentialMutationIntentV1::Create { request: decode_create()? },
            secret,
        )?,
        "tools.credential.rotate" => service.mutate(
            request.command_invocation_id.ok_or(ToolSecretError::NotAuthorized)?,
            ToolCredentialMutationIntentV1::Rotate { request: decode_change()? },
            secret,
        )?,
        "tools.credential.clear" => service.mutate(
            request.command_invocation_id.ok_or(ToolSecretError::NotAuthorized)?,
            ToolCredentialMutationIntentV1::Clear { request: decode_change()? },
            None,
        )?,
        "tools.credential.recover" => {
            let change = decode_change()?;
            // Recovery is scoped to a credential the authenticated user can already manage.
            let current = persistence
                .list_managed_tool_credentials_v1(change.target)?
                .into_iter()
                .find(|c| c.credential_id == change.credential_id)
                .ok_or(ToolSecretError::NotAuthorized)?;
            if current.row_revision != change.expected_row_revision {
                return Err(ToolSecretError::Conflict);
            }
            let outcomes = service.recover(Some(change.credential_id))?;
            let mut result = ToolCredentialMutationResultV1 {
                credential: Some(current),
                disposition: ToolCredentialMutationDispositionV1::Committed,
                cleanup_pending: false,
            };
            for outcome in outcomes {
                result = outcome?;
            }
            result
        }
        _ => return Err(ToolSecretError::NotAuthorized),
    };
    serde_json::to_value(result).map_err(|_| ToolSecretError::NotObtained)
}

fn secret_error(error: ToolSecretError, correlation: CorrelationId) -> IpcErrorEnvelope {
    let (code, category) = match error {
        ToolSecretError::Conflict => ("tool.credential.changed", ErrorCategory::Conflict),
        ToolSecretError::NotAuthorized | ToolSecretError::IdentityMismatch => {
            ("tool.credential.not_authorized", ErrorCategory::Permission)
        }
        ToolSecretError::Missing => ("tool.credential.missing", ErrorCategory::Unavailable),
        ToolSecretError::AccessDenied => {
            ("tool.credential.access_denied", ErrorCategory::Permission)
        }
        ToolSecretError::Cancelled => ("tool.credential.cancelled", ErrorCategory::Cancelled),
        ToolSecretError::NotObtained => {
            ("tool.credential.not_obtained", ErrorCategory::Unavailable)
        }
    };
    envelope(code, category, correlation, IpcRequestTerminal::Failed)
}
fn envelope(
    code: &str,
    category: ErrorCategory,
    correlation: CorrelationId,
    terminal: IpcRequestTerminal,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("closed credential code"),
        category,
        correlation_id: correlation,
        terminal,
        retryable: false,
        message: "工具凭据操作没有完成，请先刷新查看当前状态再重试。".into(),
        details: BTreeMap::new(),
    }
}
