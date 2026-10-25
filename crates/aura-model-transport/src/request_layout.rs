//! Stable wire prefixes without changing immutable instruction/source facts.
use aura_domain_common::ContentHash;
use aura_model_domain::{
    LogicalInstructionKind, LogicalModelRequest, ModelExecutionSnapshot, ProviderToolNameBinding,
};
use aura_tool_contract::ToolSchemaSnapshotV1;

/// OpenAI's own API sends requests that
/// share a key to the same prompt cache. The key names the stable instructions, so it changes when
/// they do. Other endpoints are never sent it: a field a gateway does not know can fail every
/// request, and the transport removes no field (its compatibility whitelist is empty).
#[must_use]
pub fn openai_prompt_cache_key(
    snapshot: &ModelExecutionSnapshot,
    layout: &InstructionLayout,
) -> Option<String> {
    let url = url::Url::parse(&snapshot.resolved_request_url).ok()?;
    (url.scheme() == "https" && url.host_str() == Some("api.openai.com")).then(|| {
        let hash = ContentHash::sha256(layout.stable.as_bytes());
        format!("aura-{}", &hash.hex[..32])
    })
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
