//! The context compiler counts replayable provider records independently of prior input usage.
use super::*;

#[test]
fn native_reasoning_uses_its_producing_answers_usage_once() {
    let mut input = tool_continuation_input(false);
    let reuse_domain = aura_model_domain::ProviderToolCallReuseDomain {
        endpoint_id: input.provider_target.endpoint_id,
        endpoint_security_fingerprint: input.provider_target.endpoint_fingerprint.clone(),
        api_format: input.provider_target.api_format,
    };
    let LogicalConversationItem::Assistant { provider_history, .. } =
        &mut input.recent_history_items[1]
    else {
        panic!("the model's call");
    };
    let reasoning =
        serde_json::json!({"type": "reasoning", "encrypted_content": "e".repeat(30_000)});
    *provider_history = Some(aura_model_domain::LogicalProviderHistory {
        reuse_domain,
        model_name: "fixture-model".to_owned(),
        reasoning_tokens: Some(4_200),
        data: aura_model_domain::ProviderHistoryData::Responses {
            reasoning_items: vec![reasoning.clone(), reasoning],
        },
    });
    assert!(input.previous_measurement.is_none());
    let prepared = compile_prepared(input);
    let estimates = &prepared.context.snapshot.budget.item_estimates;
    let (records, text): (Vec<_>, Vec<_>) =
        estimates.iter().partition(|item| item.byte_length > 30_000);
    assert_eq!(records.len(), 1, "{estimates:#?}");
    assert_eq!(records[0].estimated_tokens, 4_200);
    assert!(!text.is_empty());
}
