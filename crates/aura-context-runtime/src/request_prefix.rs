//! What the request a step would have sent is made of, kept so a compaction can be
//! asked on that same prefix and the provider's cache of it is used again.
use aura_context_domain::{ModelEgressManifestV1, PreparedLogicalContext};
use aura_model_domain::{
    CompiledInstructionEnvelope, LogicalConversationItem, ModelAttachmentSnapshot,
};
use aura_tool_contract::ToolSchemaSetSnapshotV1;

use crate::{ContextPreparationInput, ModelEgressSourceDraft};

#[derive(Clone, Debug)]
pub struct RequestPrefix {
    pub instruction_envelope: CompiledInstructionEnvelope,
    pub conversation_items: Vec<LogicalConversationItem>,
    pub tool_schema_set: ToolSchemaSetSnapshotV1,
    pub attachments: Vec<ModelAttachmentSnapshot>,
    /// A request continued from state the provider keeps has no prefix of its own to send again.
    pub provider_continuation: bool,
    /// The step's egress sources, in the order its manifest lists them.
    pub egress_sources: Vec<ModelEgressSourceDraft>,
    pub estimated_input_tokens: u64,
}

impl RequestPrefix {
    pub(crate) fn of_compilation(
        instruction_envelope: &CompiledInstructionEnvelope,
        conversation_items: Vec<LogicalConversationItem>,
        input: &ContextPreparationInput,
        provider_continuation: bool,
        manifest: &ModelEgressManifestV1,
        estimated_input_tokens: u64,
    ) -> Self {
        Self {
            instruction_envelope: instruction_envelope.clone(),
            conversation_items,
            tool_schema_set: input.tool_schema_set.clone(),
            attachments: input.attachments.clone(),
            provider_continuation,
            egress_sources: drafts(manifest),
            estimated_input_tokens,
        }
    }

    /// The prefix of a request that fits, for a compaction the user asked for.
    #[must_use]
    pub fn of_prepared(context: &PreparedLogicalContext) -> Self {
        Self {
            instruction_envelope: context.instruction_envelope.clone(),
            conversation_items: context.conversation_items.clone(),
            tool_schema_set: context.tool_schema_set.clone(),
            attachments: context.attachments.clone(),
            provider_continuation: context.provider_continuation.is_some(),
            egress_sources: drafts(&context.model_egress_manifest),
            estimated_input_tokens: context.snapshot.budget.estimated_input_tokens,
        }
    }
}

fn drafts(manifest: &ModelEgressManifestV1) -> Vec<ModelEgressSourceDraft> {
    manifest
        .items
        .iter()
        .map(|item| ModelEgressSourceDraft {
            source_ref: item.source_ref.clone(),
            source_kind: item.source_kind,
            data_class: item.data_class,
            content_hash: item.content_hash.clone(),
            byte_length: item.byte_length,
            authorization_basis: item.authorization_basis.clone(),
        })
        .collect()
}
