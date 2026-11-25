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

#[test]
fn a_deepseek_history_rebuild_counts_the_text_its_adapter_sends() {
    use crate::preparation::budget_items::{
        RequestBudgetParts, build_pending_budget_items, estimator_inputs,
    };
    use aura_model_domain::{
        CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION, EndpointAuthScheme, ProviderToolCallId,
        ProviderToolCallReuseDomain, ProviderToolCallWireBinding, ProviderToolCallWireIdentity,
        ReasoningEffort, ResolvedModelRequestOptions,
    };
    use aura_tool_contract::{ToolInputSchemaV1, ToolSchemaSnapshotV1, ToolVersion};

    let mut input = tool_continuation_input(false);
    let target = &input.provider_target;
    let mut options = ResolvedModelRequestOptions::output_only(4096);
    options.reasoning_effort = Some(ReasoningEffort::High);
    let LogicalConversationItem::Assistant { tool_calls, .. } = &input.recent_history_items[1]
    else {
        panic!("assistant tool call")
    };
    let call = &tool_calls[0];
    let snapshot = ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: input.request.model_step_id,
        provider_id: target.provider_id,
        endpoint_id: target.endpoint_id,
        model_profile_id: target.model_profile_id,
        provider_display_name: "DeepSeek fixture".into(),
        endpoint_display_name: "DeepSeek fixture".into(),
        model_name: "deepseek-flash".into(),
        readable_ref: "deepseek-flash".into(),
        api_format: ModelApiFormat::OpenAiResponses,
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.deepseek.com".into(),
        resolved_request_url: "https://api.deepseek.com/responses".into(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        chat_output_token_field: None,
        chat_instruction_role: None,
        endpoint_config_revision: 1,
        model_config_revision: 1,
        runtime_effective_window_tokens: 128_000,
        historical_snapshot_max_tokens: 4_096,
        request_options: options,
        historical_tool_call_wire_bindings: vec![ProviderToolCallWireBinding {
            tool_call_id: call.tool_call_id,
            tool_id: call.tool_id,
            provider_wire_tool_name: "fixture_read".into(),
            wire_identity: ProviderToolCallWireIdentity::ProviderOpaque {
                provider_tool_call_id: ProviderToolCallId::new("call_fixture")
                    .expect("fixture provider call ID"),
                reuse_domain: ProviderToolCallReuseDomain {
                    endpoint_id: target.endpoint_id,
                    endpoint_security_fingerprint: target.endpoint_fingerprint.clone(),
                    api_format: target.api_format,
                },
            },
        }],
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: input.prompt_bundle_snapshot.id,
        endpoint_fingerprint: target.endpoint_fingerprint.clone(),
        created_at: UtcTimestamp::now(),
    };
    let mut assembly = assemble_context_sections(input.section_inputs).expect("prompt assembly");
    assembly.instruction_envelope.serializer_version = CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION;
    let schemas = ToolSchemaSetSnapshotV1::new(
        input.tool_schema_set.registry_snapshot_ref,
        vec![
            ToolSchemaSnapshotV1::new(
                ToolId::new(),
                ToolVersion::new(None, Revision::INITIAL).expect("fixture tool version"),
                "fixture.read".parse().expect("fixture tool name"),
                "Fixture reader".into(),
                ToolInputSchemaV1::new(serde_json::json!({"type":"object","properties":{}}))
                    .expect("fixture input schema"),
                false,
            )
            .expect("fixture tool snapshot"),
        ],
    )
    .expect("fixture schema set");
    let LogicalConversationItem::ToolResult { content, content_hash, .. } =
        &mut input.recent_history_items[2]
    else {
        panic!("tool result")
    };
    *content = "1\t{\"path\":\"test\"}\n".repeat(3000);
    *content_hash = ContentHash::sha256(content.as_bytes());
    let result_hash = content_hash.clone();
    let history = &input.recent_history_items;
    let (_, archive) = aura_model_transport::archived_deepseek_group(&history[1..], &snapshot)
        .expect("adapter archives the closed group");
    let pending = build_pending_budget_items(
        &assembly,
        &schemas,
        &[],
        &[],
        RequestBudgetParts {
            recent_history: history,
            new_users: &[],
            split_user_sections: false,
            provider: Some(&snapshot),
            closing_question: None,
        },
    )
    .expect("build the request budget items");
    let (text, _) = estimator_inputs(&pending).expect("measure the archived request");
    assert!(text.iter().any(|item| item.canonical_text == archive));
    assert!(!text.iter().any(|item| item.content_hash == result_hash));
}
