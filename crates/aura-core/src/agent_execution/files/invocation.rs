//! Verified start, file invocation and durable terminal evidence.
//! Reads release mode authority before I/O; writes retain it through atomic publication.
use super::*;
use crate::agent_execution::files;

pub(in crate::agent_execution) enum FsInvocation {
    /// The real file tool ran and reached its own terminal, including a real failure.
    Done,
    /// The complete resource set was refused by a **live short execution conflict**, before any
    /// journal row or side effect. The caller waits outside this worker and re-enters.
    AdmissionWaiting,
}

pub(in crate::agent_execution) fn invoke_file_attempt(
    service: &Arc<AgentExecution>,
    call: &CallContext,
    request: &DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> anyhow::Result<FsInvocation> {
    if files::is_read(call) {
        let checked = service
            .persistence
            .with_session_permission_authority(call.snapshot.session_id, |mode| {
                verify_start(service, call, request, mode)
            })?;
        finish(service, call, request, start_lease, checked)
    } else {
        service.persistence.with_session_permission_authority(call.snapshot.session_id, |mode| {
            let checked = verify_start(service, call, request, mode);
            finish(service, call, request, start_lease, checked)
        })?
    }
}

fn verify_start(
    service: &Arc<AgentExecution>,
    call: &CallContext,
    request: &DispatchRequest,
    mode: &aura_permission_domain::SessionPermissionProjection,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        service.operation_permitted_now(
            &call.operation,
            &call.snapshot,
            mode.mode,
            mode.revision
        )?,
        "agent.permission_withdrawn"
    );
    service.verify_source(call)?;
    {
        let store = service.journal.lock()?;
        aura_persistence_v2::agent::checkpoint::verify_start_operation(
            store.database.connection(),
            request.operation.operation_id,
        )?;
        anyhow::ensure!(
            !aura_persistence_v2::execution::cancellation_requested(
                store.database.connection(),
                &request.binding.attempt,
            )?,
            "agent.cancelled"
        );
    }
    let domain = files::inspect(service, call)?;
    anyhow::ensure!(
        super::fresh_facts::still_hold(
            service,
            call,
            &request.plan.prepared.preparation["domain"],
            &domain
        )?,
        "agent.file_changed"
    );
    if domain.get("read_refusal").is_some() {
        return Err(anyhow::anyhow!("fs.operation_refused"));
    }
    Ok(())
}

fn finish(
    service: &Arc<AgentExecution>,
    call: &CallContext,
    request: &DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
    checked: anyhow::Result<()>,
) -> anyhow::Result<FsInvocation> {
    let mut invoked = false;
    let output = (|| {
        checked?;
        let _read_activity =
            if crate::agent_activity::tool_group_for(&call.operation.content.action_kind)
                == aura_activity_domain::ActivityToolGroup::FileRead
            {
                aura_fs_tools::observe_file_execution(&[])
            } else {
                None
            };
        invoked = true;
        // The ordering lease ends at the real start. Atomic writes still
        // hold the mode authority through publication; reads do not.
        drop(start_lease);
        if call.operation.content.action_kind == "fs.search" {
            let correlation = call.operation.content.identity.correlation_id;
            let cancel = aura_fs_tools::SearchCancel::new();
            aura_fs_tools::register_search(correlation, cancel.clone());
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let poller_stop = std::sync::Arc::clone(&stop);
            let journal = std::sync::Arc::clone(&service.journal);
            let attempt = request.binding.attempt.clone();
            let poller_cancel = cancel.clone();
            let poller = std::thread::spawn(move || {
                while !poller_stop.load(std::sync::atomic::Ordering::SeqCst) {
                    let requested = journal.lock().ok().and_then(|store| {
                        aura_persistence_v2::execution::cancellation_requested(
                            store.database.connection(),
                            &attempt,
                        )
                        .ok()
                    });
                    if requested == Some(true) {
                        poller_cancel.cancel();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            });
            let output = files::invoke(service, call);
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = poller.join();
            output
        } else {
            files::invoke(service, call)
        }
    })();
    // **A live short conflict is a wait, not a tool result.**
    //
    // It is recognised by the typed refusal the file layer reports for "another execution
    // holds this resource right now, no journal row and no effect exists". Reading it out of
    // an error *message* would be the inference the requirement forbids, so it is matched on
    // the typed value. A long editor claim is deliberately not in this set: it keeps its own
    // `DeferredByEditorActivity` semantics, and so does every other real failure, which is
    // why they fall through to the ordinary evidence and completion below.
    if let Err(error) = &output
        && error
            .downcast_ref::<aura_fs_tools::FsToolError>()
            .is_some_and(|error| matches!(error, aura_fs_tools::FsToolError::ResourceBusy(_)))
    {
        // Admission happens inside files::invoke, after the verified start above.
        // ResourceBusy guarantees refusal before any domain journal or file effect;
        // setting `invoked` before entering the service must not turn it into a result.
        return Ok(FsInvocation::AdmissionWaiting);
    }
    let evidence = files::evidence(service, call, request, &output, invoked)?;
    // Acceptance observer: one record per really-entered Tool backend invocation,
    // bound to this run / tool call / attempt, carrying the post-state production really read
    // and the publication stages production really recorded. It is default-off twice over
    // (feature-gated module, and nothing is written unless a control document named a
    // directory) and it changes no return value, no ledger row and no ordering. A "file
    // exists with matching bytes" fact cannot substitute for it: two identical writes are
    // two records.
    #[cfg(feature = "acceptance-barrier")]
    {
        let stages = service
            .persistence
            .workspace_broker_evidence(call.operation.content.identity.correlation_id)
            .ok()
            .and_then(|journal| {
                journal.as_array().map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| entry["state"].as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();
        let stages = stages.iter().map(String::as_str).collect::<Vec<_>>();
        crate::agent_store::acceptance_hold::observe_backend_invocation(
            crate::agent_store::acceptance_hold::BackendExecutionIdentity {
                run: &call.batch.invocation_identity.agent_run_id.to_string(),
                tool_call_id: &call.validated.call.tool_call_id.to_string(),
                attempt_ordinal: u64::from(request.binding.attempt.ordinal.get()),
                action_kind: &call.operation.content.action_kind,
            },
            invoked,
            output.is_ok(),
            &evidence.after,
            &stages,
        );
    }
    // S04-E / D7 acceptance hold point: the domain operation really ran and its post-state
    // evidence was really read, and the ledger completion below - the terminal receipt - has
    // not happened yet. A hold that outlives the production dispatch budget therefore leaves
    // exactly the D7 state: a real side effect, a real operation/attempt/runtime binding, and
    // no terminal receipt. The hold only delays the return; the ledger, the router and the
    // authorisation have all already run and none of them is skipped.
    //
    // Compiled only with the acceptance feature, and with no control document it is one
    // `OnceLock` read and one `Option::is_none`.
    #[cfg(feature = "acceptance-barrier")]
    crate::agent_store::acceptance_hold::hold_tool_post_execution(
        &call.batch.invocation_identity.agent_run_id.to_string(),
        &call.validated.call.tool_call_id.to_string(),
        u64::from(request.binding.attempt.ordinal.get()),
    );
    // SCOPE, stated so the limit of this fix is not read as more than it is: this is the
    // AGENT tool-execution ledger, and its own "outcome unknown" travels as an `anyhow` error
    // whose typed classification is made above this call by the P07 tool-runtime contract.
    // Reading a verdict out of that error's text here would be exactly the message inference
    // the requirement forbids, so the two outcomes this layer really distinguishes are
    // recorded and the tri-state is applied where the typed envelope exists
    // (`broker/port.rs::invoke`).
    service.journal.complete(
        request,
        evidence,
        if output.is_ok() {
            crate::broker::CompletionVerdict::Succeeded
        } else {
            crate::broker::CompletionVerdict::Failed
        },
    )?;
    Ok(FsInvocation::Done)
}
