//! One authenticated Main command through the Core Broker, from its live-channel check to the
//! result its handler published.
use super::completion;
use super::journal::BrokerJournal;
use super::{OwnedEvidence, OwnedInspect, port};
use aura_backend_api::ExecutionBackend;
use aura_backend_core::{CoreBroker, FACTS_CHANGED};
use aura_core_base::append_core_log;
use aura_core_base::blocking_pool::BlockingAdmission;
use aura_core_base::command_settlement::{CommandSettlements, outcome_unknown_envelope};
use aura_core_base::envelope::error_envelope;
use aura_core_base::sync_boundary::{SyncBoundary, SyncSection, SyncSubject};
use aura_domain_common::{
    CommandInvocationId, CorrelationId, ErrorCategory, ErrorCode, IpcChannelId, Revision,
    RuntimeInstanceId, SecurityOperationId, UtcTimestamp,
};
use aura_execution_contract::*;
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::*;
use std::time::Duration;
use std::{future::Future, sync::Mutex};

/// The waits before each retry of a command whose facts changed: half a second in all.
const RETRY_DELAYS: [Duration; 3] =
    [Duration::from_millis(50), Duration::from_millis(150), Duration::from_millis(300)];

/// The authenticated Main channel a UI command is authorized by: the Core that admitted it, the
/// Main process that authenticated, and the channel between them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MainChannel {
    pub core_runtime_instance_id: RuntimeInstanceId,
    pub authenticated_main_runtime_instance_id: RuntimeInstanceId,
    pub authenticated_channel_id: IpcChannelId,
}

/// What a command does when the facts it was prepared on changed before it started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnChangedFacts {
    /// The refusal is the command's answer.
    Refuse,
    /// The command is prepared, authorized and reserved again as a new Operation, at most three
    /// times within half a second: the change that refused it is another command's write, which
    /// takes milliseconds.
    Retry,
}

/// What a UI command runs against.
pub struct RunContext<'a> {
    pub journal: &'a std::sync::Arc<BrokerJournal>,
    pub boundary: &'a SyncBoundary,
    pub post_execution: &'a completion::PostExecutionOwners,
    pub settlements: &'a CommandSettlements,
    /// Reads the authenticated Main channel as it is now, and fails while there is none.
    pub channel: &'a (dyn Fn() -> anyhow::Result<MainChannel> + Send + Sync),
    /// The backend this command is prepared and dispatched on.
    pub descriptor: BackendDescriptor,
}

/// Runs one authenticated Main command through the Core Broker.
///
/// The command's own admission - the live-channel check, the operation identity, the
/// backend requirement, the authorization decision, the command claim - is unchanged, and
/// so is what the caller gets back. What changed is *where* the steps that cannot become
/// asynchronous are allowed to block: `inspect`, the ledger claim, the evidence step and
/// the ledger completion all run on the bounded synchronous boundary, so a slow file hash
/// or a storage lock wait occupies a boundary worker instead of the reactor that also has
/// to send the heartbeat, answer the control lane and serve every other connection.
///
/// The claim is still written before the handler runs and the completion after it; moving
/// where the work blocks does not change the order the ledger depends on.
///
/// WHO OWNS THE POST-EXECUTION TAIL. The completion responsibility for this command is claimed
/// *here*, before the dispatch reaches the backend, so the invariant "a business call is never
/// entered without something that can answer for its result" is decided before the handler rather
/// than after a timeout. The claim is handed to the port through a shared slot; the port takes it
/// immediately before entering the handler, and the tail it builds keeps the invocation, the
/// handler's value and the ledger row whatever the caller's own wait does.
///
/// A COMMAND WHOSE FACTS CHANGED BEFORE IT STARTED. The backend refuses it before its claim, so
/// nothing of it ran. With [`OnChangedFacts::Retry`] it is prepared again as a new Operation under
/// `<key>#retry-<n>`, after 50, 150 and 300 ms, and each refused attempt is recorded as not run;
/// facts that are still changing after the third retry give the retryable `broker.facts_changed`.
#[allow(clippy::too_many_arguments)]
pub async fn run<F, Fut, T>(
    context: RunContext<'_>,
    operation: SecuredOperation,
    key: String,
    invocation: Option<CommandInvocationId>,
    on_changed: OnChangedFacts,
    inspect: OwnedInspect,
    evidence: OwnedEvidence<T>,
    call: F,
) -> Result<T, IpcErrorEnvelope>
where
    F: FnOnce(SecurityOperationId) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, IpcErrorEnvelope>> + Send + 'static,
    T: completion::DispatchOutcome + Send + Sync + 'static,
{
    let RunContext { journal, boundary, post_execution, settlements, channel: current, descriptor } =
        context;
    let correlation = operation.content.identity.correlation_id;
    let subject =
        SyncSubject::new().request(correlation).method(&operation.content.action_kind).workspace(
            operation
                .content
                .identity
                .workspace_id
                .map_or_else(|| "-".to_owned(), |id| id.to_string()),
        );
    let boundary = boundary.clone();
    // The attempts hold a dispatch request across their awaits. Boxed, they do not enlarge the
    // future of every route that awaits this one, which a debug test drives on its thread's stack.
    let perform = Box::pin(async {
        let channel = current()?;
        let now = UtcTimestamp::now();
        let deadline = UtcTimestamp::from_value(now.value() + time::Duration::seconds(30));
        let live = || -> anyhow::Result<()> {
            anyhow::ensure!(current()? == channel, "Main channel changed");
            anyhow::ensure!(UtcTimestamp::now() < deadline, "Broker authorization expired");
            Ok(())
        };
        let output: completion::PublishedResult<T> = std::sync::Arc::new(Mutex::new(None));
        // THE COMPLETION RESPONSIBILITY IS CLAIMED BEFORE ANYTHING OF THIS COMMAND RUNS, and the
        // slot it travels in exists before the port does. A refusal is a definite "it did not
        // run": no handler is entered, no effect exists, and the caller may retry. Claiming it
        // after the handler would leave a window in which a real result exists and nothing could
        // ever answer for it. The slot is filled immediately before the dispatch is executed, so
        // no path between the claim and the handler can leave it claimed without a business call.
        let responsibility_slot: std::sync::Arc<Mutex<Option<completion::Responsibility>>> =
            std::sync::Arc::new(Mutex::new(None));
        let requirement = BackendRequirement {
            allowed_classes: [BackendClass::CoreBroker].into(),
            guarantees: [
                EnforcementGuarantee::AuthorizedReadsOnly,
                EnforcementGuarantee::AuthorizedWritesOnly,
                EnforcementGuarantee::CredentialIsolation,
            ]
            .into(),
        };
        let backend = CoreBroker::new(
            descriptor.clone(),
            port::CallPort {
                journal: std::sync::Arc::clone(journal),
                call: Mutex::new(Some(call)),
                inspect,
                evidence,
                output: std::sync::Arc::clone(&output),
                live: &live,
                boundary: boundary.clone(),
                subject: subject.clone(),
                post_execution: post_execution.clone(),
                settlements: settlements.clone(),
                responsibility: std::sync::Arc::clone(&responsibility_slot),
            },
        )
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        // A refusal before the claim leaves the handler in the port untouched, so a retried command
        // is the same command: a new Operation with its own ledger key, prepared, authorized,
        // reserved and checked again on its own facts.
        let mut operation = operation;
        let mut attempt_key = key.clone();
        let mut retries = 0;
        loop {
            let now = UtcTimestamp::now();
            let operation_id = operation.operation_id;
            let plan = backend
                .prepare(PrepareRequest {
                    operation: operation.clone(),
                    requirement: requirement.clone(),
                })
                .await
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let authority = AuthorizationDecision {
                basis: AuthorizationBasis::AuthenticatedMain {
                    principal_id: operation.content.identity.principal_id,
                    core_runtime_instance_id: channel.core_runtime_instance_id,
                    main_runtime_instance_id: channel.authenticated_main_runtime_instance_id,
                    channel_id: channel.authenticated_channel_id,
                },
                // An authenticated Main command is never a reusable consent: it is authorized by
                // the live UI channel for this one Operation.
                scope: AuthorizationScope::exact(
                    operation.fingerprint.clone(),
                    SecurityProfile::Balanced,
                    requirement.clone(),
                    AuthorizationLifetime::Once { operation_id: operation.operation_id },
                ),
                decided_at: now,
                expires_at: deadline,
            };
            let binding = DispatchBinding {
                attempt: AttemptRef {
                    operation_id: operation.operation_id,
                    ordinal: std::num::NonZeroU32::MIN,
                },
                operation_fingerprint: operation.fingerprint.clone(),
                authorization_fingerprint: authority.fingerprint()?,
                plan_fingerprint: plan.fingerprint.clone(),
                cancellation_revision: Revision::INITIAL,
            };
            let dispatch = AuthorizedDispatch::revalidate(
                DispatchRequest { operation, plan, binding },
                &authority,
                &descriptor,
                &CancellationState { revision: Revision::INITIAL, requested: false },
                now,
            )?;
            live()?;
            // The reservation is a real ledger write. It is the last step before the handler and
            // it is bounded like the others, so an unreachable writer refuses the command before
            // anything of it can have run. The dispatch is handed to the boundary and handed
            // back, so the ordering is untouched by where the write is allowed to block.
            let reserving = std::sync::Arc::clone(journal);
            let key_for_reserve = attempt_key.clone();
            let reserved = boundary
                .run(SyncSection::Ledger, subject.clone(), move || {
                    let outcome = reserving.reserve(&dispatch, &authority, &key_for_reserve);
                    (outcome, dispatch)
                })
                .await;
            let dispatch = match reserved {
                BlockingAdmission::Completed((result, dispatch)) => {
                    result?;
                    dispatch
                }
                BlockingAdmission::Refused { waited_ms } => {
                    let mut envelope = error_envelope(
                        correlation,
                        format!(
                            "the Core could not reserve this command's ledger entry: no synchronous worker was free after {waited_ms} ms, so the command did not run"
                        ),
                    );
                    envelope.code = ErrorCode::new("persistence.busy").expect("static code");
                    return Ok(Err(envelope));
                }
                BlockingAdmission::Unknown { waited_ms } => {
                    // The ledger reservation started and did not answer. The answer carries the
                    // contract's recovery field, so it reads as "query this command's outcome, do
                    // not resend" instead of a definite failure. This layer holds the Core
                    // Operation, not the connection's CommandInvocationId, so the identity it can
                    // honestly publish is the correlation id (already on the envelope) and the
                    // operation id; inventing an invocation link here would be a fabricated one.
                    let mut envelope = outcome_unknown_envelope(
                        correlation,
                        None,
                        format!(
                            "the Core's ledger reservation did not answer inside its budget after {waited_ms} ms; the command must not be retried automatically"
                        ),
                    );
                    envelope.details.insert("operation_id".into(), serde_json::json!(operation_id));
                    return Ok(Err(envelope));
                }
            };
            // THE RESPONSIBILITY IS TAKEN HERE, IMMEDIATELY BEFORE THE HANDLER. Every earlier step
            // (channel, authorization, plan, reservation) has already proved it did not run
            // anything, so a refusal now is still a definite not-run. From the next line on, the
            // port and the post-execution tail own this invocation's result.
            let operation_method = dispatch.request().operation.content.action_kind.clone();
            let responsibility =
                match post_execution.claim(settlements, invocation, correlation, &operation_method)
                {
                    Ok(responsibility) => responsibility,
                    Err(envelope) => return Ok(Err(envelope)),
                };
            *responsibility_slot
                .lock()
                .map_err(|_| anyhow::anyhow!("broker responsibility lock"))? = Some(responsibility);
            let _unentered = completion::UnenteredClaim::new(
                std::sync::Arc::clone(&responsibility_slot),
                settlements.clone(),
            );
            let retried = (on_changed == OnChangedFacts::Retry).then(|| dispatch.request().clone());
            let Err(error) = backend.execute(dispatch).await else { break };
            // The port takes the responsibility immediately before entering the handler, so a
            // slot that still holds it proves the handler was never called: the work provably did
            // not run, and its id goes back to "never attempted".
            let untouched = responsibility_slot
                .lock()
                .map_err(|_| anyhow::anyhow!("broker responsibility lock"))?
                .take();
            if let Some(responsibility) = untouched {
                post_execution.never_ran(settlements, &responsibility);
            }
            if let BackendError::DispatchUnknown { attempt } = error {
                // The dispatch really started and its end is not known. Same rule as above: the
                // recovery instruction is a contract value, and the identity this layer owns is
                // the correlation id plus the attempt's operation.
                let mut envelope = outcome_unknown_envelope(
                    correlation,
                    None,
                    "这次操作的结果未知，Aura 不会自动重试；请刷新查看当前状态。".to_owned(),
                );
                envelope
                    .details
                    .insert("operation_id".into(), serde_json::json!(attempt.operation_id));
                return Ok(Err(envelope));
            }
            let refused = retried.filter(|_| {
                matches!(&error, BackendError::DispatchRejected { reason } if reason == FACTS_CHANGED)
            });
            let Some(refused) = refused else {
                return Err(anyhow::anyhow!("{error:?}"));
            };
            let content = refused.operation.content.clone();
            settle_refused(&boundary, &subject, journal, refused).await;
            let Some(delay) = RETRY_DELAYS.get(retries) else {
                return Ok(Err(facts_changed_envelope(correlation)));
            };
            tokio::time::sleep(*delay).await;
            retries += 1;
            append_core_log(
                "broker.retry_after_changed_facts",
                serde_json::json!({
                    "refused_operation_id": operation_id,
                    "retry": retries,
                    "correlation_id": correlation,
                    "method": content.action_kind,
                }),
            );
            operation = SecuredOperation::new(SecurityOperationId::new(), content)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            attempt_key = format!("{key}#retry-{retries}");
        }
        let published = output
            .lock()
            .map_err(|_| anyhow::anyhow!("broker output lock"))?
            .take()
            .ok_or_else(|| anyhow::anyhow!("broker result missing"))?;
        // The handler's own value is moved out of the single published reference. It is unique on
        // every path the tail can take; if an unanswered step were somehow still holding it, the
        // dispatch really did run and is reported as unresolved rather than as done.
        let result = std::sync::Arc::try_unwrap(published).unwrap_or_else(|_| {
            Err(error_envelope(
                correlation,
                "the Core could not collect this dispatch's result after its post-execution steps",
            ))
        });
        Ok::<_, anyhow::Error>(result)
    })
    .await;
    perform.unwrap_or_else(|error| Err(error_envelope(correlation, error)))
}

/// Records a refused attempt as not run. A ledger that cannot take the record leaves the attempt
/// prepared, as a refusal that is not retried does; either way nothing of it ran.
async fn settle_refused(
    boundary: &SyncBoundary,
    subject: &SyncSubject,
    journal: &std::sync::Arc<BrokerJournal>,
    refused: DispatchRequest,
) {
    let journal = std::sync::Arc::clone(journal);
    let operation_id = refused.binding.attempt.operation_id;
    let settled = boundary
        .run(SyncSection::Ledger, subject.clone(), move || {
            journal.reject_unclaimed(&refused, FACTS_CHANGED)
        })
        .await;
    if !matches!(settled, BlockingAdmission::Completed(Ok(()))) {
        append_core_log(
            "broker.refused_attempt_unsettled",
            serde_json::json!({"operation_id": operation_id}),
        );
    }
}

/// The answer once the facts kept changing through every retry. Nothing ran, so trying again is
/// safe.
fn facts_changed_envelope(correlation: CorrelationId) -> IpcErrorEnvelope {
    let mut envelope =
        error_envelope(correlation, "相关内容正在被其他操作修改，这次请求没有执行，请稍后再试。");
    envelope.code = ErrorCode::new("broker.facts_changed").expect("static code");
    envelope.category = ErrorCategory::Conflict;
    envelope.retryable = true;
    envelope
}
