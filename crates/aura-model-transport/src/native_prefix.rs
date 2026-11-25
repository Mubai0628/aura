//! Commitment to the model-visible prefix that produced a native reasoning item.
use crate::{
    ModelAdapterError, ProviderTools, historical_tool_call_wire_bindings, provider_history_replay,
};
use aura_domain_common::{ContentHash, EventId, ToolCallId, ToolId};
use aura_model_domain::{
    CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION, CompiledInstructionEnvelope,
    LogicalConversationItem, ModelAttachmentSnapshot, ModelExecutionSnapshot, ModelOutputFormat,
    ProviderToolCallWireBinding, TaskStateUpdate,
};
use aura_tool_contract::ToolSchemaSetSnapshotV1;
use std::collections::BTreeMap;

pub struct NativePrefix<'a> {
    target: &'a ModelExecutionSnapshot,
    hash: ContentHash,
    attachments: BTreeMap<EventId, Vec<&'a ModelAttachmentSnapshot>>,
    bindings: BTreeMap<ToolCallId, &'a ProviderToolCallWireBinding>,
    state_updates: std::iter::Peekable<std::slice::Iter<'a, TaskStateUpdate>>,
}

#[derive(Clone, Debug)]
pub struct NativeHistoryProjection {
    pub prefix: ContentHash,
    pub missing_evidence: usize,
    pub mismatched: usize,
}

impl<'a> NativePrefix<'a> {
    pub fn new(
        target: &'a ModelExecutionSnapshot,
        envelope: &'a CompiledInstructionEnvelope,
        schema: &ToolSchemaSetSnapshotV1,
        fixed_tools: &[ToolId],
        attachments: &'a [ModelAttachmentSnapshot],
        output_format: &ModelOutputFormat,
    ) -> Result<Self, ModelAdapterError> {
        let tools = ProviderTools::compile(schema, target.api_format)?.into_wire(fixed_tools).0;
        let settings = &target.request_options;
        let domain = (
            target.endpoint_id,
            &target.endpoint_fingerprint,
            &target.resolved_request_url,
            target.api_format,
            &target.model_name,
            &target.credential_binding_fingerprint,
            &target.openai_organization_id,
            &target.openai_project_id,
            &target.anthropic_workspace_id,
            target.chat_instruction_role,
            target.chat_output_token_field,
            settings.reasoning_effort,
            settings.reasoning_by_effort,
            settings.image_input,
            &settings.speed_mode,
        );
        let mut by_event: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for attachment in attachments {
            by_event.entry(attachment.source_event_id).or_default().push(attachment);
        }
        let hash = if let Some(replay) = &envelope.task_state_replay {
            let layout = crate::instruction_envelope_layout(envelope);
            digest(&(
                "aura.native_prefix.v3.responses_state",
                envelope.serializer_version,
                domain,
                &replay.authority,
                layout.stable,
                &replay.leading,
                layout.background,
                tools,
                output_format,
            ))?
        } else {
            digest(&(
                "aura.native_prefix.v2",
                CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION,
                domain,
                &envelope.instruction_hash,
                tools,
                output_format,
            ))?
        };
        Ok(Self {
            target,
            hash,
            attachments: by_event,
            bindings: historical_tool_call_wire_bindings(target),
            state_updates: envelope
                .task_state_replay
                .as_ref()
                .map_or(&[][..], |replay| replay.updates.as_slice())
                .iter()
                .peekable(),
        })
    }

    #[must_use]
    pub fn hash(&self) -> &ContentHash {
        &self.hash
    }

    pub fn append(&mut self, item: &LogicalConversationItem) -> Result<(), ModelAdapterError> {
        let (event, calls) = match item {
            LogicalConversationItem::Assistant { source_event_id, tool_calls, .. } => {
                (*source_event_id, tool_calls.as_slice())
            }
            LogicalConversationItem::User { source_event_id, .. }
            | LogicalConversationItem::ToolResult { source_event_id, .. } => {
                (*source_event_id, &[][..])
            }
        };
        let bindings: Vec<_> =
            calls.iter().map(|call| self.bindings.get(&call.tool_call_id)).collect();
        self.hash = digest(&(&self.hash, item, self.attachments.get(&event), bindings))?;
        if self.state_updates.peek().is_some_and(|update| update.after == event)
            && let Some(update) = self.state_updates.next()
        {
            self.hash = digest(&(&self.hash, "developer", &update.content))?;
        }
        Ok(())
    }

    /// Keep a native block only if its origin proves the prefix just before that assistant
    /// response. Raw provider outputs and the journal remain untouched.
    pub fn project(
        mut self,
        items: &mut [LogicalConversationItem],
        origins: &BTreeMap<EventId, ContentHash>,
    ) -> Result<NativeHistoryProjection, ModelAdapterError> {
        let mut missing_evidence = 0;
        let mut mismatched = 0;
        for item in items {
            if let LogicalConversationItem::Assistant { source_event_id, provider_history, .. } =
                item
                && let Some(history) = provider_history
            {
                let origin = origins.get(source_event_id);
                if provider_history_replay(history, self.target).is_none()
                    || origin.is_some_and(|value| value != self.hash())
                {
                    mismatched += 1;
                    *provider_history = None;
                } else if origin.is_none() {
                    missing_evidence += 1;
                    *provider_history = None;
                }
            }
            self.append(item)?;
        }
        Ok(NativeHistoryProjection { prefix: self.hash, missing_evidence, mismatched })
    }
}

fn digest(value: &impl serde::Serialize) -> Result<ContentHash, ModelAdapterError> {
    let bytes = serde_json::to_vec(value).map_err(|_| {
        ModelAdapterError::invalid_request(
            "model_request.native_prefix_invalid",
            "Native history prefix could not be serialized",
        )
    })?;
    Ok(ContentHash::sha256(&bytes))
}
