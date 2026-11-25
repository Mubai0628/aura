//! Resolve only continuation artifacts owned by the exact producing ModelStep.
use aura_domain_common::{ContentHash, EventId, ModelStepId};
use aura_model_domain::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn native_prefixes(
    db: &Connection,
    items: &[LogicalConversationItem],
) -> StoreResult<BTreeMap<EventId, ContentHash>> {
    let mut query = db.prepare(
        "SELECT b.payload FROM events e JOIN events b
         ON b.owner=json_extract(e.payload,'$.model_step_id') AND b.kind='agent.request_boundary'
         WHERE e.event_id=?1 AND e.kind='agent.assistant_response'
         ORDER BY b.sequence DESC LIMIT 1",
    )?;
    let mut prefixes = BTreeMap::new();
    for item in items {
        let LogicalConversationItem::Assistant {
            source_event_id, provider_history: Some(_), ..
        } = item
        else {
            continue;
        };
        let raw: Option<String> =
            query.query_row([source_event_id.to_string()], |row| row.get(0)).optional()?;
        if let Some(prefix) = raw
            .and_then(|value| {
                serde_json::from_str::<super::super::request_boundary::RequestBoundary>(&value).ok()
            })
            .filter(|boundary| boundary.closing_question.is_none())
            .and_then(|boundary| boundary.native_prefix)
        {
            prefixes.insert(*source_event_id, prefix);
        }
    }
    Ok(prefixes)
}

pub(super) fn read(
    db: &Connection,
    step: ModelStepId,
    snapshot: &ModelExecutionSnapshot,
    output: &NormalizedModelOutput,
) -> StoreResult<Option<LogicalProviderHistory>> {
    let payload = match &output.provider_continuation {
        Some(
            ProviderContinuationState::OpenAiResponses { state_ref }
            | ProviderContinuationState::Anthropic { state_ref }
            | ProviderContinuationState::Opaque { state_ref, .. },
        ) => {
            let raw = artifact::read(db, *state_ref, &step.to_string())?;
            Some(serde_json::from_slice::<Value>(&raw)?)
        }
        None => None,
    };
    let data = match snapshot.api_format {
        ModelApiFormat::OpenAiChatCompletions => {
            // Only thinking that came as `reasoning_content` goes back in that field: the decoder
            // names any other field it read in the block's reference, and a service that sent
            // thinking under another name may refuse `reasoning_content` in its history.
            let reasoning = output
                .reasoning_blocks
                .iter()
                .filter(|b| b.provider_reference.as_ref().is_none_or(|r| r.block_id.is_none()))
                .map(|b| b.text.as_str())
                .collect::<String>();
            let signatures: BTreeMap<String, String> = payload
                .as_ref()
                .filter(|p| p["format"] == "openai_chat_continuation_v1")
                .map(|p| serde_json::from_value(p["thought_signatures"].clone()))
                .transpose()?
                .unwrap_or_default();
            if reasoning.is_empty() && signatures.is_empty() {
                return Ok(None);
            }
            ProviderHistoryData::Chat {
                reasoning_content: (!reasoning.is_empty()).then_some(reasoning),
                thought_signatures: signatures,
            }
        }
        ModelApiFormat::OpenAiResponses => {
            let Some(payload) = payload else { return Ok(None) };
            if payload["format"] != "openai_responses_reasoning_items_v1" {
                return Err(Error::Conflict);
            }
            let items = payload["items"].as_array().ok_or(Error::Conflict)?;
            if items.iter().any(|i| i["type"] != "reasoning") {
                return Err(Error::Conflict);
            }
            ProviderHistoryData::Responses { reasoning_items: items.clone() }
        }
        ModelApiFormat::AnthropicMessages => {
            let Some(payload) = payload else { return Ok(None) };
            if payload["format"] != "anthropic_messages_thinking_v1" {
                return Err(Error::Conflict);
            }
            let blocks = payload["blocks"]
                .as_array()
                .ok_or(Error::Conflict)?
                .iter()
                .map(|b| {
                    if let Some(data) = b["data"].as_str() {
                        return Ok(json!({"type":"redacted_thinking","data":data}));
                    }
                    let thinking = b["thinking"].as_str().ok_or(Error::Conflict)?;
                    let signature =
                        b["signature"].as_str().filter(|s| !s.is_empty()).ok_or(Error::Conflict)?;
                    Ok(json!({"type":"thinking","thinking":thinking,"signature":signature}))
                })
                .collect::<StoreResult<Vec<_>>>()?;
            ProviderHistoryData::Anthropic { thinking_blocks: blocks }
        }
    };
    Ok(Some(LogicalProviderHistory {
        reuse_domain: ProviderToolCallReuseDomain {
            endpoint_id: snapshot.endpoint_id,
            endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
            api_format: snapshot.api_format,
        },
        model_name: snapshot.model_name.clone(),
        data,
        reasoning_tokens: (output.usage.source == UsageSource::ProviderReported)
            .then_some(output.usage.reasoning_tokens)
            .flatten()
            .filter(|tokens| *tokens > 0 && *tokens <= output.usage.output_tokens),
    }))
}
