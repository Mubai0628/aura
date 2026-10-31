//! A compaction asked on the request the step would have sent. Its instructions, task state, tools,
//! images and conversation are that request's, byte for byte, so a provider that cached them for
//! the previous step reads them from its cache, and the compaction's ask follows them as a user
//! message of its own that says no tool is to be called. When that request leaves no room for the
//! summary, or continues from state the provider keeps, the compaction is asked the isolated way
//! instead, which is also where a failed attempt goes once the step's request can give no summary.
use super::*;

/// The room a summary and the thinking before it need before the prefix is tried.
const MIN_OUTPUT_TOKENS: u64 = 16_000;
/// How much of each source the ask quotes to name it.
const DESCRIPTION_CHARS: usize = 100;

pub(super) struct PrefixAttempt {
    pub envelope: CompiledInstructionEnvelope,
    /// The compaction's ask, the request's closing question.
    pub ask: String,
    pub conversation_items: Vec<LogicalConversationItem>,
    pub tool_schema_set: aura_tool_contract::ToolSchemaSetSnapshotV1,
    pub fixed_tool_ids: Vec<ToolId>,
    pub attachments: Vec<aura_model_domain::ModelAttachmentSnapshot>,
    /// The bytes of `attachments`, in their order.
    pub resolved_attachments: Arc<[aura_model_domain::ResolvedModelAttachment]>,
    pub egress_sources: Vec<ModelEgressSourceDraft>,
    pub max_output: u64,
    /// The room the window leaves the request for its output.
    pub room: u64,
}

/// The compaction's ask: where the part it replaces ends, the focus it was given and how the
/// summary is written.
pub(super) fn ask(
    core: &str,
    anchors: &CompactionAnchorManifest,
    input: &ContextPreparationInput,
    focus: serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<String> {
    let through = anchors
        .anchors
        .iter()
        .rev()
        .find(|anchor| matches!(anchor.source_ref, ContextSourceRef::Event { .. }))
        .map(|anchor| describe(anchor, input))
        .unwrap_or_default();
    let earlier = anchors
        .anchors
        .iter()
        .any(|anchor| matches!(anchor.source_ref, ContextSourceRef::Artifact { .. }));
    let focus = if focus.is_empty() {
        String::new()
    } else {
        format!("\n{}", serde_json::to_string(&focus)?)
    };
    Ok(format!(
        "Compaction request: write the summary described below in place of the older part of this \
         conversation: from its first message through the one that begins \u{ab}{through}\u{bb}{}. \
         The messages after it stay in the conversation as they are. Leave out the activities the \
         task state lists: they are shown afresh with every request. Do not call any tool and do \
         not continue the task; reply with the summary only.{focus}\n{core}",
        if earlier {
            ", and of the earlier summary in your instructions, which it takes in"
        } else {
            ""
        }
    ))
}

/// A source as the model saw it in the conversation: its role and how it begins.
fn describe(anchor: &CompactionAnchor, input: &ContextPreparationInput) -> String {
    let seen = match &anchor.source_ref {
        ContextSourceRef::Event { event_id } => input
            .recent_history_items
            .iter()
            .find(|item| selection::event_id(item) == *event_id)
            .map(|item| match item {
                LogicalConversationItem::User { content, .. } => format!("user: {content}"),
                LogicalConversationItem::Assistant { content: Some(content), .. } => {
                    format!("assistant: {content}")
                }
                LogicalConversationItem::Assistant { tool_calls, .. } => format!(
                    "assistant calls: {}",
                    tool_calls
                        .iter()
                        .map(|c| c.canonical_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                LogicalConversationItem::ToolResult { tool_name, content, .. } => {
                    format!("{tool_name} result: {content}")
                }
            }),
        ContextSourceRef::Artifact { .. } => {
            Some("the earlier summary in Historical-Snapshot-Summary".to_owned())
        }
        _ => None,
    };
    let text = seen.unwrap_or_else(|| anchor.preview.clone());
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(DESCRIPTION_CHARS).collect()
}

impl Execution {
    /// The first attempt, on the step's own prefix.
    pub(super) fn prefix_attempt(
        &self,
        prefix: &PrefixAttempt,
        mut snapshot: ModelExecutionSnapshot,
        ordinal: u8,
    ) -> anyhow::Result<PreparedCompactionModelAttempt> {
        // The calls the conversation carries keep their wire bindings, as in the step's request.
        let carried = prefix
            .conversation_items
            .iter()
            .filter_map(|item| match item {
                LogicalConversationItem::Assistant { tool_calls, .. } => Some(tool_calls),
                _ => None,
            })
            .flatten()
            .map(|call| call.tool_call_id)
            .collect::<std::collections::BTreeSet<_>>();
        snapshot
            .historical_tool_call_wire_bindings
            .retain(|binding| carried.contains(&binding.tool_call_id));
        snapshot.request_options.effective_max_output_tokens = prefix.max_output;
        let logical = LogicalModelRequest {
            fixed_tool_ids: prefix.fixed_tool_ids.clone(),
            model_step_id: snapshot.model_step_id,
            execution_snapshot_id: snapshot.id,
            context_snapshot_id: snapshot.context_snapshot_id,
            instruction_envelope: prefix.envelope.clone(),
            conversation_items: prefix.conversation_items.clone(),
            tool_schema_set: prefix.tool_schema_set.clone(),
            attachments: prefix.attachments.clone(),
            provider_continuation: None,
            output_request: ModelOutputRequest {
                format: ModelOutputFormat::Text,
                effective_max_output_tokens: prefix.max_output,
            },
            closing_question: Some(prefix.ask.clone()),
            closing_exchange: Vec::new(),
        };
        let egress_manifest = build_model_egress_manifest(
            snapshot.context_snapshot_id,
            prefix.egress_sources.clone(),
        )?;
        Ok(PreparedCompactionModelAttempt {
            attempt_ordinal: ordinal,
            logical_request: logical,
            execution_snapshot: snapshot,
            egress_manifest,
            shape: CompactionRequestShape::SessionPrefix,
        })
    }

    /// `failed`'s request asked again as an attempt of its own: with `exchange` after what followed
    /// its question, and with `max_output` as its output limit, when given.
    pub(super) fn ask_again(
        &self,
        failed: &PreparedCompactionModelAttempt,
        ordinal: u8,
        max_output: Option<u64>,
        exchange: Option<super::exchange::Exchange>,
    ) -> anyhow::Result<PreparedCompactionModelAttempt> {
        let mut snapshot = failed.execution_snapshot.clone();
        snapshot.id = ModelExecutionSnapshotId::new();
        snapshot.model_step_id = ModelStepId::new();
        snapshot.context_snapshot_id = ContextProjectionSnapshotId::new();
        snapshot.created_at = UtcTimestamp::now();
        let mut logical = failed.logical_request.clone();
        logical.model_step_id = snapshot.model_step_id;
        logical.execution_snapshot_id = snapshot.id;
        logical.context_snapshot_id = snapshot.context_snapshot_id;
        let mut sources = failed
            .egress_manifest
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
            .collect::<Vec<_>>();
        if let Some(exchange) = exchange {
            logical.closing_exchange.extend(exchange.items);
            snapshot.historical_tool_call_wire_bindings.extend(exchange.bindings);
            sources.push(exchange.source);
        }
        if let Some(limit) = max_output {
            snapshot.request_options.effective_max_output_tokens = limit;
            logical.output_request.effective_max_output_tokens = limit;
        }
        let egress_manifest = build_model_egress_manifest(snapshot.context_snapshot_id, sources)?;
        Ok(PreparedCompactionModelAttempt {
            attempt_ordinal: ordinal,
            logical_request: logical,
            execution_snapshot: snapshot,
            egress_manifest,
            shape: failed.shape,
        })
    }

    /// The room the window leaves for the output of the step's request with `exchange` after its
    /// question; each byte of the exchange is counted a token.
    pub(super) fn room_for(&self, exchange: &[LogicalConversationItem]) -> u64 {
        let Some(prefix) = &self.prefix else {
            return 0;
        };
        let added = if exchange.is_empty() {
            0
        } else {
            serde_json::to_vec(exchange).map_or(u64::MAX, |bytes| bytes.len() as u64)
        };
        prefix.room.saturating_sub(added)
    }
}

/// The request the step would have sent with the compaction asked after it, or `None` when it
/// cannot be asked that way. Its task state is the step's, the activities it lists included; only
/// the isolated request leaves them out.
pub(super) fn attempt(
    prefix: &RequestPrefix,
    ask: &str,
    compaction: ContextProjectionSnapshotId,
    snapshot: &ModelExecutionSnapshot,
    fixed_tool_ids: &[ToolId],
    resolved_attachments: Arc<[aura_model_domain::ResolvedModelAttachment]>,
) -> anyhow::Result<Option<PrefixAttempt>> {
    if prefix.provider_continuation {
        return Ok(None);
    }
    // Counted as the step's request plus the ask.
    let added = ask.len() as u64;
    let window = snapshot.runtime_effective_window_tokens;
    let safety = aura_context_domain::safety_reserve_v1(window)
        .map_err(|_| anyhow::anyhow!("context.compaction_output_budget"))?;
    let room = window.saturating_sub(safety).saturating_sub(prefix.estimated_input_tokens + added);
    let configured = snapshot.request_options.effective_max_output_tokens;
    let max_output = configured.min(room);
    if max_output < configured.min(MIN_OUTPUT_TOKENS) {
        return Ok(None);
    }
    let mut egress_sources = prefix.egress_sources.clone();
    egress_sources.push(ModelEgressSourceDraft {
        source_ref: ContextSourceRef::Compaction { context_snapshot_id: compaction },
        source_kind: ModelEgressSourceKind::Compaction,
        data_class: aura_security_contract::NetworkDataClass::WorkspaceSource,
        content_hash: ContentHash::sha256(ask.as_bytes()),
        byte_length: ask.len() as u64,
        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
    });
    Ok(Some(PrefixAttempt {
        envelope: prefix.instruction_envelope.clone(),
        ask: ask.to_owned(),
        conversation_items: prefix.conversation_items.clone(),
        tool_schema_set: prefix.tool_schema_set.clone(),
        fixed_tool_ids: fixed_tool_ids.to_vec(),
        attachments: prefix.attachments.clone(),
        resolved_attachments,
        egress_sources,
        max_output,
        room,
    }))
}
