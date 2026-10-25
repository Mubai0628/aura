//! Bound metadata, one-shot SecretInput dispatch and expiration.
use crate::*;

pub(crate) async fn prepare_pending_secret_command(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
    channel: IpcFrameHeader,
) -> Result<Option<PendingSecretInput>> {
    let Some(command_invocation_id) = request.command_invocation_id else {
        let error = IpcErrorEnvelope {
            code: ErrorCode::new("ipc.command_invocation_id_required")?,
            category: ErrorCategory::Validation,
            message: "这次密钥输入缺少操作编号，没有执行；请重试。".to_owned(),
            retryable: false,
            correlation_id: request.id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        };
        try_send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request.id, error))?;
        return Ok(None);
    };
    if state
        .command_outcomes
        .lock()
        .map_err(|_| anyhow::anyhow!("command outcome ledger was poisoned"))?
        .entries
        .contains_key(&command_invocation_id)
    {
        let mut details = BTreeMap::new();
        details.insert(
            "command_invocation_id".to_owned(),
            serde_json::to_value(command_invocation_id)?,
        );
        let error = IpcErrorEnvelope {
            code: ErrorCode::new("ipc.command_invocation_already_completed")?,
            category: ErrorCategory::Conflict,
            message: "这项操作已经完成过，不会重复执行；请刷新查看结果。".to_owned(),
            retryable: false,
            correlation_id: request.id,
            terminal: IpcRequestTerminal::Failed,
            details,
        };
        try_send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request.id, error))?;
        return Ok(None);
    }
    if !matches!(state.startup_mode, StartupMode::Ready(_)) {
        let error = IpcErrorEnvelope {
            code: ErrorCode::new("core.recovery_required")?,
            category: ErrorCategory::Unavailable,
            message: "Aura 的数据需要恢复，凭据操作暂不可用。".to_owned(),
            retryable: false,
            correlation_id: request.id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        };
        try_send_recorded_response(
            state,
            queue,
            request,
            JsonRpcResponse::failure(request.id, error),
        )
        .await?;
        return Ok(None);
    }
    if let Err(error) = if terminal_secret::binding(&request.method).is_some() {
        terminal_secret::validate(&request.params, request.id)
    } else if mcp::credentials::command::binding(&request.method).is_some() {
        mcp::credentials::command::validate(&request.params, request.id)
    } else if tool_credential_service::commands::binding(&request.method).is_some() {
        tool_credential_service::commands::validate(&request.method, &request.params, request.id)
    } else {
        P05Services::validate_secret_control_params(&request.method, &request.params, request.id)
    } {
        try_send_recorded_response(
            state,
            queue,
            request,
            JsonRpcResponse::failure(request.id, error),
        )
        .await?;
        return Ok(None);
    }
    let (operation, slot) = terminal_secret::binding(&request.method)
        .or_else(|| mcp::credentials::command::binding(&request.method))
        .or_else(|| tool_credential_service::commands::binding(&request.method))
        .or_else(|| P05Services::secret_input_binding(&request.method))
        .context("recognized SecretInput method has no frame binding")?;
    Ok(Some(PendingSecretInput::new(
        request.id,
        command_invocation_id,
        operation,
        slot,
        channel.channel_id,
        channel.connection_epoch,
        Instant::now(),
    )))
}

pub(crate) async fn handle_secret_request(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
    secret: &SecretBytes,
) -> Result<IpcRequestTerminal> {
    let StartupMode::Ready(services) = &state.startup_mode else {
        bail!("SecretInput command survived after Core entered recovery mode");
    };
    let command_invocation_id = request
        .command_invocation_id
        .context("pending SecretInput request has no CommandInvocationId")?;
    let result = if terminal_secret::binding(&request.method).is_some() {
        terminal_secret::dispatch(state, request, secret).await
    } else if mcp::credentials::command::binding(&request.method).is_some() {
        mcp::credentials::command::dispatch(state, request, secret).await
    } else if tool_credential_service::commands::binding(&request.method).is_some() {
        tool_credential_service::commands::dispatch(
            state,
            request,
            Some(SecretBytes::new(secret.expose().to_vec())),
        )
        .await
        .map(|value| p05::P05CommandDispatch { value, projection_changes: Vec::new() })
    } else {
        match services.p05() {
            Err(_) => Err(MissingCapability::P05ModelAuthority.envelope(request.id)),
            Ok(p05) => {
                let p05 = p05.clone();
                let owned_request = request.clone();
                let owned_secret = SecretBytes::new(secret.expose().to_vec());
                secret_boundary::run(
                    state,
                    request,
                    move || {
                        p05.dispatch_secret_main_command(
                            &owned_request.method,
                            &owned_request.params,
                            command_invocation_id,
                            owned_request.id,
                            &owned_secret,
                        )
                    },
                    |dispatch| dispatch.value.clone(),
                )
                .await
            }
        }
    };
    match result {
        Ok(dispatch) => {
            for change in dispatch.projection_changes {
                send_notification(
                    queue,
                    projection_change_priority(&change),
                    method::PROJECTION_CHANGE,
                    &change,
                )
                .await?;
            }
            try_send_recorded_response(
                state,
                queue,
                request,
                JsonRpcResponse::success(request.id, dispatch.value),
            )
            .await?;
            Ok(IpcRequestTerminal::Succeeded)
        }
        Err(error) => {
            try_send_recorded_response(
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

pub(crate) async fn expire_pending_secret_commands(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    ledger: &Arc<std::sync::Mutex<RequestLedger>>,
    registry: &mut PendingSecretInputRegistry,
    commands: &mut BTreeMap<CommandInvocationId, JsonRpcRequest>,
) -> Result<()> {
    for command_invocation_id in registry.take_expired(Instant::now()) {
        let request = commands
            .remove(&command_invocation_id)
            .context("expired SecretInput command metadata was missing")?;
        let error = IpcErrorEnvelope {
            code: ErrorCode::new("ipc.secret_input.expired")?,
            category: ErrorCategory::Unavailable,
            message: "密钥没有在时限内送达，没有保存；请重新输入。".to_owned(),
            retryable: false,
            correlation_id: request.id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        };
        try_send_recorded_response(
            state,
            queue,
            &request,
            JsonRpcResponse::failure(request.id, error),
        )
        .await?;
        ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("request ledger was poisoned"))?
            .finish(request.id, IpcRequestTerminal::Failed)
            .context("expired SecretInput request terminal was not unique")?;
    }
    Ok(())
}
