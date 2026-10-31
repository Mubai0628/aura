//! An answer to a compaction that called tools instead of giving the summary, sent back after the
//! question with a result for each call: no tool runs, and the summary is still asked for. All
//! before it is the request already sent, so the provider reads it from its cache, and the model
//! answers again from what it already thought - its reasoning or thinking goes back with its calls,
//! as a thinking model requires.
use super::*;

/// What each call is answered with.
const NO_TOOL_RUNS: &str = "No tool runs during a compaction request. Write the summary now, \
                            between <summary> and </summary>, and call no tool.";

pub(super) struct Exchange {
    pub items: Vec<LogicalConversationItem>,
    pub bindings: Vec<ProviderToolCallWireBinding>,
    pub source: ModelEgressSourceDraft,
}

impl Execution {
    /// `failed`'s request with the answer it got, which called tools, and their results.
    pub(super) async fn answer_tools(
        &self,
        failed: &PreparedCompactionModelAttempt,
        ordinal: u8,
    ) -> anyhow::Result<PreparedCompactionModelAttempt> {
        let snapshot = failed.execution_snapshot.clone();
        let (output, history) = self
            .app
            .store
            .read(
                StoreStep::run(self.record.agent_run_id, "agent.compaction.tool_answer"),
                move |db| {
                    let output = aura_persistence_v2::context::compaction::output(db, snapshot.id)?;
                    let history = aura_persistence_v2::agent::model_context::provider_history(
                        db,
                        snapshot.model_step_id,
                        &snapshot,
                        &output,
                    )?;
                    Ok((output, history))
                },
            )
            .await?;
        let exchange = answered(failed, &output, history, self.record.id)?;
        let mut follows = failed.logical_request.closing_exchange.clone();
        follows.extend(exchange.items.iter().cloned());
        let limit = failed
            .logical_request
            .output_request
            .effective_max_output_tokens
            .min(self.room_for(&follows))
            .max(1);
        self.ask_again(failed, ordinal, Some(limit), Some(exchange))
    }
}

/// The answer and its calls' results as the request sends them, with the wire binding of each call
/// and the egress source they make.
fn answered(
    failed: &PreparedCompactionModelAttempt,
    output: &NormalizedModelOutput,
    history: Option<aura_model_domain::LogicalProviderHistory>,
    compaction: ContextProjectionSnapshotId,
) -> anyhow::Result<Exchange> {
    anyhow::ensure!(!output.tool_calls.is_empty(), "context.compaction_tool_answer_missing");
    let names = aura_model_transport::provider_tool_name_bindings(&failed.logical_request)
        .map_err(|error| anyhow::anyhow!("context.compaction_tool_answer_names: {error}"))?;
    let snapshot = &failed.execution_snapshot;
    let reuse_domain = aura_model_domain::ProviderToolCallReuseDomain {
        endpoint_id: snapshot.endpoint_id,
        endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
        api_format: snapshot.api_format,
    };
    let mut made = output.tool_calls.iter().collect::<Vec<_>>();
    made.sort_by_key(|call| call.position);
    let (mut calls, mut bindings, mut results) = (Vec::new(), Vec::new(), Vec::new());
    for call in made {
        let named = names.iter().find(|binding| binding.provider_wire_name == call.tool_name);
        let tool_id = named.map(|binding| binding.tool_definition.tool_id);
        let name = named.map_or_else(|| call.tool_name.clone(), |b| b.canonical_name.clone());
        let arguments = aura_model_domain::history_tool_call_arguments_v1(&call.arguments_json);
        calls.push(aura_model_domain::LogicalHistoricalToolCall {
            tool_call_id: call.tool_call_id,
            tool_id,
            canonical_name: name.clone(),
            arguments_hash: ContentHash::sha256(arguments.as_bytes()),
            arguments_json: arguments,
        });
        bindings.push(ProviderToolCallWireBinding {
            tool_call_id: call.tool_call_id,
            tool_id,
            provider_wire_tool_name: call.tool_name.clone(),
            wire_identity: aura_model_domain::ProviderToolCallWireIdentity::ProviderOpaque {
                provider_tool_call_id: call.provider_call_id.clone(),
                reuse_domain: reuse_domain.clone(),
            },
        });
        results.push(LogicalConversationItem::ToolResult {
            source_event_id: EventId::new(),
            tool_call_id: call.tool_call_id,
            tool_name: name,
            content: NO_TOOL_RUNS.to_owned(),
            content_hash: ContentHash::sha256(NO_TOOL_RUNS.as_bytes()),
            is_error: true,
        });
    }
    let text = output.assistant_blocks.iter().map(|block| block.text.as_str()).collect::<String>();
    let content = (!text.trim().is_empty()).then_some(text);
    let mut items = vec![LogicalConversationItem::Assistant {
        source_event_id: EventId::new(),
        content_hash: content.as_ref().map(|text| ContentHash::sha256(text.as_bytes())),
        content,
        tool_calls: calls,
        provider_history: history,
    }];
    items.extend(results);
    let bytes = serde_json::to_vec(&items)?;
    let source = ModelEgressSourceDraft {
        source_ref: ContextSourceRef::Compaction { context_snapshot_id: compaction },
        source_kind: ModelEgressSourceKind::Compaction,
        data_class: aura_security_contract::NetworkDataClass::WorkspaceSource,
        content_hash: ContentHash::sha256(&bytes),
        byte_length: bytes.len() as u64,
        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
    };
    Ok(Exchange { items, bindings, source })
}
