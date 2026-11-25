use crate::{ContextPreparationError, ContextPreparationInput};
use aura_domain_common::{ContentHash, EventId, ToolId};
use aura_model_domain::{
    CompiledInstructionEnvelope, LogicalConversationItem, ModelExecutionSnapshot,
};
pub use aura_model_transport::NativeHistoryProjection;
use aura_model_transport::NativePrefix;
use std::collections::BTreeMap;

pub(crate) struct NativeHistory<'a> {
    pub(crate) target: &'a ModelExecutionSnapshot,
    pub(crate) fixed_tools: &'a [ToolId],
    pub(crate) origins: &'a BTreeMap<EventId, ContentHash>,
}

impl NativeHistory<'_> {
    /// Check the prefix immediately before the assistant group. Appending to the request does
    /// not invalidate an earlier commitment. Original provider records remain in the journal.
    pub(crate) fn project(
        &self,
        input: &ContextPreparationInput,
        instructions: &CompiledInstructionEnvelope,
        items: &mut [LogicalConversationItem],
    ) -> Result<NativeHistoryProjection, ContextPreparationError> {
        Ok(NativePrefix::new(
            self.target,
            instructions,
            &input.tool_schema_set,
            self.fixed_tools,
            &input.attachments,
            &input.request.output_request.format,
        )?
        .project(items, self.origins)?)
    }
}
