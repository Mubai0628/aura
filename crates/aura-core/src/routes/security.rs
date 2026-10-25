//! security route adapter; domain services retain authorization and transaction ownership.
use crate::*;

pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    match request.method.as_str() {
        method_name if P04Services::handles_main_command(method_name) => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                let error = IpcErrorEnvelope {
                    code: ErrorCode::new("core.recovery_required")?,
                    category: ErrorCategory::Unavailable,
                    message: "Aura 的数据需要恢复，权限设置暂不可用。".to_owned(),
                    retryable: false,
                    correlation_id: request.id,
                    terminal: IpcRequestTerminal::Failed,
                    details: BTreeMap::new(),
                };
                send_recorded_response(
                    state,
                    queue,
                    request,
                    JsonRpcResponse::failure(request.id, error),
                )
                .await?;
                return Ok(IpcRequestTerminal::Failed);
            };
            let previous_mode = if method_name == method::PERMISSION_SET_SESSION_MODE {
                let mode: aura_permission_domain::SetSessionPermissionModeInput =
                    serde_json::from_value(request.params.clone())?;
                Some(services.p04()?.ensure_permission_state(mode.session_id, request.id)?.mode)
            } else {
                None
            };
            let mut result = if method_name == "broker.http_read" {
                broker::dispatch_http(state, services, request).await
            } else if method_name == method::ARTIFACT_EXPORT {
                broker::dispatch_artifact(state, services, request).await
            } else {
                // P04's security authority is a real dependency of this
                // route, so a damaged Security/Aura domain answers with a stable
                // `Unavailable` envelope instead of a panic. The route's own
                // refusals keep their existing shapes.
                match services.p04() {
                    Ok(p04) => p04
                        .dispatch_main_command(method_name, &request.params, request.id)
                        .await
                        .expect("recognized P04 Electron Main command must be dispatched"),
                    Err(_) => {
                        Err(crate::MissingCapability::P04SecurityAuthority.envelope(request.id))
                    }
                }
            };
            if method_name == method::PERMISSION_SET_SESSION_MODE && result.is_ok() {
                let mode: aura_permission_domain::SetSessionPermissionModeInput =
                    serde_json::from_value(request.params.clone())?;
                // Defaults change without cancelling model generation or Query waiting.
                // Capability differences and valid explicit approval decide managed stops.
                if let Err(error) = services
                    .agent()?
                    .mode_changed(
                        mode.session_id,
                        previous_mode.expect("mode setter read its previous mode"),
                    )
                    .await
                {
                    let mut failure = broker::error_envelope(request.id, error);
                    failure.message = format!(
                        "The default permission mode was changed to {:?}; managed execution has not confirmed stopping: {}",
                        mode.mode, failure.message
                    );
                    failure
                        .details
                        .insert("permission_change_applied".to_owned(), Value::Bool(true));
                    result = Err(failure);
                }
            }
            if matches!(
                method_name,
                method::FILE_SESSION_CONSENT_REVOKE
                    | method::MEMORY_SESSION_CONSENT_REVOKE
                    | method::NETWORK_SESSION_CONSENT_REVOKE
                    | method::NATIVE_SESSION_CONSENT_REVOKE
            ) && result.is_ok()
            {
                let session: aura_domain_common::SessionId =
                    serde_json::from_value(request.params["session_id"].clone())?;
                if let Err(error) = services.agent()?.file_authority_withdrawn(session).await {
                    let mut failure = broker::error_envelope(request.id, error);
                    failure.message = format!(
                        "Consent was revoked; managed execution has not confirmed stopping: {}",
                        failure.message
                    );
                    failure
                        .details
                        .insert("permission_change_applied".to_owned(), Value::Bool(true));
                    result = Err(failure);
                }
            }
            match result {
                Ok(value) => {
                    send_recorded_response(
                        state,
                        queue,
                        request,
                        JsonRpcResponse::success(request.id, value),
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Succeeded)
                }
                Err(error) => {
                    send_recorded_response(
                        state,
                        queue,
                        request,
                        JsonRpcResponse::failure(request.id, error),
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Failed)
                }
            }
        }

        _ => bail!("registered security route has no handler"),
    }
}
