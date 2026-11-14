//! Git-specific Core Broker adapter; the shared native adapter owns every child Job.
mod backend;
mod feedback;
mod journal;
use super::*;
use aura_domain_common::jcs::canonical_sha256;
use aura_git_service::git;
use aura_workspace_domain::{GitReadInputV1, GitReadResultV1};
pub(super) use backend::{NATIVE_ID, is_write, native_descriptor};
pub(super) use feedback::admission_feedback;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const BACKEND_ID: &str = "native-git-broker-v1";
pub(super) fn descriptor(runtime: RuntimeInstanceId) -> BackendDescriptor {
    crate::broker::git_descriptor(runtime)
}
pub(super) fn backend(
    service: &Arc<AgentExecution>,
    call: &CallContext,
) -> anyhow::Result<Arc<dyn aura_backend_api::ExecutionBackend>> {
    let port = broker::Port { service: service.clone(), call: call.clone(), commands: None };
    if is_write(&call.operation.content.action_kind) {
        Ok(Arc::new(backend::NativeGitBackend {
            descriptor: native_descriptor(service.runtime),
            port,
        }))
    } else {
        Ok(Arc::new(
            aura_backend_core::CoreBroker::new(descriptor(service.runtime), port).map_err(|_| {
                anyhow::anyhow!(
                    "git.broker_unavailable: Aura 没能建立 Git 读取通道，这次 Git 调用没有执行。请再试一次；仍然失败时请告诉用户。"
                )
            })?,
        ))
    }
}
fn reader(
    service: &AgentExecution,
    call: &CallContext,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<git::GitReader> {
    git::GitReader::open(&root(service, call.snapshot.workspace_id)?, cancel)
}
/// The workspace folder Git runs in; a folder Aura cannot reach is said in words.
fn root(service: &AgentExecution, workspace: WorkspaceId) -> anyhow::Result<std::path::PathBuf> {
    let resolver = service.workspace.resolver(workspace).map_err(journal::workspace)?;
    resolver.verify_root_identity().map_err(journal::workspace)?;
    let top = WorkspacePath::parse(WorkspacePath::ROOT)?;
    Ok(resolver.resolve_existing(&top).map_err(journal::workspace)?.absolute_path)
}
pub(super) fn inspect(service: &AgentExecution, call: &CallContext) -> anyhow::Result<Value> {
    let reader = reader(service, call, Arc::new(AtomicBool::new(false)))?;
    let facts = reader.facts()?;
    let kind = call.operation.content.action_kind.as_str();
    if matches!(kind, "git.sync" | "git.push") {
        let target = reader.remote_plan(kind, &call.operation.content.action)?;
        let what = if kind == "git.push" { "推送" } else { "同步" };
        anyhow::ensure!(
            aura_security_contract::git_remote_facts(&call.operation.content.resources)
                == Some(&target),
            "git.remote_changed: 确定这次{what}的目标之后，本地分支、HEAD 或远端已经改变；{what}没有执行，也没有连接远端。请重新调用，按当前目标重新批准。"
        );
    }
    Ok(facts)
}
pub(super) fn remote_resources(
    service: &AgentExecution,
    snapshot: &PermissionSnapshot,
    kind: &str,
    action: &Value,
) -> anyhow::Result<Vec<ResourceDescriptor>> {
    let folder = root(service, snapshot.workspace_id)?;
    let reader = git::GitReader::open(&folder, Arc::new(AtomicBool::new(false)))?;
    let facts = reader.remote_plan(kind, action)?;
    Ok(vec![
        ResourceDescriptor::GitRepository { workspace_id: snapshot.workspace_id },
        ResourceDescriptor::GitRemoteDestination { facts },
    ])
}

pub(super) async fn invoke(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> anyhow::Result<()> {
    let lifetime = service.active_calls.retain_worker(call.validated.call.tool_call_id)?;
    let mut cancellation = service
        .active_calls
        .session(call.snapshot.session_id)?
        .into_iter()
        .find(|entry| entry.call.validated.call.tool_call_id == call.validated.call.tool_call_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "git.call_not_active: 这次 Git 调用在开始前已经不在运行（所属的轮次可能已经停止），没有执行任何 Git 命令。需要时请重新调用。"
            )
        })?
        .cancel
        .subscribe();
    let token = Arc::new(AtomicBool::new(*cancellation.borrow()));
    let cancelled = token.clone();
    let relay = tokio::spawn(async move {
        while cancellation.changed().await.is_ok() {
            if *cancellation.borrow_and_update() {
                cancelled.store(true, Ordering::Release);
                break;
            }
        }
    });
    let _relay = Relay { task: relay, token: token.clone() };
    tokio::task::spawn_blocking(move || {
        let _lifetime = lifetime;
        // Git processes not seen to stop are kept under this call: its locks wait for them.
        let _kept = aura_backend_windows_user::lingering::enter(request.binding.attempt.operation_id.to_bytes());
        let checked = service.persistence.with_session_permission_authority(call.snapshot.session_id, |mode| -> anyhow::Result<_> {
            anyhow::ensure!(service.operation_permitted_now(&call.operation, &call.snapshot, mode.mode, mode.revision)?, journal::PERMISSION_WITHDRAWN);
            service.verify_source(&call)?;
            aura_persistence_v2::agent::checkpoint::verify_start_operation(service.journal.lock()?.database.connection(), request.operation.operation_id)?;
            let reader = reader(&service, &call, token)?;
            anyhow::ensure!(reader.facts()? == request.plan.prepared.preparation["domain"], journal::REPOSITORY_CHANGED);
            Ok(reader)
        })?;
        let mut side_effect = SideEffectStateV1::NoEffect;
        let output: anyhow::Result<Value> = (|| {
            let reader = checked?;
            if is_write(&call.operation.content.action_kind) {
                let remote = aura_security_contract::git_remote_facts(&call.operation.content.resources).cloned();
                let mut writer = git::GitWriter::new(reader, move || drop(start_lease));
                let result: anyhow::Result<Value> = match call.operation.content.action_kind.as_str() {
                    "git.commit" => writer.commit(&serde_json::from_value(call.operation.content.action.clone())?)
                        .and_then(|value| Ok(serde_json::to_value(value)?)),
                    "git.branch" => writer.branch(&serde_json::from_value(call.operation.content.action.clone())?)
                        .and_then(|value| Ok(serde_json::to_value(value)?)),
                    "git.sync" | "git.push" => writer.remote(&call.operation.content.action_kind, &call.operation.content.action, &remote.ok_or_else(|| anyhow::anyhow!(journal::REMOTE_FACTS_MISSING))?)
                        .and_then(|value| Ok(serde_json::to_value(value)?)),
                    _ => unreachable!("closed Native Git actions"),
                };
                side_effect = writer.side_effect(&result);
                return result;
            }
            drop(start_lease);
            let kind = match call.operation.content.action_kind.as_str() {
                "git.status" => GitReadInputV1::Status(serde_json::from_value(call.operation.content.action.clone())?),
                "git.diff" => GitReadInputV1::Diff(serde_json::from_value(call.operation.content.action.clone())?),
                "git.log" => GitReadInputV1::Log(serde_json::from_value(call.operation.content.action.clone())?),
                _ => anyhow::bail!(journal::UNKNOWN_TOOL),
            };
            Ok(match reader.read(&kind)? { GitReadResultV1::Status(value) => serde_json::to_value(value)?,
                GitReadResultV1::Diff(value) => serde_json::to_value(value)?, GitReadResultV1::Log(value) => serde_json::to_value(value)? })
        })();
        let unknown = output.as_ref().err().is_some_and(|error| error.is::<git::StopUnconfirmed>() || error.is::<git::GitWriteInterrupted>());
        let evidence = BrokerEvidence { action_kind: request.operation.content.action_kind.clone(),
            request_fingerprint: request.operation.fingerprint.0.clone(), before: request.plan.prepared.preparation["domain"].clone(),
            after: output.as_ref().ok().map(canonical_sha256).transpose()?.map_or(Value::Null, |hash| json!({"result":hash})),
            domain_journal: json!({"schema":"agent-native-git-v1","output":output.as_ref().ok(),"error":output.as_ref().err().map(|error| journal::error_text(error, side_effect))}),
            side_effect };
        service.journal.complete(&request, evidence, if unknown { crate::broker::CompletionVerdict::Unknown }
            else if output.is_ok() { crate::broker::CompletionVerdict::Succeeded } else { crate::broker::CompletionVerdict::Failed })
    }).await?
}
struct Relay {
    task: tokio::task::JoinHandle<()>,
    token: Arc<AtomicBool>,
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.token.store(true, Ordering::Release);
        self.task.abort();
    }
}
