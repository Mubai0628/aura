use super::completion::{
    DispatchOutcome, PostExecution, PostExecutionOwners, PublishedResult, Responsibility,
    TailOutcome,
};
use super::journal::BrokerJournal;
use aura_backend_api::BackendFuture;
use aura_backend_core::BrokerPort;
use aura_core_base::blocking_pool::BlockingAdmission;
use aura_core_base::sync_boundary::{SyncBoundary, SyncSection, SyncSubject};
use aura_core_base::{
    append_core_log,
    command_settlement::{self, CommandSettlements},
    ipc_session,
};
use aura_execution_contract::*;
use aura_runtime_contract::IpcErrorEnvelope;
use serde_json::Value;
use std::{future::Future, sync::Arc, sync::Mutex};

/// The Core Broker port: the one place where a synchronous Core step is reached from an
/// async dispatch.
///
/// Every synchronous step this port owns - reading the pre-state facts, writing the
/// post-state evidence, claiming and completing the broker ledger, reconciling an attempt -
/// runs on the bounded [`SyncBoundary`] instead of on the reactor. The port itself stays
/// async and keeps the order the ledger depends on: the claim happens before the handler,
/// the evidence and the completion after it, and none of them is skipped, reordered or
/// re-implemented because the work moved to a blocking thread.
///
/// The closures are owned (`Arc<dyn Fn ...>`) rather than borrowed. That is what makes the
/// move possible at all, and it is deliberate: the inputs a step needs are built by the
/// route that knows them, so nothing has to be reconstructed on the blocking thread.
///
/// WHAT THIS PORT NO LONGER DOES ITSELF. The two steps after the handler are not awaited by the
/// caller's future any more. The completion responsibility was claimed before this dispatch
/// (`broker::run`), the port takes it immediately before entering the handler, and the whole
/// post-execution tail is handed to [`PostExecution`]. The caller keeps whichever ending its own
/// wait reached; the tail keeps the invocation. See `broker/completion.rs` for why.
pub struct CallPort<'a, F, T> {
    pub journal: Arc<BrokerJournal>,
    pub call: Mutex<Option<F>>,
    pub inspect: super::OwnedInspect,
    pub evidence: super::OwnedEvidence<T>,
    /// Where the route reads this dispatch's single result from. The tail writes it exactly once,
    /// whether or not the caller was still waiting for it.
    pub output: PublishedResult<T>,
    pub live: &'a (dyn Fn() -> anyhow::Result<()> + Send + Sync),
    pub boundary: SyncBoundary,
    pub subject: SyncSubject,
    /// The bounded post-execution owner: the tail's record and its regression injections.
    pub post_execution: PostExecutionOwners,
    /// The completion owner of the invocation this dispatch belongs to.
    pub settlements: CommandSettlements,
    /// The responsibility `broker::run` claimed before the handler. The port takes it here, so
    /// "who answers for this result" is settled before the business can run.
    pub responsibility: Arc<Mutex<Option<Responsibility>>>,
}

pub(super) fn rejected(error: impl std::fmt::Display) -> BackendError {
    BackendError::DispatchRejected { reason: error.to_string() }
}

/// Runs one synchronous Core step on the bounded boundary and turns its three outcomes
/// into the two answers a dispatch may legally give.
///
/// A refusal means the step never ran, and a step that never ran must not be reported as
/// having an effect. An `Unknown` means the step started and may still be running; it is
/// never reported as "not executed", and the caller is told so with the attempt it belongs
/// to when the port knows one.
async fn bounded<T, F>(
    boundary: &SyncBoundary,
    section: SyncSection,
    subject: &SyncSubject,
    step: &'static str,
    unknown: impl FnOnce() -> BackendError,
    work: F,
) -> Result<T, BackendError>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    match boundary.run(section, subject.clone(), work).await {
        BlockingAdmission::Completed(value) => value.map_err(rejected),
        BlockingAdmission::Refused { waited_ms } => Err(rejected(format!(
            "the Core could not run its {step} step: no synchronous worker was free after {waited_ms} ms, so it did not run"
        ))),
        BlockingAdmission::Unknown { waited_ms: _ } => Err(unknown()),
    }
}

/// How one synchronous step that runs **after** the handler ended is classified, in one place.
///
/// WHY THIS IS NOT `bounded`. `bounded` maps a refusal to `DispatchRejected` and an unanswered
/// step to the caller's own `unknown` error. That is right for the steps that run *before* the
/// handler: nothing has happened yet, so "rejected" is a true statement about the command. It is
/// exactly wrong after the handler: by then the side effect may really exist, and reporting it as
/// a pre-execution rejection is the reinterpretation the S6 requirement forbids. Both endings of
/// a post-execution step are therefore statements about *evidence*, never about dispatch.
///
/// The steps themselves are no longer run here: `broker::completion::PostExecution` owns them, so
/// a step that outlives this caller's wait keeps running under its own owner instead of being
/// dropped with this future.
impl<F, Fut, T> BrokerPort for CallPort<'_, F, T>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, IpcErrorEnvelope>> + Send + 'static,
    T: DispatchOutcome + Send + Sync + 'static,
{
    fn inspect(&self, _: &SecuredOperation) -> BackendFuture<'_, Value> {
        Box::pin(async move {
            (self.live)().map_err(rejected)?;
            let inspect = Arc::clone(&self.inspect);
            bounded(
                &self.boundary,
                SyncSection::Inspect,
                &self.subject,
                "pre-state check",
                || rejected("the Core's pre-state check did not answer inside its budget"),
                move || inspect(),
            )
            .await
        })
    }

    fn claim(&self, dispatch: &AuthorizedDispatch) -> BackendFuture<'_, ()> {
        let request = dispatch.request().clone();
        let attempt = request.binding.attempt.clone();
        Box::pin(async move {
            (self.live)().map_err(rejected)?;
            let journal = Arc::clone(&self.journal);
            let claimed = request.clone();
            bounded(
                &self.boundary,
                SyncSection::Ledger,
                &self.subject,
                "ledger claim",
                || BackendError::DispatchUnknown { attempt: attempt.clone() },
                move || journal.claim(&claimed),
            )
            .await
        })
    }

    fn invoke(&self, request: &DispatchRequest) -> BackendFuture<'_, ()> {
        let request = request.clone();
        Box::pin(async move {
            (self.live)().map_err(rejected)?;
            let call = self
                .call
                .lock()
                .map_err(rejected)?
                .take()
                .ok_or_else(|| rejected("call already taken"))?;
            // THE RESPONSIBILITY IS TAKEN BEFORE THE HANDLER. `broker::run` claimed it before this
            // dispatch was handed to the backend, so from here the invocation already has an owner
            // that can answer for its result no matter who stops waiting.
            let correlation = request.operation.content.identity.correlation_id;
            let method = request.operation.content.action_kind.clone();
            let responsibility = self
                .responsibility
                .lock()
                .map_err(rejected)?
                .take()
                .map(Ok)
                .unwrap_or_else(|| self.post_execution.unclaimed(correlation, &method))
                .map_err(|error| rejected(error.message))?;
            let attempt = request.binding.attempt.clone();
            let ticket = responsibility.ticket();
            let waiter =
                command_settlement::SettlementWaiter::new(self.settlements.clone(), ticket.clone());
            let owners = self.post_execution.clone();
            let settlements = self.settlements.clone();
            let evidence = Arc::clone(&self.evidence);
            let journal = Arc::clone(&self.journal);
            let boundary = self.boundary.clone();
            let budget = boundary.budget();
            let subject = self.subject.clone();
            let published = Arc::clone(&self.output);
            let work_attempt = attempt.clone();
            let task_ticket = ticket.clone();
            // The handler and both post-execution steps belong to this bounded owner. An IPC
            // shutdown may abort this waiter; it cannot abort or discard already admitted work.
            let resource_wait = ipc_session::current_resource_wait_handler();
            let mut work = tokio::spawn(async move {
                let task =
                    command_settlement::SettlementTask::new(settlements.clone(), task_ticket);
                let output = ipc_session::with_resource_wait_handler(resource_wait, call()).await;
                let shared = Arc::new(output);
                let tail = PostExecution::new(
                    owners,
                    settlements,
                    responsibility,
                    work_attempt,
                    Arc::new(request),
                    Arc::clone(&shared),
                    evidence,
                    journal,
                    boundary,
                    subject,
                    published,
                );
                let outcome = tail.run_within_caller_wait().await;
                drop(shared);
                task.finished();
                outcome
            });
            let outcome = match tokio::time::timeout(budget, &mut work).await {
                Ok(Ok(outcome)) => outcome,
                _ => TailOutcome::Continued,
            };
            match outcome {
                TailOutcome::Finished => {
                    // The caller received the task's own answer: the slot is given back so the
                    // caller's reply is what the ledger records. A stored ending is discarded with
                    // it on purpose, because the caller holds the same value.
                    waiter.received();
                    Ok(())
                }
                TailOutcome::Continued => {
                    // The caller stopped waiting. The tail keeps the invocation, the permit and
                    // the responsibility; this dispatch's end is not known to this caller.
                    if let Some(ticket) = &ticket {
                        self.settlements.caller_departed(ticket);
                    }
                    append_core_log(
                        "broker.post_execution_caller_departed",
                        serde_json::json!({
                            "operation_id": attempt.operation_id,
                            "ordinal": attempt.ordinal.get(),
                            "correlation_id": correlation,
                            "outcome": "unknown",
                        }),
                    );
                    Err(BackendError::DispatchUnknown { attempt })
                }
            }
        })
    }

    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        Box::pin(async move {
            let journal = Arc::clone(&self.journal);
            let verifying = handle.clone();
            bounded(
                &self.boundary,
                SyncSection::JournalRead,
                &self.subject,
                "handle verification",
                || rejected("the Core's handle verification did not answer inside its budget"),
                move || journal.verify_handle(&verifying),
            )
            .await?;
            let journal = Arc::clone(&self.journal);
            let attempt = handle.attempt.clone();
            let reconciled = bounded(
                &self.boundary,
                SyncSection::JournalRead,
                &self.subject,
                "attempt reconciliation",
                || rejected("the Core's attempt reconciliation did not answer inside its budget"),
                move || journal.reconcile(attempt),
            )
            .await?;
            let termination = match reconciled {
                ReconcileResult::Completed { result } => result.termination,
                _ => TerminationEvidence::Unknown,
            };
            Ok(CancelReceipt { handle, termination })
        })
    }

    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        Box::pin(async move {
            let journal = Arc::clone(&self.journal);
            bounded(
                &self.boundary,
                SyncSection::JournalRead,
                &self.subject,
                "attempt reconciliation",
                || rejected("the Core's attempt reconciliation did not answer inside its budget"),
                move || journal.reconcile(attempt),
            )
            .await
        })
    }
}
