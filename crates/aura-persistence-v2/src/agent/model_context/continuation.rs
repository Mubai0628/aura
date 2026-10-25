//! Resolve only continuation artifacts owned by the exact producing ModelStep.
use aura_domain_common::ModelStepId;
use aura_model_domain::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
            let reasoning =
                output.reasoning_blocks.iter().map(|b| b.text.as_str()).collect::<String>();
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
    }))
}
