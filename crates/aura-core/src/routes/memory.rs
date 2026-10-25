//! Desktop management is authenticated independently of model tool permissions.
//! All roots come from Core's fixed Memory owner and verified workspace resolver.
use crate::sync_boundary::{SyncSection, SyncSubject};
use crate::*;
use aura_domain_common::WorkspaceId;
use aura_memory::*;
pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    let result = handle(state, request).await;
    let (terminal, response) = match result {
        Ok(value) => (IpcRequestTerminal::Succeeded, JsonRpcResponse::success(request.id, value)),
        Err(error) => (error.terminal, JsonRpcResponse::failure(request.id, error)),
    };
    send_recorded_response(state, queue, request, response).await?;
    Ok(terminal)
}
async fn handle(
    state: &RuntimeState,
    request: &JsonRpcRequest,
) -> std::result::Result<Value, IpcErrorEnvelope> {
    authenticated_configuration_channel_identity(state).map_err(|_| {
        error(
            request.id,
            "memory.not_authorized",
            ErrorCategory::Permission,
            "记忆管理只能由 Aura 主界面发起。",
        )
    })?;
    let StartupMode::Ready(services) = &state.startup_mode else {
        return Err(error(
            request.id,
            "memory.unavailable",
            ErrorCategory::Unavailable,
            "请先完成数据恢复。",
        ));
    };
    let mutation = aura_ipc::routes::side_effecting(&request.method);
    if mutation && request.command_invocation_id.is_none() {
        return Err(error(
            request.id,
            "ipc.command_invocation_id_required",
            ErrorCategory::Validation,
            "缺少操作标识。",
        ));
    }
    let memory = services.broker.memory().clone();
    let settings = services._settings.clone();
    let workspace = services.p03().ok().map(|s| s.workspace.clone());
    let runtime = services.activity_runtime_instance_id;
    let params = request.params.clone();
    let method = request.method.clone();
    match state
        .sync
        .run(
            SyncSection::MemoryVault,
            SyncSubject::new().request(request.id).method(&request.method),
            move || run(&memory, &settings, workspace.as_deref(), runtime, &method, params),
        )
        .await
    {
        crate::blocking_pool::BlockingAdmission::Completed(Ok(value)) => Ok(value),
        crate::blocking_pool::BlockingAdmission::Completed(Err(cause))
            if mutation && write_may_have_committed(&cause) =>
        {
            append_core_log(
                "memory.management_outcome_unknown",
                json!({"correlation_id":request.id}),
            );
            Err(crate::command_settlement::outcome_unknown_envelope(request.id,request.command_invocation_id,
                "记忆文件或设置可能已经写入，后续记录未完成。请刷新、校验并检查文件；不要重复提交原操作。".into()))
        }
        crate::blocking_pool::BlockingAdmission::Completed(Err(cause)) => {
            Err(classify(request.id, &cause))
        }
        crate::blocking_pool::BlockingAdmission::Refused { .. } => Err(error(
            request.id,
            "memory.busy",
            ErrorCategory::Unavailable,
            "记忆操作尚未开始，Aura 正忙，请稍后重试。",
        )),
        _ if mutation => Err(crate::command_settlement::outcome_unknown_envelope(
            request.id,
            request.command_invocation_id,
            "记忆操作的结果尚未确认；请刷新查看，不要重复提交。".into(),
        )),
        _ => Err(error(
            request.id,
            "memory.timeout",
            ErrorCategory::Unavailable,
            "读取记忆超时，请稍后刷新。",
        )),
    }
}
fn workspace(
    runtime: Option<&aura_workspace::WorkspaceRuntime>,
    id: Option<WorkspaceId>,
) -> anyhow::Result<Option<MemoryWorkspace>> {
    id.map(|workspace_id| {
        let runtime = runtime.ok_or_else(|| anyhow::anyhow!("workspace unavailable"))?;
        let resolver = runtime.resolver(workspace_id)?;
        resolver.verify_root_identity()?;
        Ok(MemoryWorkspace { workspace_id, root: resolver.canonical_root().to_path_buf() })
    })
    .transpose()
}
fn decode<T: serde::de::DeserializeOwned>(params: Value) -> anyhow::Result<T> {
    Ok(serde_json::from_value(params)?)
}
pub(super) fn run(
    memory: &MemoryService,
    settings: &SettingsFacade,
    runtime_workspace: Option<&aura_workspace::WorkspaceRuntime>,
    runtime: RuntimeInstanceId,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    let actor = WriteActor { kind: "settings_repair".into(), ..Default::default() };
    let result = match method {
        "memory.settings.get" => {
            let _: aura_agent_domain::EmptyToolInput = decode(params)?;
            serde_json::to_value(settings.memory_settings())?
        }
        "memory.settings.update" => {
            let input: aura_config::MemorySettingsUpdate = decode(params)?;
            settings.update_memory_settings(input.expected_revision, &input.settings)?;
            serde_json::to_value(settings.memory_settings())?
        }
        "memory.vaults.get" => {
            let input: MemoryDesktopRequest = decode(params)?;
            let workspace = workspace(runtime_workspace, input.workspace_id)?;
            serde_json::to_value(memory.desktop_snapshot(workspace.as_ref(), runtime)?)?
        }
        "memory.export.entry" => serde_json::to_value(memory.export_entry(&decode(params)?)?)?,
        "memory.export.chunk" => serde_json::to_value(memory.export_chunk(&decode(params)?)?)?,
        "memory.export.close" => serde_json::to_value(memory.close_export(&decode(params)?))?,
        "memory.vault.repair.apply" => {
            let input: MemoryDesktopRepairApply = decode(params)?;
            let workspace = workspace(runtime_workspace, input.vault.workspace_id)?;
            let vault = memory.desktop_vault(input.vault.scope, workspace.as_ref())?;
            serde_json::to_value(MemoryRepairResult {
                preview: vault.apply_repair(&input.preview_id, &actor)?,
                applied: true,
            })?
        }
        _ => {
            let input: MemoryVaultRequest = decode(params)?;
            let workspace = workspace(runtime_workspace, input.workspace_id)?;
            let vault = memory.desktop_vault(input.scope, workspace.as_ref())?;
            match method {
                "memory.vault.validate" => serde_json::to_value(vault.validation_report()?)?,
                "memory.vault.rebuild" => {
                    vault.rebuild()?;
                    serde_json::to_value(vault.desktop_status()?)?
                }
                "memory.vault.repair.preview" => {
                    serde_json::to_value(vault.preview_repair(&actor)?)?
                }
                "memory.export.prepare" => serde_json::to_value(memory.prepare_export(&vault)?)?,
                _ => anyhow::bail!("unknown memory management route"),
            }
        }
    };
    if serde_json::to_vec(&result)?.len() > 2 * 1024 * 1024 {
        anyhow::bail!("memory response too large");
    }
    Ok(result)
}
fn error(
    id: CorrelationId,
    code: &str,
    category: ErrorCategory,
    message: &str,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static code"),
        category,
        message: message.into(),
        retryable: false,
        correlation_id: id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}
fn classify(id: CorrelationId, cause: &anyhow::Error) -> IpcErrorEnvelope {
    let conflict = cause.chain().any(|c| {
        matches!(
            c.downcast_ref::<aura_config::SettingsFacadeError>(),
            Some(aura_config::SettingsFacadeError::RevisionConflict { .. })
        ) || matches!(
            c.downcast_ref::<MemoryError>(),
            Some(
                MemoryError::Conflict { .. }
                    | MemoryError::File(aura_fs_tools::vault::VaultFileError::Conflict { .. })
            )
        )
    });
    // Log only the error class, never arbitrary external file content or secrets.
    let code = if conflict { "memory.conflict" } else { "memory.management_failed" };
    append_core_log(code, json!({"correlation_id":id}));
    error(
        id,
        code,
        if conflict { ErrorCategory::Conflict } else { ErrorCategory::Validation },
        if conflict {
            "记忆或设置已变化，请刷新后重新预览。"
        } else {
            "记忆操作未完成。请校验 Vault、检查路径与文件权限后重试；修复需要重新预览。"
        },
    )
}
// The file is authoritative: a later metadata/index failure cannot say that a
// repair failed without changing files. Keep the same conservative distinction
// as the model Memory adapter, with the original invocation and no replay.
fn write_may_have_committed(cause: &anyhow::Error) -> bool {
    if let Some(error) = cause.downcast_ref::<MemoryError>() {
        return !matches!(
            error,
            MemoryError::Format(_)
                | MemoryError::Secret { .. }
                | MemoryError::Conflict { .. }
                | MemoryError::Invalid(_)
                | MemoryError::NotFound
                | MemoryError::RolledBack { .. }
                | MemoryError::File(aura_fs_tools::vault::VaultFileError::Conflict { .. })
        );
    }
    if let Some(error) = cause.downcast_ref::<aura_config::SettingsFacadeError>() {
        return !matches!(
            error,
            aura_config::SettingsFacadeError::Validation { .. }
                | aura_config::SettingsFacadeError::RevisionConflict { .. }
                | aura_config::SettingsFacadeError::DomainUnavailable { .. }
                | aura_config::SettingsFacadeError::Codec { .. }
                | aura_config::SettingsFacadeError::RevisionExhausted { .. }
        );
    }
    false
}
