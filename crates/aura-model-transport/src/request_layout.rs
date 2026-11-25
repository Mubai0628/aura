//! Stable wire prefixes without changing immutable instruction/source facts.
use aura_domain_common::ContentHash;
use aura_model_domain::{
    CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION, LogicalInstructionKind, LogicalModelRequest,
    ModelExecutionSnapshot,
};
use aura_provider_profile::{RequestRules, RoutingKey};

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

/// A state entry may be inserted between complete exchanges on the Responses routes that
/// already lead with a developer message. Archived history has different item boundaries.
#[must_use]
pub fn supports_task_state_replay(
    snapshot: &ModelExecutionSnapshot,
    envelope: &aura_model_domain::CompiledInstructionEnvelope,
    tools: &aura_tool_contract::ToolSchemaSetSnapshotV1,
) -> bool {
    snapshot.api_format == aura_model_domain::ModelApiFormat::OpenAiResponses
        && envelope.serializer_version == CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION
        && state_before_conversation(snapshot)
        && !crate::uses_deepseek_history_archive(snapshot, envelope, tools)
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
    if let Some(background) = &layout.background {
        basis.push(b'\n');
        basis.extend_from_slice(background.as_bytes());
    }
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
    pub background: Option<String>,
    pub checkpoint: bool,
}

impl InstructionLayout {
    /// The leading instruction for protocols that place state next to the fixed rules.
    #[must_use]
    pub fn including_checkpoint(&self) -> String {
        if self.checkpoint {
            format!("{}\n{}", self.stable, self.current)
        } else {
            self.stable.clone()
        }
    }
}

#[must_use]
pub fn instruction_layout(request: &LogicalModelRequest) -> InstructionLayout {
    instruction_envelope_layout(&request.instruction_envelope)
}

#[must_use]
pub fn instruction_envelope_layout(
    envelope: &aura_model_domain::CompiledInstructionEnvelope,
) -> InstructionLayout {
    let checkpoint = envelope.serializer_version == CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION;
    let mut stable = Vec::new();
    let mut current = Vec::new();
    let mut background = None;
    for block in &envelope.ordered_blocks {
        match block.kind {
            LogicalInstructionKind::TaskState => current.push(block.content.as_str()),
            // New requests use the checkpoint envelope even when they read an old summary.
            // Keep the layout of a legacy frozen request stable when it is compiled again.
            LogicalInstructionKind::HistoricalSnapshot if checkpoint => {
                background = conversation_background(&block.content);
            }
            _ => stable.push(block.content.as_str()),
        }
    }
    InstructionLayout {
        stable: stable.join("\n"),
        current: current.join("\n"),
        background,
        checkpoint,
    }
}

fn conversation_background(block: &str) -> Option<String> {
    // The immutable envelope escapes XML for its instruction sections. In conversation
    // history the provider needs the actual wording, including literal ampersands and tags.
    let Some(body) = block
        .strip_prefix("<Historical-Snapshot-Summary>\n")
        .and_then(|s| s.strip_suffix("\n</Historical-Snapshot-Summary>"))
    else {
        return (!block.trim().is_empty()).then(|| block.to_owned());
    };
    if body.trim() == "<Empty reason=\"no-active-content\" />" {
        return None;
    }
    let text = body
        .lines()
        .map(|line| line.strip_prefix("  ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "Historical-Snapshot-Summary\n{}",
        text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
    ))
}

#[cfg(test)]
mod background_tests {
    use super::*;
    #[test]
    fn context_continuity_preserves_literal_user_wording_in_history() {
        let text = "<Historical-Snapshot-Summary>\n  A&amp;B &lt;literal&gt; &amp;lt;\n</Historical-Snapshot-Summary>";
        assert_eq!(
            conversation_background(text).as_deref(),
            Some("Historical-Snapshot-Summary\nA&B <literal> &lt;")
        );
    }
}
