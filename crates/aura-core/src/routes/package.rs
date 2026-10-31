//! `packages.*` route adapter. Main is authenticated; every request is decoded into
//! its exact contract here, and the package service owns every check and write.
use crate::package_services::{ImportSource, PackageServices, RunEnvironment};
use crate::*;
use aura_core_base::sync_boundary::{SyncSection, SyncSubject};
use aura_tool_domain::*;

pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    let outcome = handle(state, request).await;
    let (terminal, response) = match outcome {
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
    let StartupMode::Ready(services) = &state.startup_mode else {
        return Err(envelope(
            request.id,
            "package.unavailable",
            ErrorCategory::Unavailable,
            "工具包功能在数据恢复完成前不可用。",
        ));
    };
    let packages = services.packages.clone().ok_or_else(|| {
        envelope(
            request.id,
            "package.unavailable",
            ErrorCategory::Unavailable,
            "工具包服务启动失败，请查看 Core 日志后重启 Aura。",
        )
    })?;
    let method = request.method.as_str();
    let mutation = aura_ipc::routes::side_effecting(method);
    if mutation {
        authenticated_configuration_channel_identity(state).map_err(|_| {
            envelope(
                request.id,
                "package.not_authorized",
                ErrorCategory::Permission,
                "工具包操作只能由 Aura 主界面发起。",
            )
        })?;
        if request.command_invocation_id.is_none() {
            return Err(envelope(
                request.id,
                "ipc.command_invocation_id_required",
                ErrorCategory::Validation,
                "缺少操作标识。",
            ));
        }
    }
    let params = request.params.clone();
    let id = request.id;
    let invocation = request.command_invocation_id;
    // Prepare needs Core-resolved inputs that are only reachable asynchronously.
    let environment = if method == "packages.conformance.prepare" {
        let powershell = match services.commands() {
            Ok(commands) => commands.managed_runtime().await.ok(),
            Err(_) => None,
        };
        Some(RunEnvironment {
            powershell,
            user_environment: aura_windows_platform::current_user_environment()
                .map_err(|error| package_error(id, &anyhow::anyhow!(error)))?,
        })
    } else {
        None
    };
    let work_method = method.to_owned();
    let work =
        move || -> anyhow::Result<Value> { run(&packages, &work_method, params, environment) };
    match state
        .sync
        .run(SyncSection::PackageLifecycle, SyncSubject::new().request(id).method(method), work)
        .await
    {
        aura_core_base::blocking_pool::BlockingAdmission::Completed(Ok(value)) => Ok(value),
        aura_core_base::blocking_pool::BlockingAdmission::Completed(Err(error)) => {
            append_core_log(
                "package.request_failed",
                json!({"method": method, "reason": format!("{error:#}")}),
            );
            Err(package_error(id, &error))
        }
        aura_core_base::blocking_pool::BlockingAdmission::Refused { .. } => Err(envelope(
            id,
            "package.busy",
            ErrorCategory::Unavailable,
            "Aura 正忙，工具包操作没有开始，请稍后重试。",
        )),
        _ if mutation => Err(aura_core_base::command_settlement::outcome_unknown_envelope(
            id,
            invocation,
            "工具包操作仍在进行，结果尚未确认。请刷新工具包列表查看，不要重复提交。".into(),
        )),
        _ => Err(envelope(
            id,
            "package.timeout",
            ErrorCategory::Unavailable,
            "读取工具包超时，请重试。",
        )),
    }
}

fn run(
    packages: &PackageServices,
    method: &str,
    params: Value,
    environment: Option<RunEnvironment>,
) -> anyhow::Result<Value> {
    fn decode<T: serde::de::DeserializeOwned>(params: Value) -> anyhow::Result<T> {
        serde_json::from_value(params)
            .map_err(|_| anyhow::anyhow!("package.invalid_request: 请求格式不正确"))
    }
    Ok(match method {
        "packages.list" => {
            let _: aura_agent_domain::EmptyToolInput = decode(params)?;
            serde_json::to_value(packages.list()?)?
        }
        "packages.import_directory" | "packages.import_archive" => {
            let source: PackageImportSourceV1 = decode(params)?;
            let kind = if method == "packages.import_directory" {
                ImportSource::Directory
            } else {
                ImportSource::Archive
            };
            serde_json::to_value(packages.import(kind, std::path::Path::new(&source.path))?)?
        }
        "packages.version.get" => serde_json::to_value(packages.version_view(&decode(params)?)?)?,
        "packages.conformance.prepare" => serde_json::to_value(packages.prepare(
            &decode(params)?,
            environment.ok_or_else(|| anyhow::anyhow!("package.environment_missing"))?,
        )?)?,
        "packages.conformance.start" => serde_json::to_value(packages.start(&decode(params)?)?)?,
        "packages.conformance.get" => {
            serde_json::to_value(packages.conformance(&decode(params)?)?)?
        }
        "packages.conformance.cancel" => serde_json::to_value(packages.cancel(&decode(params)?)?)?,
        "packages.install" => serde_json::to_value(packages.install(&decode(params)?)?)?,
        "packages.set_enabled" => serde_json::to_value(packages.set_enabled(&decode(params)?)?)?,
        "packages.revoke_trust" => serde_json::to_value(packages.revoke_trust(&decode(params)?)?)?,
        "packages.uninstall" => serde_json::to_value(packages.uninstall(&decode(params)?)?)?,
        "packages.credential.bind" => {
            serde_json::to_value(packages.bind_credential(&decode(params)?)?)?
        }
        "packages.credential.clear" => {
            serde_json::to_value(packages.clear_credential(&decode(params)?)?)?
        }
        _ => anyhow::bail!("package.unknown_method"),
    })
}

fn envelope(
    correlation_id: CorrelationId,
    code: &str,
    category: ErrorCategory,
    message: &str,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code)
            .unwrap_or_else(|_| ErrorCode::new("package.failed").expect("static code")),
        category,
        message: message.to_owned(),
        retryable: false,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

/// Fixed codes and messages only: a raw OS error, path or type name never reaches the
/// renderer. A `package.*` reason carries its own user text after ": ".
fn package_error(correlation_id: CorrelationId, error: &anyhow::Error) -> IpcErrorEnvelope {
    if error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<aura_persistence_core::PersistenceCoreError>(),
            Some(aura_persistence_core::PersistenceCoreError::Conflict)
        )
    }) {
        return envelope(
            correlation_id,
            "package.conflict",
            ErrorCategory::Conflict,
            "工具包已经变化，请刷新后重试。",
        );
    }
    for cause in error.chain() {
        let text = cause.to_string();
        let Some(rest) = text.strip_prefix("package.") else { continue };
        let (code, message) = match rest.split_once(": ") {
            Some((code, message)) => (format!("package.{code}"), message.to_owned()),
            None => (format!("package.{rest}"), default_message(rest).to_owned()),
        };
        if code.len() <= 96
            && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_')
        {
            return envelope(correlation_id, &code, ErrorCategory::Validation, &message);
        }
    }
    envelope(
        correlation_id,
        "package.failed",
        ErrorCategory::Unavailable,
        "工具包操作未完成，请查看 Core 日志。",
    )
}

fn default_message(code: &str) -> &'static str {
    match code {
        "not_found" | "version_not_found" => "找不到这个工具包或版本，请刷新列表。",
        "conformance_not_approved" | "conformance_missing" | "conformance_incomplete" => {
            "这个版本没有通过验证，不能安装。"
        }
        "settings_invalid" | "settings_not_canonical" => "工具设置不符合这个版本的要求。",
        "not_installed" => "这个工具包尚未安装。",
        "not_trusted" => "这个工具包已撤销信任，请重新安装。",
        "program_credentials_unsupported" => "运行程序的工具不能使用 Aura 保存的凭据，不能安装。",
        "owner_stopping" => "Aura 正在退出，工具包操作没有开始。",
        "conformance_capacity" => "同时进行的验证过多，请稍后重试。",
        "run_not_active" => "这次验证已经结束。",
        _ => "工具包操作未完成。",
    }
}
