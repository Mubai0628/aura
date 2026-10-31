//! The parts of the Agent activity module that read Core's own state: the execution decorator,
//! the Shell worker's foreground observation, and the projection of an Agent call's resources
//! and parameters from its `CallContext`. The rest of the module is
//! `aura_core_activity::agent_activity`, mounted here under its old names.
pub(crate) mod foreground;

use crate::agent_execution::CallContext;
use aura_activity_domain::{
    ActivityParameter, ActivityParameterAllowlist, ActivityResource, ActivityResourceRelation,
    ActivityResourceTarget, ActivityToolGroup,
};
pub use aura_core_activity::agent_activity::*;

/// The resource relations a real Agent call publishes.
///
/// It reads the **already-verified** resources from the secured operation. It does
/// not resolve a path, open a file or consult a journal to learn them; the caller
/// already holds them.
#[must_use]
pub(crate) fn project_resources(
    call: &CallContext,
    group: ActivityToolGroup,
) -> Vec<ActivityResource> {
    if group != ActivityToolGroup::Shell
        && let Ok(observed) = call.observed_resources.try_lock()
        && !observed.is_empty()
    {
        return observed.clone();
    }
    if group == ActivityToolGroup::Shell {
        // A Shell's write set is not provable from its cwd, its script or the model's
        // own declaration. It is published as unknown, in the honest sense: not
        // "we did not look", but "this cannot be proven here". Its cwd is always a
        // workspace folder, so the set is taken to stay in the workspace unless the
        // script names places outside it.
        let folder = match &call.validated.call.input {
            aura_tool_contract::NormalizedToolCallInputStateV1::Ready { arguments, .. } => {
                arguments.get("working_directory").and_then(serde_json::Value::as_str)
            }
            aura_tool_contract::NormalizedToolCallInputStateV1::InvalidInput { .. } => None,
        }
        .filter(|folder| !folder.is_empty())
        .unwrap_or(".");
        return vec![ActivityResource {
            target: ActivityResourceTarget::Unproven {
                hint: format!("{} write set not provable", call.operation.content.action_kind),
            },
            relation: shell_relation(&call.operation.content.risks),
            display_name: truncate_display(folder),
            display_name_truncated: folder.len()
                > aura_activity_domain::MAX_ACTIVITY_RESOURCE_DISPLAY_BYTES,
        }];
    }
    let relation = if call.operation.content.action_kind.starts_with("memory.") {
        if call.operation.content.capabilities
            == [aura_security_contract::ResolvedCapability::MemoryRead]
        {
            ActivityResourceRelation::Read
        } else {
            ActivityResourceRelation::Write
        }
    } else {
        match group {
            ActivityToolGroup::FileRead => ActivityResourceRelation::Read,
            ActivityToolGroup::FileWrite => ActivityResourceRelation::Write,
            ActivityToolGroup::Web => ActivityResourceRelation::Read,
            ActivityToolGroup::Shell | ActivityToolGroup::Other => {
                ActivityResourceRelation::UnknownWriteSet
            }
        }
    };
    call.operation
        .content
        .resources
        .iter()
        .filter_map(|descriptor| {
            target_of(descriptor).map(|target| (target, display_of(descriptor)))
        })
        .map(|(target, display_name)| ActivityResource {
            target,
            relation,
            display_name,
            display_name_truncated: false,
        })
        .collect()
}

/// The allow-listed key parameters a real Agent call publishes.
///
/// Only keys the group's allowlist names are projected. A path is a legitimate
/// file-tool parameter; a whole script, an environment block or a credential never
/// is, and a key that is not on the allowlist is simply absent rather than being
/// trimmed into something that looks allowed.
#[must_use]
pub(crate) fn project_parameters(
    call: &CallContext,
    group: ActivityToolGroup,
) -> Vec<ActivityParameter> {
    let mut parameters = Vec::new();
    let action = &call.operation.content.action;
    for key in ActivityParameterAllowlist::keys(group) {
        // The `invocation` key is the real execution reference, not a script body.
        if *key == "invocation" {
            parameters.push(ActivityParameter {
                key: (*key).to_owned(),
                value: call.batch.invocation_identity.command_invocation_id.to_string(),
                truncated: false,
            });
            continue;
        }
        if let Some(value) = action.get(*key).and_then(serde_json::Value::as_str)
            && let Some(parameter) = ActivityParameterAllowlist::project(group, key, value)
        {
            parameters.push(parameter);
        }
    }
    parameters
}

pub mod execution;
