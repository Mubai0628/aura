//! A request measured by its provider's count of the previous one counts each part that count did
//! not include by what it is: the model's own record of its answer at the provider's rate, text by
//! character class or at that rate where it is higher.
use super::*;

#[test]
fn the_models_record_of_its_answer_is_counted_at_the_providers_rate() {
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
        data: aura_model_domain::ProviderHistoryData::Responses {
            reasoning_items: vec![reasoning],
        },
    });
    // A tenth of a token a byte, for a request none of whose parts this one sends again.
    input.previous_measurement =
        MeasuredRequest::new(10_000, [(ContentHash::sha256(b"earlier"), 100_000)]);
    let prepared = compile_prepared(input);
    let estimates = &prepared.context.snapshot.budget.item_estimates;
    let (records, text): (Vec<_>, Vec<_>) =
        estimates.iter().partition(|item| item.byte_length > 30_000);
    assert_eq!(records.len(), 1, "{estimates:#?}");
    assert_eq!(records[0].estimated_tokens, records[0].byte_length.div_ceil(10));
    assert!(!text.is_empty());
    for item in text {
        assert!(item.estimated_tokens >= item.byte_length / 3, "text by character class: {item:?}");
    }
}
