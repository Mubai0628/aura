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
#[cfg(test)]
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
pub(super) use aura_core_broker::broker::run::OnChangedFacts;
use aura_core_broker::broker::run::{MainChannel, RunContext};
use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{Revision, SecurityOperationId};
use aura_execution_contract::*;
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::*;
use std::future::Future;

/// Runs one authenticated Main command through the Core Broker
/// (`aura_core_broker::broker::run`), authorized by this Core's live Main channel.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run<F, Fut, T>(
    state: &RuntimeState,
    journal: &std::sync::Arc<BrokerJournal>,
    operation: SecuredOperation,
    key: String,
    invocation: Option<aura_domain_common::CommandInvocationId>,
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
    let channel = || {
        authenticated_configuration_channel_identity(state).map(|channel| MainChannel {
            core_runtime_instance_id: channel.core_runtime_instance_id,
            authenticated_main_runtime_instance_id: channel.authenticated_main_runtime_instance_id,
            authenticated_channel_id: channel.authenticated_channel_id,
        })
    };
    let runtime = state.options.runtime_instance_id;
    let git_read = operation.content.action_kind == "git.workspace.read";
    let context = RunContext {
        journal,
        boundary: &state.sync,
        post_execution: &state.post_execution,
        settlements: &state.settlements,
        channel: &channel,
        descriptor: if git_read { git_descriptor(runtime) } else { descriptor(runtime) },
    };
    aura_core_broker::broker::run::run(
        context, operation, key, invocation, on_changed, inspect, evidence, call,
    )
    .await
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
