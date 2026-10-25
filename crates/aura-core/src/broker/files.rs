use crate::{JsonRpcRequest, P04Services, RuntimeServices, RuntimeState, p04};
use aura_domain_common::{SessionId, WorkspaceId, WorkspacePath};
use aura_execution_contract::{BrokerEvidence, SideEffectStateV1};
use aura_runtime_contract::IpcErrorEnvelope;
#[cfg(feature = "acceptance-barrier")]
use aura_runtime_contract::IpcRequestTerminal;
use aura_security_contract::{ResolvedCapability, ResourceDescriptor};
use aura_workspace_domain::{FsStatRequest, WorkspaceFileTarget};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// How long one file/Store command may occupy a blocking worker before the route answers.
///
/// It matches the workspace route's own bound. The work itself is unchanged: a command
/// that reaches its handler still runs to completion, and only a request whose handler has
/// not answered by this point is told that its outcome is unknown.
const FILE_DISPATCH_DEADLINE: Duration = Duration::from_secs(120);

pub(crate) async fn dispatch_files(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> Result<Value, IpcErrorEnvelope> {
    let mutation = P04Services::gates_authenticated_main_workspace_mutation(&request.method);
    let (workspace, session, paths, capabilities) = if mutation {
        let r = p04::resolve_workspace_mutation(&request.method, &request.params, request.id)?;
        (r.workspace_id, r.session_id, r.paths, r.capabilities)
    } else {
        read_scope(&request.params).map_err(|e| super::error_envelope(request.id, e))?
    };
    let resources = paths
        .iter()
        .map(|path| ResourceDescriptor::WorkspacePath {
            workspace_id: workspace,
            canonical_relative_path: path.clone(),
            file_target: None,
        })
        .collect();
    let operation =
        super::operation(&request.method, &request.params, request.id, capabilities, resources)
            .map_err(|e| super::error_envelope(request.id, e))?;
    // The broker runs each real pre/post read on its existing bounded owner. Keep only the
    // first preparation for observation; never substitute it for the post-execution evidence.
    let prepared: Arc<std::sync::OnceLock<Value>> = Arc::new(std::sync::OnceLock::new());
    let inspect: super::OwnedInspect = {
        // The workspace/buffer capability is a real dependency of this
        // broker route, so it is refused with a stable code when its configuration
        // domain is damaged.
        let p03 = services
            .p03()
            .map_err(|_| crate::MissingCapability::P03WorkspaceFiles.envelope(request.id))?;
        let fs = Arc::clone(&p03.fs);
        let runtime = Arc::clone(&p03.workspace);
        let paths = paths.clone();
        let prepared = Arc::clone(&prepared);
        Arc::new(move || {
            let value = facts(&fs, &runtime, workspace, session, &paths)?;
            let _ = prepared.set(value.clone());
            Ok(value)
        })
    };
    let activity_subject = crate::agent_activity::MainFsActivitySubject {
        session_id: session,
        workspace_id: workspace,
        runtime_instance_id: services.activity_runtime_instance_id,
        operation_id: operation.operation_id,
        command_invocation_id: request.command_invocation_id,
        targets: Vec::new(),
    };
    let index = services.activity_index.clone();
    let observer = services.activity_observer.clone();
    let activity_method = request.method.clone();
    let activity_paths = paths.clone();
    let activity_publisher: aura_fs_tools::FileExecutionObserver = Arc::new(move |requests| {
        let mut subject = activity_subject.clone();
        subject.targets = verified_targets_of(&activity_paths, prepared.get());
        let mut record = crate::agent_activity::main_record(&subject, &activity_method);
        if !requests.is_empty() {
            record.resources = crate::agent_activity::resources::project(requests);
        }
        let id = record.id;
        let generation = record.generation;
        observer.observe(index.publish(record));
        Box::new(crate::agent_activity::AgentActivityGuard::new(index.clone(), id, generation))
    });
    let evidence_inspect = Arc::clone(&inspect);
    let evidence_persistence = Arc::clone(&services.persistence);
    let event_method = request.method.clone();
    let event_id = request.id;
    let evidence: super::OwnedEvidence<Value> = Arc::new(
        move |dispatch: &aura_execution_contract::DispatchRequest,
              output: &Result<Value, IpcErrorEnvelope>| {
            let after = evidence_inspect().unwrap_or(Value::Null);
            let journal = evidence_persistence.workspace_broker_evidence(event_id)?;
            let before = dispatch.plan.prepared.preparation.clone();
            let states = journal
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v["state"].as_str())
                .collect::<Vec<_>>();
            let buffer_saved = event_method == aura_ipc::method::BUFFER_SAVE
                && output.as_ref().is_ok_and(|value| {
                    value["dirty"] == false
                        && value.get("disk_hash").is_some_and(|hash| {
                            !hash.is_null() && Some(hash) == after.pointer("/paths/0/disk_hash")
                        })
                });
            let side_effect = if !mutation {
                SideEffectStateV1::NoEffect
            } else if buffer_saved {
                if before == after {
                    SideEffectStateV1::NoEffect
                } else {
                    SideEffectStateV1::FullyApplied
                }
            } else if !states.is_empty() && states.iter().all(|s| *s == "compensated") {
                SideEffectStateV1::Compensated
            } else if !states.is_empty()
                && states.iter().all(|s| *s == "completed")
                && after != Value::Null
            {
                SideEffectStateV1::FullyApplied
            } else if states.is_empty() && after == before {
                SideEffectStateV1::NoEffect
            } else {
                SideEffectStateV1::Unknown
            };
            // A compensated mutation is a failure. Never label it succeeded merely
            // because a wrapper happened to return a success projection.
            anyhow::ensure!(
                side_effect != SideEffectStateV1::Compensated || output.is_err(),
                "compensated success"
            );
            Ok(BrokerEvidence {
                action_kind: event_method.clone(),
                request_fingerprint: dispatch.operation.fingerprint.0.clone(),
                before,
                after,
                domain_journal: journal,
                side_effect,
            })
        },
    );
    let handler_request = request.clone();
    // The security authority and the workspace capability are real
    // dependencies of a Main file mutation, so a damaged configuration domain
    // refuses this route with a stable `Unavailable` code rather than panicking.
    let handler_p04 = services
        .p04()
        .map_err(|_| crate::MissingCapability::P04SecurityAuthority.envelope(request.id))?
        .clone();
    let handler_p03 = services
        .p03()
        .map_err(|_| crate::MissingCapability::P03WorkspaceFiles.envelope(request.id))?
        .clone();
    let outcome = super::run(
        state,
        &services.broker,
        operation,
        request.command_invocation_id.map_or_else(|| request.id.to_string(), |id| id.to_string()),
        request.command_invocation_id,
        inspect,
        evidence,
        move || async move {
            // Recheck live permission revisions after preparation and before
            // entering the existing secure-path transaction implementation.
            if mutation {
                handler_p04.authorize_authenticated_main_workspace_mutation(
                    &handler_request.method,
                    &handler_request.params,
                    handler_request.id,
                )?;
            }
            // ACCEPTANCE PROCESSING BOUNDARY (default-off, feature-gated, marker-matched): every real
            // step above has already happened - authorization, claim, ledger reservation, router - and
            // the real handler below still runs and still produces the real result. Only this
            // request's entry can be delayed. With the feature off none of this exists.
            #[cfg(feature = "acceptance-barrier")]
            crate::acceptance_barrier::hold_processing(
                &handler_request.method,
                &handler_request.params,
                handler_request.id,
                handler_request.command_invocation_id,
            )
            .await;
            // The work under this route is synchronous - a path walk, a hash, a catalog
            // update, the one writer's transaction - so it runs on the bounded blocking
            // pool instead of on this reactor thread. Everything that made the request
            // admissible has already run (authorization, the broker reservation, the
            // command claim, the router decision), the handler is the same production
            // one, and its result and its typed envelope are passed through untouched;
            // only where the synchronous part is allowed to block changes. A pool that
            // cannot admit the call inside the deadline says so with the same
            // classification the workspace route uses: `persistence.busy` when nothing
            // ran, `command.outcome_unknown` when the handler may still have an effect.
            let recheck_request = handler_request.clone();
            let recheck_p04 = handler_p04.clone();
            handler_p03
                .dispatch_main_command_owned(
                    &handler_request.method,
                    &handler_request.params,
                    handler_request.id,
                    std::time::Instant::now() + FILE_DISPATCH_DEADLINE,
                    Some(activity_publisher),
                    Arc::new(move || {
                        if mutation {
                            recheck_p04.authorize_authenticated_main_workspace_mutation(
                                &recheck_request.method,
                                &recheck_request.params,
                                recheck_request.id,
                            )
                        } else {
                            Ok(())
                        }
                    }),
                )
                .await
                .into_route_result(handler_request.id, handler_request.command_invocation_id)
        },
    )
    .await;
    // ACCEPTANCE EFFECT RECORD (default-off, feature-gated, designated request only): production
    // evidence of how many times this invocation's handler and broker transaction really finished.
    // It sits after the whole transaction, records nothing but identity, a committed flag and the real
    // terminal of this same dispatch, and records nothing at all for a request no injected install
    // designated. The terminal is the one this `Result` really carries - a successful dispatch is
    // `succeeded` (the terminal `JsonRpcResponse::success` puts on the wire), a failed one is the
    // terminal of the real error envelope. No value is invented here, and nothing about the result
    // itself changes.
    #[cfg(feature = "acceptance-barrier")]
    crate::acceptance_barrier::record_effect(
        &request.method,
        request.id,
        request.command_invocation_id,
        outcome.is_ok(),
        match &outcome {
            Ok(_) => IpcRequestTerminal::Succeeded,
            Err(envelope) => envelope.terminal,
        },
    );
    outcome
}

type ReadScope = (WorkspaceId, SessionId, Vec<WorkspacePath>, Vec<ResolvedCapability>);
fn read_scope(params: &Value) -> anyhow::Result<ReadScope> {
    let workspace = serde_json::from_value(params["workspace_id"].clone())?;
    let session = serde_json::from_value(params["session_id"].clone())?;
    let path = params
        .get("root")
        .or_else(|| params.pointer("/target/path"))
        .or_else(|| params.get("path"));
    let path = path.map_or_else(
        || WorkspacePath::parse(WorkspacePath::ROOT).map_err(anyhow::Error::from),
        |v| serde_json::from_value(v.clone()).map_err(anyhow::Error::from),
    )?;
    Ok((workspace, session, vec![path], vec![ResolvedCapability::WorkspaceFileRead]))
}

pub(crate) fn facts(
    fs: &aura_fs_tools::FsToolService,
    runtime: &aura_workspace::WorkspaceRuntime,
    workspace: WorkspaceId,
    session: SessionId,
    paths: &[WorkspacePath],
) -> anyhow::Result<Value> {
    let resolver = runtime.resolver(workspace)?;
    resolver.verify_root_identity()?;
    let mut values = Vec::new();
    for path in paths {
        match resolver.resolve_existing(path) {
            Ok(resolved) => {
                let stat = fs.stat(FsStatRequest {
                    workspace_id: workspace,
                    session_id: session,
                    target: WorkspaceFileTarget { path: path.clone() },
                })?;
                let disk_hash = if stat.kind == aura_workspace_domain::FsNodeKind::File {
                    use std::io::Read;
                    let mut file = resolver
                        .open_existing(path, aura_windows_platform::NativeFileAccess::Read)?;
                    let mut hasher = blake3::Hasher::new();
                    let mut block = [0u8; 65536];
                    loop {
                        let count = file.file.file_mut().read(&mut block)?;
                        if count == 0 {
                            break;
                        }
                        hasher.update(&block[..count]);
                    }
                    Some(aura_domain_common::ContentHash::new(
                        aura_domain_common::ContentHashAlgorithm::Blake3,
                        hasher.finalize().to_hex().to_string(),
                    )?)
                } else {
                    None
                };
                values.push(json!({"path":path,"identity":aura_workspace::domain_identity(&resolved.info.identity),"kind":stat.kind,"hash":stat.content_hash,"disk_hash":disk_hash,"editor":stat.active_editor_session}));
            }
            Err(aura_workspace::WorkspaceError::NotFound { .. }) => {
                // New nested directories may have absent intermediate parents;
                // bind the nearest existing ancestor without following escapes.
                let mut ancestor = path.clone();
                let parent = loop {
                    let text =
                        ancestor.as_str().rsplit_once('/').map_or(WorkspacePath::ROOT, |(p, _)| p);
                    ancestor = WorkspacePath::parse(text)?;
                    match resolver.resolve_existing(&ancestor) {
                        Ok(parent) => break parent,
                        Err(aura_workspace::WorkspaceError::NotFound { .. })
                            if !ancestor.is_root() => {}
                        Err(e) => return Err(e.into()),
                    }
                };
                values.push(json!({"path":path,"absent":true,"parent":ancestor,"parent_identity":aura_workspace::domain_identity(&parent.info.identity)}));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(json!({"root_identity":resolver.root_identity(),"paths":values}))
}

/// Copies the identities `facts()` already verified into the activity contract's target shape (R1/R3).
///
/// WHY THIS IS A COPY AND NOT A SECOND READ
/// `facts()` above is the one place that really verifies identity: it calls
/// `resolver.verify_root_identity()` and `resolver.resolve_existing(path)` and reports the real
/// `FileIdentity`. This function only reinterprets that already-produced JSON, so publishing
/// costs no additional file I/O, opens no handle and takes no lock.
///
/// WHY THE KEY IS DERIVED THE SAME WAY THE OTHER PUBLISHERS DERIVE IT
/// The platform identity is two opaque strings (`volume_serial_number`, `file_id`). They are
/// folded into the index's compact `(u32, u64)` key space with the same canonical digest the
/// Agent publisher uses (`agent_activity::identity_key`), so a Main row and an Agent row for one
/// physical file land on the **same** `ActivityResourceKey`. A truncated hash or a path string
/// would not.
///
/// A target with no verified identity (an absent path) is reported against its prospective
/// parent's verified identity, which is the honest position fact for a file that does not exist
/// yet, rather than being passed off as a real object or silently dropped.
fn verified_targets_of(
    paths: &[WorkspacePath],
    pre_state: Option<&Value>,
) -> Vec<crate::agent_activity::MainFsTarget> {
    let entries = pre_state
        .and_then(|value| value.get("paths"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let entry = entries.get(index);
            let identity = entry.and_then(|entry| entry.get("identity")).and_then(|value| {
                serde_json::from_value::<aura_domain_common::FileIdentity>(value.clone()).ok()
            });
            let directory = entry
                .and_then(|entry| entry.get("kind"))
                .and_then(|value| {
                    serde_json::from_value::<aura_workspace_domain::FsNodeKind>(value.clone()).ok()
                })
                .is_some_and(|kind| kind == aura_workspace_domain::FsNodeKind::Directory);
            crate::agent_activity::MainFsTarget {
                path: path.as_str().to_owned(),
                target: identity.as_ref().map(|identity| {
                    aura_activity_domain::ActivityResourceTarget::VerifiedObject {
                        identity: identity.clone(),
                        ancestors: Vec::new(),
                        subtree: directory,
                    }
                }),
            }
        })
        .collect()
}
