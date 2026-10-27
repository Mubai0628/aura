//! Stable wire prefixes without changing immutable instruction/source facts.
use aura_domain_common::ContentHash;
use aura_model_domain::{
    LogicalInstructionKind, LogicalModelRequest, ModelApiFormat, ModelExecutionSnapshot,
    ProviderToolNameBinding,
};
use aura_tool_contract::ToolSchemaSnapshotV1;

/// Whether the endpoint reaches OpenAI's own prompt cache: OpenAI's API itself, or a gateway on this
/// computer (CLIProxyAPI and the like) in front of it. Another provider's endpoint is answered by
/// that provider's cache, which may not know OpenAI's fields; DeepSeek's cached nearly every step
/// with the request as it was.
#[must_use]
pub fn reaches_openai_cache(snapshot: &ModelExecutionSnapshot) -> bool {
    url::Url::parse(&snapshot.resolved_request_url).is_ok_and(|url| match url.host() {
        Some(url::Host::Domain(host)) => {
            (url.scheme() == "https" && host == "api.openai.com") || host == "localhost"
        }
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    })
}

/// OpenAI's prompt cache sends the requests that share a key to the same cache, so the steps of one
/// conversation, each the one before with more appended, find what the one before left. The key
/// names the stable instructions and the conversation's first item, which stay the same from step
/// to step until a compaction replaces the history. It goes to OpenAI and to a local gateway in
/// front of it (through one without the key, a turn's steps were cached erratically); a Chat
/// Completions endpoint is sent it only at OpenAI itself. Other endpoints may not know the field, a
/// field a gateway does not know can fail every request, and the transport removes no field (its
/// compatibility whitelist is empty).
#[must_use]
pub fn prompt_cache_key(
    snapshot: &ModelExecutionSnapshot,
    request: &LogicalModelRequest,
    layout: &InstructionLayout,
) -> Option<String> {
    let official = url::Url::parse(&snapshot.resolved_request_url)
        .is_ok_and(|url| url.scheme() == "https" && url.host_str() == Some("api.openai.com"));
    let responses = snapshot.api_format == ModelApiFormat::OpenAiResponses;
    if !official && !(responses && reaches_openai_cache(snapshot)) {
        return None;
    }
    let mut basis = layout.stable.as_bytes().to_vec();
    if let Some(first) = request.conversation_items.first() {
        basis.push(b'\n');
        basis.extend(serde_json::to_vec(first).ok()?);
    }
    let hash = ContentHash::sha256(&basis);
    Some(format!("aura-{}", &hash.hex[..32]))
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
