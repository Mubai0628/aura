//! A Chat step's thinking goes back with its answer in `reasoning_content` only when it came in
//! that field: a service that sent it under another name may refuse `reasoning_content` in its
//! history. Records written before the decoder named other fields carry no name and go back.
#![allow(clippy::unwrap_used)]
use aura_domain_common::ModelContentBlockId;
use aura_model_domain::{
    ModelApiFormat, ProviderHistoryData, ProviderProtocolReference, ReasoningContentBlock,
};
use aura_persistence_v2::agent::model_context;

fn sent_back(reference: Option<ProviderProtocolReference>) -> Option<String> {
    let canonical = aura_contract_schema::canonical_fixture().unwrap();
    let mut snapshot = canonical.models.execution_snapshot.clone().unwrap();
    snapshot.api_format = ModelApiFormat::OpenAiChatCompletions;
    let mut output = canonical.models.output.clone().unwrap();
    output.provider_continuation = None;
    output.reasoning_blocks = vec![ReasoningContentBlock {
        block_id: ModelContentBlockId::new(),
        text: "Reading the file first.".to_owned(),
        summary: None,
        provider_reference: reference,
    }];
    let db = rusqlite::Connection::open_in_memory().unwrap();
    let history =
        model_context::provider_history(&db, output.model_step_id, &snapshot, &output).unwrap();
    history.and_then(|history| match history.data {
        ProviderHistoryData::Chat { reasoning_content, .. } => reasoning_content,
        _ => None,
    })
}

fn named(field: Option<&str>) -> Option<ProviderProtocolReference> {
    Some(ProviderProtocolReference {
        item_id: None,
        block_id: field.map(ToOwned::to_owned),
        index: Some(0),
    })
}

#[test]
fn thinking_from_reasoning_content_goes_back_with_the_answer() {
    assert_eq!(sent_back(named(None)).as_deref(), Some("Reading the file first."));
    assert_eq!(sent_back(None).as_deref(), Some("Reading the file first."));
}

#[test]
fn thinking_from_another_field_is_not_sent_back() {
    for field in ["reasoning", "reasoning_text", "reasoning_details"] {
        assert_eq!(sent_back(named(Some(field))), None, "{field}");
    }
}
