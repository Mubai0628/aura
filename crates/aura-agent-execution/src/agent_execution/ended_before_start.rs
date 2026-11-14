//! A call whose turn was stopped in the moment after it was marked started: when it reached its
//! domain, the call was no longer active, so nothing of it was sent. Its awaiting future went
//! with the turn, so nothing else records its end: it is recorded here as not run, never left
//! with an unknown outcome. So is a file call that waited past its limit for a file another call
//! held: it was refused before it started, and the model reads why.
use super::*;
use aura_backend_api::BackendFuture;
use aura_core_broker::agent_store::{StoreAccess, StoreStep};

/// The outcome of a call's domain; a refusal because the call was no longer active is recorded
/// as the call ending before it started, and the call then has a result to reconcile.
pub(super) async fn settle(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    domain: BackendFuture<'_, ()>,
) -> Result<(), BackendError> {
    let outcome = domain.await;
    let (reason, stopped) = match &outcome {
        Err(BackendError::DispatchRejected { reason }) if not_active(reason) => {
            (reason.clone(), true)
        }
        Err(BackendError::DispatchRejected { reason }) if waited_too_long(reason) => {
            (reason.clone(), false)
        }
        _ => return outcome,
    };
    let writer = Arc::clone(&service);
    let recorded = service
        .store
        .run_critical(
            StoreStep::run(
                call.batch.invocation_identity.agent_run_id,
                "agent.broker.ended_before_start",
            ),
            StoreAccess::Write,
            move || {
                Ok(writer.journal.lock()?.database.unit_of_work(|tx| {
                    let record = if stopped {
                        aura_persistence_v2::agent::recovery::end_before_start
                    } else {
                        aura_persistence_v2::agent::recovery::refused_before_start
                    };
                    record(
                        tx,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        &request,
                        &reason,
                        UtcTimestamp::now(),
                    )
                })?)
            },
        )
        .await;
    // An end that could not be recorded leaves the call as it was: its outcome stays unknown.
    if recorded.is_ok() { Ok(()) } else { outcome }
}

/// Whether a refusal says the call was no longer active when it was to start: the turn that owned
/// it had stopped. Every domain refuses that way before it sends anything.
fn not_active(reason: &str) -> bool {
    reason.split(':').next().is_some_and(|code| code.trim().ends_with(".call_not_active"))
}

/// Whether a refusal says another call held the call's file past the wait limit: refused before
/// anything was read or written.
fn waited_too_long(reason: &str) -> bool {
    reason.split(':').next().is_some_and(|code| code.trim() == "agent.resource_wait_exhausted")
}

#[cfg(test)]
mod tests {
    use super::{not_active, waited_too_long};

    #[test]
    fn only_a_call_that_was_no_longer_active_ended_before_it_started() {
        for reason in [
            "agent.call_not_active",
            "browser.call_not_active: 这次浏览器调用在发给浏览器之前已经结束（回合停止了或调用被取消），什么都没做。",
            "memory.call_not_active: this memory call ended before it started (the turn stopped or the call was cancelled); nothing was read or written",
        ] {
            assert!(not_active(reason), "{reason}");
        }
        for reason in [
            "agent.resource_wait_exhausted",
            "terminal.ended: 终端已结束",
            "browser.page_closed: call_not_active is only named here",
            "",
        ] {
            assert!(!not_active(reason), "{reason}");
        }
    }

    #[test]
    fn a_wait_that_ran_out_is_a_refusal_before_the_start() {
        assert!(waited_too_long("agent.resource_wait_exhausted"));
        assert!(waited_too_long(
            "agent.resource_wait_exhausted: another execution still held this file resource"
        ));
        for reason in ["agent.call_not_active", "fs.resource_busy", "agent.resource_changed", ""] {
            assert!(!waited_too_long(reason), "{reason}");
        }
    }
}
