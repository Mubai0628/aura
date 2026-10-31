use std::{collections::BTreeSet, sync::Arc};

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ArtifactId, ContentHash, ContextProjectionSnapshotId, EventId, FileReferenceId,
    ModelExecutionSnapshotId, ModelStepId,
};
use aura_domain_common::{ToolCallId, ToolId};
use aura_tool_contract::{ToolDefinitionSnapshotRef, ToolSchemaSetSnapshotV1};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::ProviderContinuationState;

pub const PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V1: u32 = 1;
pub const PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V2: u32 = 2;
/// The tool's own fields at the top level beside `aura_purpose`; every new binding uses it.
pub const PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V3: u32 = 3;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum LogicalInstructionKind {
    Core,
    Permission,
    ToolSummary,
    WorkspaceAgents,
    HistoricalSnapshot,
    AgentMode,
    Hook,
    TaskState,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalInstructionBlock {
    pub kind: LogicalInstructionKind,
    pub content: String,
    pub content_hash: ContentHash,
    pub source_event_ids: Vec<EventId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledInstructionEnvelope {
    pub serializer_version: u32,
    pub ordered_blocks: Vec<LogicalInstructionBlock>,
    pub canonical_text: String,
    pub instruction_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalHistoricalToolCall {
    pub tool_call_id: ToolCallId,
    /// Null only for a call whose Provider name was absent from the frozen request.
    #[serde(deserialize_with = "aura_domain_common::nullable::required")]
    #[schemars(with = "aura_domain_common::nullable::Required<ToolId>", required)]
    pub tool_id: Option<ToolId>,
    pub canonical_name: String,
    pub arguments_json: String,
    pub arguments_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LogicalConversationItem {
    User {
        source_event_id: EventId,
        content: String,
        content_hash: ContentHash,
    },
    Assistant {
        source_event_id: EventId,
        content: Option<String>,
        content_hash: Option<ContentHash>,
        tool_calls: Vec<LogicalHistoricalToolCall>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_history: Option<crate::LogicalProviderHistory>,
    },
    ToolResult {
        source_event_id: EventId,
        tool_call_id: ToolCallId,
        tool_name: String,
        content: String,
        content_hash: ContentHash,
        is_error: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderToolNameBinding {
    pub tool_definition: ToolDefinitionSnapshotRef,
    pub canonical_name: String,
    pub provider_wire_name: String,
    pub package_schema_hash: CanonicalHashV1,
    pub provider_wrapped_schema_hash: CanonicalHashV1,
    pub provider_arguments_envelope_version: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttachmentSnapshot {
    pub source_event_id: EventId,
    pub artifact_id: ArtifactId,
    pub file_reference_id: Option<FileReferenceId>,
    pub display_name: String,
    pub media_type: String,
    pub byte_length: u64,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedModelAttachment {
    pub snapshot: ModelAttachmentSnapshot,
    pub bytes: Arc<[u8]>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelOutputFormat {
    Text,
    JsonObject,
    JsonSchema { name: String, schema: Value, schema_hash: ContentHash, strict: bool },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOutputRequest {
    pub format: ModelOutputFormat,
    pub effective_max_output_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalModelRequest {
    pub model_step_id: ModelStepId,
    pub execution_snapshot_id: ModelExecutionSnapshotId,
    pub context_snapshot_id: ContextProjectionSnapshotId,
    pub instruction_envelope: CompiledInstructionEnvelope,
    pub conversation_items: Vec<LogicalConversationItem>,
    pub tool_schema_set: ToolSchemaSetSnapshotV1,
    /// Wire order only. The immutable schema set retains canonical ToolId order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_tool_ids: Vec<ToolId>,
    pub attachments: Vec<ModelAttachmentSnapshot>,
    pub provider_continuation: Option<ProviderContinuationState>,
    pub output_request: ModelOutputRequest,
    /// A question asked after the whole request the step would have sent - a compaction asked on
    /// that request, so the provider reads it from its cache. It is a user message of its own
    /// after the conversation and the task state, which stay where the step put them. The tools
    /// the request carries for the cache stay declared; the question itself says none may be
    /// called.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closing_question: Option<String>,
    /// What followed the closing question when the model called tools instead of answering it:
    /// that answer, then a result for each call saying no tool runs. Sent after the question in
    /// this order, so all before them is the request already sent and the model answers again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closing_exchange: Vec<LogicalConversationItem>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum LogicalModelRequestError {
    #[error("tool snapshots must be strictly ordered by ToolId and contain each ToolId once")]
    InvalidToolOrder,
    #[error("tool canonical names and provider wire names must be non-blank")]
    InvalidToolName,
    #[error("tool version must contain a non-zero definition revision")]
    InvalidToolVersion,
    #[error("tool input_schema and json_schema output formats must contain JSON objects")]
    InvalidSchema,
    #[error("assistant content and content_hash must be simultaneously present or absent")]
    InvalidAssistantContent,
    #[error("assistant tool_call_id values must be unique within one assistant item")]
    DuplicateToolCallId,
    #[error("historical tool arguments must contain a complete JSON value")]
    InvalidToolArguments,
    #[error("attachment display_name and media_type must be non-blank")]
    InvalidAttachmentMetadata,
    #[error("compiled instruction envelope is not canonical or its hash does not match")]
    InvalidInstructionEnvelope,
    #[error("a closing question must be canonical, non-empty instruction text")]
    InvalidClosingQuestion,
    #[error("model output request requires a non-zero resolved output token limit")]
    InvalidOutputLimit,
    #[error("model request attachments have not been resolved through the authorized pipeline")]
    AttachmentsNotResolved,
    #[error("resolved attachment bytes do not match the frozen attachment snapshot")]
    ResolvedAttachmentMismatch,
    #[error("provider arguments envelope could not be compiled canonically")]
    ProviderArgumentsEnvelopeInvalid,
}

impl LogicalModelRequest {
    pub fn validate_contract_shape(&self) -> Result<(), LogicalModelRequestError> {
        self.instruction_envelope.validate_contract_shape()?;
        self.tool_schema_set.validate().map_err(|_| LogicalModelRequestError::InvalidSchema)?;
        if !self.fixed_tool_ids.windows(2).all(|p| p[0] < p[1])
            || self
                .fixed_tool_ids
                .iter()
                .any(|id| !self.tool_schema_set.tools.iter().any(|t| t.tool_id == *id))
        {
            return Err(LogicalModelRequestError::InvalidToolOrder);
        }

        for item in self.sent_items() {
            if let LogicalConversationItem::Assistant {
                content, content_hash, tool_calls, ..
            } = item
            {
                if content.is_some() != content_hash.is_some() {
                    return Err(LogicalModelRequestError::InvalidAssistantContent);
                }
                let mut tool_call_ids = BTreeSet::new();
                for tool_call in tool_calls {
                    if tool_call.canonical_name.trim().is_empty() {
                        return Err(LogicalModelRequestError::InvalidToolName);
                    }
                    if serde_json::from_str::<Value>(&tool_call.arguments_json).is_err() {
                        return Err(LogicalModelRequestError::InvalidToolArguments);
                    }
                    if !tool_call_ids.insert(tool_call.tool_call_id) {
                        return Err(LogicalModelRequestError::DuplicateToolCallId);
                    }
                }
            }
        }
        if self.attachments.iter().any(|attachment| {
            attachment.display_name.trim().is_empty() || attachment.media_type.trim().is_empty()
        }) {
            return Err(LogicalModelRequestError::InvalidAttachmentMetadata);
        }
        if let ModelOutputFormat::JsonSchema { name, schema, .. } = &self.output_request.format
            && (name.trim().is_empty() || !schema.is_object())
        {
            return Err(LogicalModelRequestError::InvalidSchema);
        }
        if self.output_request.effective_max_output_tokens == 0 {
            return Err(LogicalModelRequestError::InvalidOutputLimit);
        }
        if self.closing_question.as_deref().is_some_and(|q| !is_canonical_instruction_text(q))
            || (self.closing_question.is_none() && !self.closing_exchange.is_empty())
        {
            return Err(LogicalModelRequestError::InvalidClosingQuestion);
        }
        Ok(())
    }

    /// The conversation as the request sends it: the history, then what followed the closing
    /// question.
    pub fn sent_items(&self) -> impl Iterator<Item = &LogicalConversationItem> {
        self.conversation_items.iter().chain(&self.closing_exchange)
    }
}

impl CompiledInstructionEnvelope {
    pub fn new(
        serializer_version: u32,
        ordered_blocks: Vec<LogicalInstructionBlock>,
        canonical_text: String,
    ) -> Result<Self, LogicalModelRequestError> {
        let instruction_hash = ContentHash::sha256(canonical_text.as_bytes());
        let envelope =
            Self { serializer_version, ordered_blocks, canonical_text, instruction_hash };
        envelope.validate_contract_shape()?;
        Ok(envelope)
    }

    pub fn validate_contract_shape(&self) -> Result<(), LogicalModelRequestError> {
        const ORDER: [LogicalInstructionKind; 8] = [
            LogicalInstructionKind::Core,
            LogicalInstructionKind::Permission,
            LogicalInstructionKind::ToolSummary,
            LogicalInstructionKind::WorkspaceAgents,
            LogicalInstructionKind::HistoricalSnapshot,
            LogicalInstructionKind::AgentMode,
            LogicalInstructionKind::Hook,
            LogicalInstructionKind::TaskState,
        ];
        let blocks_are_valid = self.ordered_blocks.len() == ORDER.len()
            && self.ordered_blocks.iter().zip(ORDER).all(|(block, expected_kind)| {
                block.kind == expected_kind
                    && is_canonical_instruction_text(&block.content)
                    && block.content_hash == ContentHash::sha256(block.content.as_bytes())
                    && block.source_event_ids.windows(2).all(|pair| pair[0] < pair[1])
            });
        let compiled_text = self
            .ordered_blocks
            .iter()
            .map(|block| block.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if self.serializer_version == 0
            || !blocks_are_valid
            || !is_canonical_instruction_text(&self.canonical_text)
            || self.canonical_text != compiled_text
            || self.instruction_hash != ContentHash::sha256(self.canonical_text.as_bytes())
        {
            return Err(LogicalModelRequestError::InvalidInstructionEnvelope);
        }
        Ok(())
    }
}

fn is_canonical_instruction_text(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('\u{feff}')
        && !value.contains('\r')
        && !value.starts_with('\n')
        && !value.ends_with('\n')
}

impl ResolvedModelAttachment {
    pub fn validate_contract_shape(&self) -> Result<(), LogicalModelRequestError> {
        let byte_length = u64::try_from(self.bytes.len())
            .map_err(|_| LogicalModelRequestError::ResolvedAttachmentMismatch)?;
        if byte_length != self.snapshot.byte_length
            || ContentHash::sha256(&self.bytes) != self.snapshot.content_hash
        {
            return Err(LogicalModelRequestError::ResolvedAttachmentMismatch);
        }
        Ok(())
    }
}

pub fn validate_resolved_attachments(
    request: &LogicalModelRequest,
    resolved: &[ResolvedModelAttachment],
) -> Result<(), LogicalModelRequestError> {
    if request.attachments.len() != resolved.len() {
        return Err(LogicalModelRequestError::AttachmentsNotResolved);
    }
    for (snapshot, attachment) in request.attachments.iter().zip(resolved) {
        if snapshot != &attachment.snapshot {
            return Err(LogicalModelRequestError::ResolvedAttachmentMismatch);
        }
        attachment.validate_contract_shape()?;
    }
    Ok(())
}

impl ProviderToolNameBinding {
    pub fn from_snapshot(
        registry_snapshot_ref: &aura_tool_contract::ToolRegistrySnapshotRef,
        snapshot: &aura_tool_contract::ToolSchemaSnapshotV1,
        provider_wrapped_schema_hash: CanonicalHashV1,
    ) -> Self {
        let canonical_name = snapshot.canonical_name.to_string();
        Self {
            tool_definition: ToolDefinitionSnapshotRef {
                tool_id: snapshot.tool_id,
                tool_version: snapshot.tool_version.clone(),
                registry_snapshot_ref: registry_snapshot_ref.clone(),
            },
            provider_wire_name: provider_wire_tool_name(&canonical_name, snapshot.tool_id),
            canonical_name,
            package_schema_hash: snapshot.schema_hash.clone(),
            provider_wrapped_schema_hash,
            provider_arguments_envelope_version: PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V3,
        }
    }

    pub fn validate_contract_shape(&self) -> Result<(), LogicalModelRequestError> {
        if self.canonical_name.trim().is_empty() || self.provider_wire_name.trim().is_empty() {
            return Err(LogicalModelRequestError::InvalidToolName);
        }
        self.tool_definition
            .tool_version
            .validate()
            .map_err(|_| LogicalModelRequestError::InvalidToolVersion)?;
        if !matches!(
            self.provider_arguments_envelope_version,
            PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V1
                | PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V2
                | PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V3
        ) || self.provider_wire_name
            != provider_wire_tool_name(&self.canonical_name, self.tool_definition.tool_id)
        {
            return Err(LogicalModelRequestError::InvalidToolName);
        }
        Ok(())
    }
}

pub fn provider_arguments_envelope_schema_v1(
    package_arguments_schema: &Value,
) -> Result<(Value, CanonicalHashV1), LogicalModelRequestError> {
    if !package_arguments_schema.is_object() {
        return Err(LogicalModelRequestError::ProviderArgumentsEnvelopeInvalid);
    }
    let wrapped = serde_json::json!({
        "type": "object",
        "properties": {
            "_aura": {
                "type": "object",
                "properties": {
                    "purpose": {"type": "string", "minLength": 1, "maxLength": 4096}
                },
                "required": ["purpose"],
                "additionalProperties": false
            },
            "arguments": package_arguments_schema
        },
        "required": ["_aura", "arguments"],
        "additionalProperties": false
    });
    let hash = aura_domain_common::jcs::canonical_sha256(&wrapped)
        .map_err(|_| LogicalModelRequestError::ProviderArgumentsEnvelopeInvalid)?;
    Ok((wrapped, hash))
}

/// Version 2 binds local references to the package root before wrapping.
pub fn provider_arguments_envelope_schema_v2(
    package_arguments_schema: &Value,
) -> Result<(Value, CanonicalHashV1), LogicalModelRequestError> {
    Ok(crate::provider_schema::compiled_envelope(package_arguments_schema)?.as_ref().clone())
}

/// Version 3 adds the required `aura_purpose` to the package's own root object.
pub fn provider_arguments_envelope_schema_v3(
    package_arguments_schema: &Value,
) -> Result<(Value, CanonicalHashV1), LogicalModelRequestError> {
    Ok(crate::provider_schema::compiled_flat_envelope(package_arguments_schema)?.as_ref().clone())
}

#[must_use]
pub fn provider_wire_tool_name(canonical_name: &str, tool_id: ToolId) -> String {
    let mut stem = String::with_capacity(36);
    let mut previous_replaced = false;
    for character in canonical_name.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
            if stem.len() == 36 {
                break;
            }
            stem.push(character);
            previous_replaced = false;
        } else if !previous_replaced && stem.len() < 36 {
            stem.push('_');
            previous_replaced = true;
        }
    }
    let stem = stem.trim_matches(['_', '-']);
    let stem = if stem.is_empty() { "tool" } else { stem };
    format!("{stem}__{}", tool_id.ulid().to_string().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{
        ContentHash, ContentHashAlgorithm, ContextProjectionSnapshotId, EventId,
        ModelExecutionSnapshotId, ModelStepId, Revision,
    };
    use aura_domain_common::{ToolCallId, ToolId, ToolRegistrySnapshotId};
    use aura_tool_contract::{
        ToolInputSchemaV1, ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1,
        ToolVersion,
    };
    use serde_json::json;

    use super::{
        CompiledInstructionEnvelope, LogicalConversationItem, LogicalInstructionBlock,
        LogicalInstructionKind, LogicalModelRequest, ModelOutputFormat, ModelOutputRequest,
        provider_wire_tool_name,
    };

    fn hash() -> ContentHash {
        ContentHash::new(ContentHashAlgorithm::Sha256, "11".repeat(32)).expect("hash")
    }

    fn instruction_envelope() -> CompiledInstructionEnvelope {
        let kinds = [
            LogicalInstructionKind::Core,
            LogicalInstructionKind::Permission,
            LogicalInstructionKind::ToolSummary,
            LogicalInstructionKind::WorkspaceAgents,
            LogicalInstructionKind::HistoricalSnapshot,
            LogicalInstructionKind::AgentMode,
            LogicalInstructionKind::Hook,
            LogicalInstructionKind::TaskState,
        ];
        let ordered_blocks: Vec<LogicalInstructionBlock> = kinds
            .into_iter()
            .map(|kind| {
                let content = format!("{kind:?}");
                LogicalInstructionBlock {
                    kind,
                    content_hash: ContentHash::sha256(content.as_bytes()),
                    content,
                    source_event_ids: Vec::new(),
                }
            })
            .collect();
        let canonical_text = ordered_blocks
            .iter()
            .map(|block: &LogicalInstructionBlock| block.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        CompiledInstructionEnvelope::new(1, ordered_blocks, canonical_text)
            .expect("canonical instruction envelope")
    }

    fn registry_ref() -> ToolRegistrySnapshotRef {
        ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"registry"),
        }
    }

    fn empty_schema_set() -> ToolSchemaSetSnapshotV1 {
        ToolSchemaSetSnapshotV1::new(registry_ref(), Vec::new()).expect("empty schema set")
    }

    #[test]
    fn tool_snapshots_use_frozen_version_wire_and_require_tool_id_order() {
        let make = |tool_id, name: &str| {
            ToolSchemaSnapshotV1::new(
                tool_id,
                ToolVersion::new(None, Revision::INITIAL).expect("version"),
                name.parse().expect("name"),
                "Read workspace metadata".to_owned(),
                ToolInputSchemaV1::new(json!({"type": "object"})).expect("schema"),
                true,
            )
            .expect("snapshot")
        };
        let first = make(ToolId::from_bytes([1; 16]), "fs.read_text");
        let second = make(ToolId::from_bytes([2; 16]), "fs.stat");
        let schema_set = ToolSchemaSetSnapshotV1::new(registry_ref(), vec![first.clone(), second])
            .expect("schema set");
        let request = LogicalModelRequest {
            fixed_tool_ids: vec![],
            model_step_id: ModelStepId::new(),
            execution_snapshot_id: ModelExecutionSnapshotId::new(),
            context_snapshot_id: ContextProjectionSnapshotId::new(),
            instruction_envelope: instruction_envelope(),
            conversation_items: vec![LogicalConversationItem::Assistant {
                provider_history: None,
                source_event_id: EventId::new(),
                content: None,
                content_hash: None,
                tool_calls: Vec::new(),
            }],
            tool_schema_set: schema_set,
            attachments: Vec::new(),
            provider_continuation: None,
            output_request: ModelOutputRequest {
                format: ModelOutputFormat::Text,
                effective_max_output_tokens: 128,
            },
            closing_question: None,
            closing_exchange: Vec::new(),
        };
        assert!(request.validate_contract_shape().is_ok());
        assert_eq!(
            serde_json::to_value(&request).expect("request")["tool_schema_set"]["tools"][0]["tool_version"],
            json!({"semantic_version": null, "definition_revision": "1"})
        );

        let mut duplicate = request;
        duplicate.tool_schema_set.tools.push(first);
        assert!(duplicate.validate_contract_shape().is_err());
    }

    #[test]
    fn provider_tool_wire_name_uses_the_frozen_stem_and_full_tool_id_payload() {
        let tool_id = ToolId::from_bytes([1; 16]);
        let expected_payload = tool_id.ulid().to_string().to_ascii_lowercase();
        assert_eq!(
            provider_wire_tool_name("--图..fs/read???", tool_id),
            format!("fs_read__{expected_payload}")
        );

        let long_name = "abcdefghijklmnopqrstuvwxyz0123456789-extra";
        let wire_name = provider_wire_tool_name(long_name, tool_id);
        assert_eq!(wire_name, format!("abcdefghijklmnopqrstuvwxyz0123456789__{expected_payload}"));
        assert_eq!(wire_name.len(), 64);
    }

    #[test]
    fn assistant_content_pair_and_tool_call_ids_are_validated() {
        let call_id = ToolCallId::new();
        let mut request = LogicalModelRequest {
            fixed_tool_ids: vec![],
            model_step_id: ModelStepId::new(),
            execution_snapshot_id: ModelExecutionSnapshotId::new(),
            context_snapshot_id: ContextProjectionSnapshotId::new(),
            instruction_envelope: instruction_envelope(),
            conversation_items: vec![LogicalConversationItem::Assistant {
                provider_history: None,
                source_event_id: EventId::new(),
                content: Some("text".to_owned()),
                content_hash: None,
                tool_calls: Vec::new(),
            }],
            tool_schema_set: empty_schema_set(),
            attachments: Vec::new(),
            provider_continuation: None,
            output_request: ModelOutputRequest {
                format: ModelOutputFormat::Text,
                effective_max_output_tokens: 128,
            },
            closing_question: None,
            closing_exchange: Vec::new(),
        };
        assert!(request.validate_contract_shape().is_err());

        request.conversation_items = vec![LogicalConversationItem::Assistant {
            provider_history: None,
            source_event_id: EventId::new(),
            content: None,
            content_hash: None,
            tool_calls: vec![
                super::LogicalHistoricalToolCall {
                    tool_call_id: call_id,
                    tool_id: Some(ToolId::new()),
                    canonical_name: "fs.stat".to_owned(),
                    arguments_json: "{}".to_owned(),
                    arguments_hash: hash(),
                },
                super::LogicalHistoricalToolCall {
                    tool_call_id: call_id,
                    tool_id: Some(ToolId::new()),
                    canonical_name: "fs.read_text".to_owned(),
                    arguments_json: "{}".to_owned(),
                    arguments_hash: hash(),
                },
            ],
        }];
        assert!(request.validate_contract_shape().is_err());
    }

    #[test]
    fn a_closing_question_is_canonical_text_and_left_out_when_absent() {
        let mut request = LogicalModelRequest {
            fixed_tool_ids: vec![],
            model_step_id: ModelStepId::new(),
            execution_snapshot_id: ModelExecutionSnapshotId::new(),
            context_snapshot_id: ContextProjectionSnapshotId::new(),
            instruction_envelope: instruction_envelope(),
            conversation_items: Vec::new(),
            tool_schema_set: empty_schema_set(),
            attachments: Vec::new(),
            provider_continuation: None,
            output_request: ModelOutputRequest {
                format: ModelOutputFormat::Text,
                effective_max_output_tokens: 128,
            },
            closing_question: None,
            closing_exchange: Vec::new(),
        };
        assert!(request.validate_contract_shape().is_ok());
        let wire = serde_json::to_value(&request).expect("request");
        assert!(wire.get("closing_question").is_none(), "{wire}");
        for invalid in ["", "\nask", "ask\r\nmore", "ask\n"] {
            request.closing_question = Some(invalid.to_owned());
            assert!(request.validate_contract_shape().is_err(), "{invalid:?}");
        }
        request.closing_question = Some("Summarize the conversation.".to_owned());
        assert!(request.validate_contract_shape().is_ok());
        let wire = serde_json::to_value(&request).expect("request");
        assert_eq!(wire["closing_question"], "Summarize the conversation.");
        assert!(wire.get("closing_exchange").is_none(), "{wire}");
        let read: LogicalModelRequest = serde_json::from_value(wire).expect("round trip");
        assert_eq!(read, request);

        // What followed the question is checked as the history is, and only follows a question.
        let answered = |content: Option<&str>| LogicalConversationItem::Assistant {
            provider_history: None,
            source_event_id: EventId::new(),
            content: content.map(ToOwned::to_owned),
            content_hash: None,
            tool_calls: Vec::new(),
        };
        request.closing_exchange = vec![answered(Some("text"))];
        assert!(request.validate_contract_shape().is_err(), "content without its hash");
        request.closing_exchange = vec![answered(None)];
        assert!(request.validate_contract_shape().is_ok());
        assert_eq!(request.sent_items().count(), 1);
        let wire = serde_json::to_value(&request).expect("request");
        assert_eq!(wire["closing_exchange"].as_array().map(Vec::len), Some(1));
        let read: LogicalModelRequest = serde_json::from_value(wire).expect("round trip");
        assert_eq!(read, request);
        request.closing_question = None;
        assert!(request.validate_contract_shape().is_err(), "no question to follow");
    }
}
