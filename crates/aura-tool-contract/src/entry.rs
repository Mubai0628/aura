//! The unified entry: `tools.call` runs a tool whose definition is not sent with the request.
//!
//! The request's tool list stays the same for a whole turn, so the Provider can reuse its cached
//! prefix; any other tool of the step's frozen registry is called through this one entry. The call
//! is rewritten into a direct call of the tool it names before anything checks it, so admission,
//! approvals, Hooks and the result treat it exactly as that tool's own call. Loading a tool never
//! granted permission, and neither does the entry.
use aura_domain_common::jcs::canonical_sha256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    NormalizedToolCallInputStateV1, NormalizedToolCallV1, ToolDefinitionSnapshotRef,
    ToolRegistryCatalogEntryV1, ToolRegistrySnapshotV1,
};

/// The entry's canonical name.
pub const TOOLS_CALL_NAME: &str = "tools.call";

/// The registry tools themselves and the turn's protocol tools. Each mode and task purpose sends
/// the protocol tools it uses, so none of them is ever reached through the entry.
const NOT_THROUGH_ENTRY: &[&str] = &[
    "tools.",
    "skills.",
    "plan.",
    "goal.",
    "mode.",
    "normal.",
    "query.",
    "task_lineage.",
    "workspace_init.",
];

/// Whether `tools.call` may run the tool with this canonical name.
#[must_use]
pub fn entry_callable(canonical_name: &str) -> bool {
    !NOT_THROUGH_ENTRY.iter().any(|prefix| canonical_name.starts_with(prefix))
}

/// What the model sends to `tools.call`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolCallInput {
    /// The tool's canonical_name from tools.search, for example git.commit (its tool_id works too).
    pub tool: String,
    /// The tool's own arguments, as its input schema in tools.describe asks for them.
    pub arguments: Map<String, Value>,
}

/// What `tools.call` returns: the named tool's own result. The entry adds nothing to it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(transparent)]
pub struct AgentToolCallResult(pub Map<String, Value>);

/// The tool `tools.call` runs for `name` in this snapshot: a catalog entry bound for this step -
/// sent with the request or offered through the entry - outside the protocol namespaces. `name` is
/// its canonical name, its tool_id, or the name the Provider saw (`<stem>__<ulid>`).
#[must_use]
pub fn entry_target<'a>(
    registry: &'a ToolRegistrySnapshotV1,
    name: &str,
) -> Option<&'a ToolRegistryCatalogEntryV1> {
    let name = name.trim();
    let wire_ulid = name.rsplit_once("__").map(|(_, ulid)| ulid);
    let entry = registry.catalog_entries.iter().find(|entry| {
        entry.canonical_name.as_str() == name
            || entry.tool_id.to_string() == name
            || wire_ulid
                .is_some_and(|ulid| entry.tool_id.ulid().to_string().eq_ignore_ascii_case(ulid))
    })?;
    let bound = registry.effective_bindings.iter().chain(&registry.entry_bindings).any(|binding| {
        binding.tool_id == entry.tool_id && binding.tool_version == entry.tool_version
    });
    (bound && entry_callable(entry.canonical_name.as_str())).then_some(entry)
}

/// Whether `call` is bound to this snapshot's `tools.call`.
#[must_use]
pub fn is_entry_call(registry: &ToolRegistrySnapshotV1, call: &NormalizedToolCallV1) -> bool {
    call.definition_ref.as_ref().is_some_and(|reference| {
        reference.registry_snapshot_ref == registry.snapshot_ref
            && registry.catalog_entries.iter().any(|entry| {
                entry.tool_id == reference.tool_id
                    && entry.canonical_name.as_str() == TOOLS_CALL_NAME
            })
    })
}

/// Rewrites every `tools.call` call into the call of the tool it names. Its id, ordinal, purpose
/// and the Provider's envelope hash are kept; the definition becomes the named tool's and the input
/// that tool's own arguments. A call that names no tool [`entry_target`] allows, or whose input is
/// not `{tool, arguments}`, stays bound to `tools.call`, which admission refuses without running
/// anything. Every reader of a step's calls applies this the same way, so history and recovery
/// derive exactly what admission admitted.
pub fn expand_entry_calls(registry: &ToolRegistrySnapshotV1, calls: &mut [NormalizedToolCallV1]) {
    for call in calls.iter_mut() {
        if !is_entry_call(registry, call) {
            continue;
        }
        let NormalizedToolCallInputStateV1::Ready { purpose, arguments, .. } = &call.input else {
            continue;
        };
        let Ok(input) = serde_json::from_value::<AgentToolCallInput>(arguments.clone()) else {
            continue;
        };
        let Some(target) = entry_target(registry, &input.tool) else {
            continue;
        };
        let arguments = Value::Object(input.arguments);
        let Ok(arguments_hash) = canonical_sha256(&arguments) else {
            continue;
        };
        let purpose = purpose.clone();
        call.definition_ref = Some(ToolDefinitionSnapshotRef {
            tool_id: target.tool_id,
            tool_version: target.tool_version.clone(),
            registry_snapshot_ref: registry.snapshot_ref.clone(),
        });
        call.input = NormalizedToolCallInputStateV1::Ready { purpose, arguments, arguments_hash };
    }
}
