//! Stable wire prefixes without changing immutable instruction/source facts.
use aura_domain_common::ContentHash;
use aura_model_domain::{
    LogicalInstructionKind, LogicalModelRequest, ModelExecutionSnapshot, ProviderToolNameBinding,
};
use aura_provider_profile::{RequestRules, RoutingKey};
use aura_tool_contract::ToolSchemaSnapshotV1;

/// What the profile of the request's host asks of the request beyond the protocol.
fn profile_rules(snapshot: &ModelExecutionSnapshot) -> RequestRules {
    url::Url::parse(&snapshot.resolved_request_url)
        .map(|url| aura_provider_profile::request_rules(&url, snapshot.api_format))
        .unwrap_or_default()
}

/// Whether the request's profile puts the task's current state before the conversation.
#[must_use]
pub fn state_before_conversation(snapshot: &ModelExecutionSnapshot) -> bool {
    profile_rules(snapshot).state_before_conversation
}

/// Whether the request's profile marks cache breakpoints in the body.
#[must_use]
pub fn cache_breakpoints(snapshot: &ModelExecutionSnapshot) -> bool {
    profile_rules(snapshot).cache_breakpoints
}

/// Whether the request's profile asks the Provider to keep neither it nor its reply.
#[must_use]
pub fn store_off(snapshot: &ModelExecutionSnapshot) -> bool {
    profile_rules(snapshot).store_off
}

/// Whether the request asks for the model's reasoning encrypted: where its profile does, for a
/// model that reasons, as its catalog lists thinking levels or one is sent. A model that does not
/// reason refuses the field.
#[must_use]
pub fn encrypted_reasoning(snapshot: &ModelExecutionSnapshot) -> bool {
    let options = &snapshot.request_options;
    profile_rules(snapshot).encrypted_reasoning
        && (options.reasoning_by_effort || options.reasoning_effort.is_some())
}

/// A body field the Provider routes the steps of a conversation by.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingKeyField {
    pub name: String,
    pub value: String,
}

/// The body field the request's profile routes a conversation's steps by, when it asks for one.
/// The value names the stable instructions and the conversation's first item, which stay the same
/// from step to step until a compaction replaces the history, so the steps of one conversation,
/// each the one before with more appended, find what the one before left. A request whose profile
/// asks for no field is sent none: a field a gateway does not know can fail every request, and the
/// transport removes no field (its compatibility whitelist is empty).
#[must_use]
pub fn routing_key_field(
    snapshot: &ModelExecutionSnapshot,
    request: &LogicalModelRequest,
    layout: &InstructionLayout,
) -> Option<RoutingKeyField> {
    let Some(RoutingKey::PromptPrefixField { name }) = profile_rules(snapshot).routing_key else {
        return None;
    };
    let mut basis = layout.stable.as_bytes().to_vec();
    if let Some(first) = request.conversation_items.first() {
        basis.push(b'\n');
        basis.extend(serde_json::to_vec(first).ok()?);
    }
    let hash = ContentHash::sha256(&basis);
    Some(RoutingKeyField { name, value: format!("aura-{}", &hash.hex[..32]) })
}

pub struct InstructionLayout {
    pub stable: String,
    pub current: String,
}

#[must_use]
pub fn instruction_layout(request: &LogicalModelRequest) -> InstructionLayout {
    let mut stable = Vec::new();
    let mut current = Vec::new();
    for block in &request.instruction_envelope.ordered_blocks {
        if block.kind == LogicalInstructionKind::TaskState {
            current.push(block.content.as_str());
        } else {
            stable.push(block.content.as_str());
        }
    }
    InstructionLayout { stable: stable.join("\n"), current: current.join("\n") }
}

#[must_use]
pub fn ordered_tool_bindings<'a>(
    request: &'a LogicalModelRequest,
    bindings: &'a [ProviderToolNameBinding],
) -> Vec<(&'a ToolSchemaSnapshotV1, &'a ProviderToolNameBinding)> {
    let mut pairs = request.tool_schema_set.tools.iter().zip(bindings).collect::<Vec<_>>();
    pairs.sort_by_key(|(tool, _)| (!request.fixed_tool_ids.contains(&tool.tool_id), tool.tool_id));
    pairs
}
