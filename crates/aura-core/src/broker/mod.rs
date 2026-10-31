//! Core composition for authenticated UI commands. Agent dispatch uses R04's
//! Policy/Grant guard; this closed adapter never invents a Session or grant for
//! a global settings action. The live IPC channel is its explicit authority.
mod artifact;
pub(crate) use aura_core_broker::broker::completion;
mod context;
pub(crate) mod files;
mod git;
mod http;
use aura_core_broker::broker::journal;

#[cfg(test)]
mod completion_owner_tests;
mod model;
use aura_core_broker::broker::port;
#[cfg(test)]
mod tests;
pub(crate) mod tool_configuration;
mod tools;
pub(crate) use artifact::dispatch_artifact;
pub(crate) use aura_agent_execution::broker::git_descriptor;
pub(crate) use context::dispatch_context;
pub(crate) use files::dispatch_files;
pub(crate) use git::dispatch_git;
pub(crate) use http::dispatch_http;
pub(crate) use journal::BrokerJournal;
#[cfg(test)]
pub(crate) use journal::CompletionVerdict;
pub(crate) use model::dispatch_model;
pub(crate) use tools::dispatch_tools;

pub(super) use aura_core_broker::broker::{OwnedEvidence, OwnedInspect};

use crate::{RuntimeState, authenticated_configuration_channel_identity};
use aura_backend_api::ExecutionBackend;
use aura_backend_core::CoreBroker;
use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{Revision, SecurityOperationId, UtcTimestamp};
use aura_execution_contract::*;
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::*;
use std::{future::Future, sync::Mutex};

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
#[allow(clippy::too_many_arguments)]
pub(super) async fn run<F, Fut, T>(
    state: &RuntimeState,
    journal: &std::sync::Arc<BrokerJournal>,
    operation: SecuredOperation,
    key: String,
    invocation: Option<aura_domain_common::CommandInvocationId>,
    inspect: OwnedInspect,
    evidence: OwnedEvidence<T>,
    call: F,
) -> Result<T, IpcErrorEnvelope>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, IpcErrorEnvelope>> + Send + 'static,
    T: completion::DispatchOutcome + Send + Sync + 'static,
{
    let correlation = operation.content.identity.correlation_id;
    let operation_id = operation.operation_id;
    let subject = aura_core_base::sync_boundary::SyncSubject::new()
        .request(correlation)
        .method(&operation.content.action_kind)
        .workspace(
            operation
                .content
                .identity
                .workspace_id
                .map_or_else(|| "-".to_owned(), |id| id.to_string()),
        );
    let boundary = state.sync.clone();
    let perform = async {
        let channel = authenticated_configuration_channel_identity(state)?;
        let now = UtcTimestamp::now();
        let deadline = UtcTimestamp::from_value(now.value() + time::Duration::seconds(30));
        let live = || -> anyhow::Result<()> {
            anyhow::ensure!(
                authenticated_configuration_channel_identity(state)? == channel,
                "Main channel changed"
            );
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
        let git_read = operation.content.action_kind == "git.workspace.read";
        let descriptor = if git_read { git_descriptor(state.options.runtime_instance_id) } else { descriptor(state.options.runtime_instance_id) };
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
                post_execution: state.post_execution.clone(),
                settlements: state.settlements.clone(),
                responsibility: std::sync::Arc::clone(&responsibility_slot),
            },
        )
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
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
            scope: AuthorizationScope {
                operation_fingerprint: operation.fingerprint.clone(),
                profile: SecurityProfile::Balanced,
                backend_requirement: requirement,
                lifetime: AuthorizationLifetime::Once { operation_id: operation.operation_id },
                // An authenticated Main command is never a reusable consent: it
                // is authorized by the live UI channel for this one Operation.
                consent: None,
                // Nor is it a reusable file consent, for the same reason: the user's own
                // command is authorized by the channel they are using right now.
                network_consent: None,
                capability_limit: None,
                memory_consent: None,
                mcp_consent: None,
                chrome_consent: None,
                chrome_site_consent: None,
                file_consent: None,
            },
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
        let key_for_reserve = key.clone();
        let reserved = boundary
            .run(
                aura_core_base::sync_boundary::SyncSection::Ledger,
                subject.clone(),
                move || {
                    let outcome = reserving.reserve(&dispatch, &authority, &key_for_reserve);
                    (outcome, dispatch)
                },
            )
            .await;
        let dispatch = match reserved {
            aura_core_base::blocking_pool::BlockingAdmission::Completed((result, dispatch)) => {
                result?;
                dispatch
            }
            aura_core_base::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                let mut envelope = error_envelope(
                    correlation,
                    format!(
                        "the Core could not reserve this command's ledger entry: no synchronous worker was free after {waited_ms} ms, so the command did not run"
                    ),
                );
                envelope.code =
                    aura_domain_common::ErrorCode::new("persistence.busy").expect("static code");
                return Ok(Err(envelope));
            }
            aura_core_base::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                // The ledger reservation started and did not answer. The answer carries the
                // contract's recovery field, so it reads as "query this command's outcome, do
                // not resend" instead of a definite failure. This layer holds the Core
                // Operation, not the connection's CommandInvocationId, so the identity it can
                // honestly publish is the correlation id (already on the envelope) and the
                // operation id; inventing an invocation link here would be a fabricated one.
                let mut envelope = aura_core_base::command_settlement::outcome_unknown_envelope(
                    correlation,
                    None,
                    format!(
                        "the Core's ledger reservation did not answer inside its budget after {waited_ms} ms; the command must not be retried automatically"
                    ),
                );
                envelope
                    .details
                    .insert("operation_id".into(), serde_json::json!(operation_id));
                return Ok(Err(envelope));
            }
        };
        // THE RESPONSIBILITY IS TAKEN HERE, IMMEDIATELY BEFORE THE HANDLER. Every earlier step
        // (channel, authorization, plan, reservation) has already proved it did not run anything,
        // so a refusal now is still a definite not-run. From the next line on, the port and the
        // post-execution tail own this invocation's result.
        let operation_method = dispatch.request().operation.content.action_kind.clone();
        let responsibility = match state.post_execution.claim(
            &state.settlements,
            invocation,
            correlation,
            &operation_method,
        ) {
            Ok(responsibility) => responsibility,
            Err(envelope) => return Ok(Err(envelope)),
        };
        *responsibility_slot
            .lock()
            .map_err(|_| anyhow::anyhow!("broker responsibility lock"))? = Some(responsibility);
        let _unentered = completion::UnenteredClaim::new(
            std::sync::Arc::clone(&responsibility_slot),
            state.settlements.clone(),
        );
        if let Err(error) = backend.execute(dispatch).await {
            // The port takes the responsibility immediately before entering the handler, so a
            // slot that still holds it proves the handler was never called: the work provably did
            // not run, and its id goes back to "never attempted".
            let untouched = responsibility_slot
                .lock()
                .map_err(|_| anyhow::anyhow!("broker responsibility lock"))?
                .take();
            if let Some(responsibility) = untouched {
                state.post_execution.never_ran(&state.settlements, &responsibility);
            }
            if let BackendError::DispatchUnknown { attempt } = error {
                // The dispatch really started and its end is not known. Same rule as above: the
                // recovery instruction is a contract value, and the identity this layer owns is
                // the correlation id plus the attempt's operation.
                let mut envelope = aura_core_base::command_settlement::outcome_unknown_envelope(
                    correlation,
                    None,
                    "这次操作的结果未知，Aura 不会自动重试；请刷新查看当前状态。".to_owned(),
                );
                envelope
                    .details
                    .insert("operation_id".into(), serde_json::json!(attempt.operation_id));
                return Ok(Err(envelope));
            }
            return Err(anyhow::anyhow!("{error:?}"));
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
    }
    .await;
    perform.unwrap_or_else(|error| Err(error_envelope(correlation, error)))
}

pub(super) use aura_core_broker::broker::descriptor;

pub(super) fn operation(
    method: &str,
    params: &serde_json::Value,
    correlation: aura_domain_common::CorrelationId,
    capabilities: Vec<ResolvedCapability>,
    resources: Vec<ResourceDescriptor>,
) -> anyhow::Result<SecuredOperation> {
    Ok(SecuredOperation::new(
        SecurityOperationId::new(),
        OperationContent {
            compatibility: Default::default(),
            network: OperationNetwork::None,
            identity: InvocationIdentity {
                principal_id: aura_domain_common::PrincipalId::new(),
                actor: InvocationActor::User,
                workspace_id: params
                    .get("workspace_id")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
                session_id: params
                    .get("session_id")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
                turn_task_id: None,
                agent_run_id: None,
                parent_agent_run_id: None,
                correlation_id: correlation,
                permission_snapshot_id: None,
            },
            source: SecuritySourceRef::UserInterface,
            source_revision_fingerprint: None,
            capabilities,
            resources,
            risks: vec![],
            credentials: vec![],
            purpose: None,
            // UI argument bytes are held by the closed Core port for this call.
            // Persist the exact canonical reference before domain validation, never
            // unvalidated config/header/credential strings. Recovery cannot replay
            // these calls and reconciles the owning domain's receipt instead.
            action_kind: method.into(),
            action_revision: Revision::INITIAL,
            action: serde_json::json!({"request_fingerprint":canonical_sha256(params)?}),
        },
    )?)
}

pub(super) use aura_core_base::envelope::error_envelope;
