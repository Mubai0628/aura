//! model route adapter; domain services retain authorization and transaction ownership.
use crate::*;

pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    subscriptions: &ProjectionSubscriptions,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    match request.method.as_str() {
        method_name if P05Services::handles_main_command(method_name) => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                let error = IpcErrorEnvelope {
                    code: ErrorCode::new("core.recovery_required")?,
                    category: ErrorCategory::Unavailable,
                    message: "Aura 的数据需要恢复，模型设置暂不可用。".to_owned(),
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
            if method_name == "broker.context" {
                let (response, terminal) = match broker::dispatch_context(state, services, request)
                    .await
                {
                    Ok(value) => {
                        (JsonRpcResponse::success(request.id, value), IpcRequestTerminal::Succeeded)
                    }
                    Err(error) => {
                        (JsonRpcResponse::failure(request.id, error), IpcRequestTerminal::Failed)
                    }
                };
                send_recorded_response(state, queue, request, response).await?;
                return Ok(terminal);
            }
            let result = broker::dispatch_model(state, services, request).await;
            match result {
                Ok(dispatch) => {
                    send_recorded_response(
                        state,
                        queue,
                        request,
                        JsonRpcResponse::success(request.id, dispatch.value),
                    )
                    .await?;
                    for change in dispatch.projection_changes {
                        if subscriptions.contains(&change) {
                            send_notification(
                                queue,
                                projection_change_priority(&change),
                                method::PROJECTION_CHANGE,
                                &change,
                            )
                            .await?;
                        }
                    }
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

        _ => bail!("registered model route has no handler"),
    }
}
