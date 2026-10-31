//! Git-specific Core Broker adapter; the shared native adapter owns every child Job.
mod backend;
mod feedback;
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
            aura_backend_core::CoreBroker::new(descriptor(service.runtime), port)
                .map_err(|error| anyhow::anyhow!("{error:?}"))?,
        ))
    }
}
fn reader(
    service: &AgentExecution,
    call: &CallContext,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<git::GitReader> {
    let resolver = service.workspace.resolver(call.snapshot.workspace_id)?;
    resolver.verify_root_identity()?;
    let root = resolver.resolve_existing(&WorkspacePath::parse(WorkspacePath::ROOT)?)?;
    git::GitReader::open(&root.absolute_path, cancel)
}
pub(super) fn inspect(service: &AgentExecution, call: &CallContext) -> anyhow::Result<Value> {
    let reader = reader(service, call, Arc::new(AtomicBool::new(false)))?;
    let facts = reader.facts()?;
    if matches!(call.operation.content.action_kind.as_str(), "git.sync" | "git.push") {
        let target = reader
            .remote_plan(&call.operation.content.action_kind, &call.operation.content.action)?;
        anyhow::ensure!(
            aura_security_contract::git_remote_facts(&call.operation.content.resources)
                == Some(&target),
            "git.remote_changed: 分支或远端已改变，请重新发起操作"
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
    let resolver = service.workspace.resolver(snapshot.workspace_id)?;
    resolver.verify_root_identity()?;
    let root = resolver.resolve_existing(&WorkspacePath::parse(WorkspacePath::ROOT)?)?;
    let reader = git::GitReader::open(&root.absolute_path, Arc::new(AtomicBool::new(false)))?;
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
        .ok_or_else(|| anyhow::anyhow!("git.call_not_active"))?
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
        let checked = service.persistence.with_session_permission_authority(call.snapshot.session_id, |mode| -> anyhow::Result<_> {
            anyhow::ensure!(service.operation_permitted_now(&call.operation, &call.snapshot, mode.mode, mode.revision)?, "git.permission_withdrawn");
            service.verify_source(&call)?;
            aura_persistence_v2::agent::checkpoint::verify_start_operation(service.journal.lock()?.database.connection(), request.operation.operation_id)?;
            let reader = reader(&service, &call, token)?;
            anyhow::ensure!(reader.facts()? == request.plan.prepared.preparation["domain"], "git.repository_changed");
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
                    "git.sync" | "git.push" => writer.remote(&call.operation.content.action_kind, &call.operation.content.action, &remote.ok_or_else(|| anyhow::anyhow!("git.remote_facts_missing"))?)
                        .and_then(|value| Ok(serde_json::to_value(value)?)),
                    _ => unreachable!("closed Native Git actions"),
                };
                side_effect = writer.side_effect(result.is_ok());
                return result;
            }
            drop(start_lease);
            let kind = match call.operation.content.action_kind.as_str() {
                "git.status" => GitReadInputV1::Status(serde_json::from_value(call.operation.content.action.clone())?),
                "git.diff" => GitReadInputV1::Diff(serde_json::from_value(call.operation.content.action.clone())?),
                "git.log" => GitReadInputV1::Log(serde_json::from_value(call.operation.content.action.clone())?),
                _ => anyhow::bail!("git.unknown_tool"),
            };
            Ok(match reader.read(&kind)? { GitReadResultV1::Status(value) => serde_json::to_value(value)?,
                GitReadResultV1::Diff(value) => serde_json::to_value(value)?, GitReadResultV1::Log(value) => serde_json::to_value(value)? })
        })();
        let unknown = output.as_ref().err().is_some_and(|error| error.is::<git::StopUnconfirmed>() || error.is::<git::GitWriteInterrupted>());
        let evidence = BrokerEvidence { action_kind: request.operation.content.action_kind.clone(),
            request_fingerprint: request.operation.fingerprint.0.clone(), before: request.plan.prepared.preparation["domain"].clone(),
            after: output.as_ref().ok().map(canonical_sha256).transpose()?.map_or(Value::Null, |hash| json!({"result":hash})),
            domain_journal: json!({"schema":"agent-native-git-v1","output":output.as_ref().ok(),"error":output.as_ref().err().map(ToString::to_string)}),
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
