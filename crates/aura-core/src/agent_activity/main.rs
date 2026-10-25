//! Execution facts for the main publisher.
use super::*;

/// The real owner facts Main already resolved for one ordinary file command (R1).
///
/// These are handed in by the entry that resolved them: `broker::dispatch_files` has already
/// parsed the workspace, the authenticated session and the target paths out of the request
/// before any work starts. Nothing here re-reads a session projection to *guess* an owner, and
/// nothing converts a `CommandInvocationId` into a journal or security-operation identity it
/// does not own.
#[derive(Clone, Debug)]
pub(crate) struct MainFsActivitySubject {
    pub session_id: aura_domain_common::SessionId,
    pub workspace_id: aura_domain_common::WorkspaceId,
    pub runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    /// The real operation identity the broker built for this dispatch.
    pub operation_id: aura_domain_common::SecurityOperationId,
    /// Main's own command invocation, when the request really carries one.
    pub command_invocation_id: Option<aura_domain_common::CommandInvocationId>,
    /// The targets the request names, with the identity A already verified for each.
    pub targets: Vec<MainFsTarget>,
}

/// One already-resolved Main file target: a display path plus, when A verified it, the real
/// physical identity of the object (or the directory it is a scope for).
#[derive(Clone, Debug)]
pub(crate) struct MainFsTarget {
    pub path: String,
    /// The identity A verified, copied verbatim. `None` means "not proven here" and is
    /// published as `Unproven` rather than being passed off as a real object.
    pub target: Option<ActivityResourceTarget>,
}

/// Publishes one Main filesystem activity for an ordinary `fs.*` request (R1).
///
/// WHERE THIS IS CALLED, AND WHY IT MATTERS
/// The original wiring published from the `ToolMutationControlServiceV1` branch, which is the
/// tool-settings/package/verification entry. Ordinary `fs.*` is a Workspace-owner path:
/// `routes/workspace.rs` → `broker/files.rs` → P03/FsToolService, and it never passed that
/// point. The result was the exact inversion R1 names: normal file tools carried no activity,
/// while a *configuration* operation was reported as a Main file write.
///
/// This publisher is therefore called from `broker::dispatch_files`, at the real
/// actual-execution boundary, with the owner facts that entry already holds.
///
/// Main has no Agent identity, so the record carries Main's own real operation identity and
/// **no** Agent run. It never byte-converts an invocation into a `MutationJournalId` and then
/// claims to own one.
pub(crate) fn main_record(
    subject: &MainFsActivitySubject,
    method_name: &str,
) -> aura_activity_domain::ActivityRecord {
    use aura_activity_domain::*;
    let generation = main_generation(subject);
    let execution = ActivityExecutionRef::MainFileMutation {
        operation_id: subject.operation_id,
        command_invocation_id: subject.command_invocation_id,
    };
    let id = ActivityId::derive(ActivityKind::MainFs, &execution, generation);
    let tool_group = tool_group_for(method_name);
    ActivityRecord {
        tool_name: method_name.into(),
        private_source: aura_activity_domain::ActivityPrivateSource::WorkspaceMetadata,
        id,
        kind: ActivityKind::MainFs,
        execution,
        generation,
        tool_id: aura_domain_common::ToolId::from_stable_name(method_name),
        tool_group,
        owner: ActivityOwnerScope {
            session_id: subject.session_id,
            workspace_id: subject.workspace_id,
            // Main has no Agent identity. Nothing synthesises one here.
            agent_run_id: None,
            turn_task_id: None,
            model_step_id: None,
            runtime_instance_id: subject.runtime_instance_id,
        },
        resources: main_resources(subject, tool_group),
        parameters: main_parameters(subject, tool_group),
        started_at: aura_domain_common::UtcTimestamp::now(),
        runtime_instance_id: subject.runtime_instance_id,
        published_at_revision: aura_domain_common::Revision::INITIAL,
    }
}

fn main_generation(subject: &MainFsActivitySubject) -> u32 {
    let digest = aura_domain_common::jcs::canonical_sha256(&(
        "aura.activity.main_generation.v1",
        subject.runtime_instance_id.to_string(),
    ));
    digest
        .map(|hash| u32::from_str_radix(hash.hex().get(..8).unwrap_or("0"), 16).unwrap_or(1).max(1))
        .unwrap_or(1)
}

/// The resource relations Main's own request names.
///
/// R3: where A already verified a physical identity for a target, that identity is copied into
/// the contract so Main, Buffer and Agent agree on one key for one file. A path string is used
/// only for targets with no verified identity, and is then reported as `Unproven` - never
/// passed off as a real volume/file identity.
fn main_resources(
    subject: &MainFsActivitySubject,
    group: ActivityToolGroup,
) -> Vec<ActivityResource> {
    let relation = match group {
        ActivityToolGroup::FileRead => ActivityResourceRelation::Read,
        ActivityToolGroup::FileWrite => ActivityResourceRelation::Write,
        _ => ActivityResourceRelation::UnknownWriteSet,
    };
    let mut resources = Vec::new();
    for target in &subject.targets {
        if resources.len() > aura_activity_domain::MAX_ACTIVITY_RESOURCES {
            break;
        }
        resources.push(ActivityResource {
            target: target
                .target
                .clone()
                .unwrap_or_else(|| ActivityResourceTarget::Unproven { hint: target.path.clone() }),
            relation,
            display_name: truncate_display(&target.path),
            display_name_truncated: target.path.len()
                > aura_activity_domain::MAX_ACTIVITY_RESOURCE_DISPLAY_BYTES,
        });
    }
    if resources.is_empty() {
        resources.push(ActivityResource {
            target: ActivityResourceTarget::Unproven {
                hint: "request carried no usable target".to_owned(),
            },
            relation: ActivityResourceRelation::UnknownWriteSet,
            display_name: "unknown target".to_owned(),
            display_name_truncated: false,
        });
    }
    resources
}

fn main_parameters(
    subject: &MainFsActivitySubject,
    group: ActivityToolGroup,
) -> Vec<ActivityParameter> {
    let mut parameters = Vec::new();
    // The allowlist decides which keys are projected. Main's request is the real invocation, so
    // the first named target is the key-parameter reference a model needs to look the call up.
    for key in ActivityParameterAllowlist::keys(group) {
        if *key == "path"
            && let Some(first) = subject.targets.first()
            && let Some(parameter) = ActivityParameterAllowlist::project(group, key, &first.path)
        {
            parameters.push(parameter);
        }
    }
    parameters
}
