use aura_context_domain::{ContextPreparationTriggerV1, ContextSourceRef};
use aura_context_fixtures::p06_performance_preparation_input;
use aura_context_runtime::{
    ContextEstimatorRegistry, ContextPreparationCompilationOutcome, ContextPreparationCompiler,
};
use aura_domain_common::EventId;

#[test]
fn batch_requires_distinct_authored_sources_and_preserves_their_identity() {
    let mut input = p06_performance_preparation_input(262_144, 100).expect("fixture");
    let ContextPreparationTriggerV1::UserSubmission { user_event_id: first } = input.trigger else {
        panic!("user source")
    };
    let second = EventId::new();
    let mut fragment = input.section_inputs[1].fragments[0].clone();
    fragment.source_ref = ContextSourceRef::Event { event_id: second };
    fragment.content = "second independent instruction".into();
    input.section_inputs[1].fragments.push(fragment);
    input.trigger = ContextPreparationTriggerV1::UserBatch { user_event_ids: vec![first, second] };
    let estimators = ContextEstimatorRegistry::default();
    let compiler = ContextPreparationCompiler::new(&estimators);
    let outcome = compiler.compile(input.clone()).expect("compile independent inputs");
    let ContextPreparationCompilationOutcome::Prepared(prepared) = outcome else {
        panic!("batch fits budget")
    };
    let items = &prepared.context.conversation_items;
    assert_eq!(items.len(), 2);
    assert!(
        matches!(&items[0], aura_model_domain::LogicalConversationItem::User { source_event_id, content, .. } if *source_event_id == first && content == &"x".repeat(100))
    );
    assert!(
        matches!(&items[1], aura_model_domain::LogicalConversationItem::User { source_event_id, content, .. } if *source_event_id == second && content == "second independent instruction")
    );
    let mut duplicate = input.clone();
    duplicate.trigger =
        ContextPreparationTriggerV1::UserBatch { user_event_ids: vec![first, first] };
    assert!(compiler.compile(duplicate).is_err(), "one source cannot be consumed twice");
    let mut invented = input.clone();
    invented.trigger =
        ContextPreparationTriggerV1::UserBatch { user_event_ids: vec![first, EventId::new()] };
    assert!(
        compiler.compile(invented).is_err(),
        "missing user source cannot become an instruction"
    );
    input.section_inputs[1].fragments[1].source_kind =
        aura_context_domain::ModelEgressSourceKind::ToolResult;
    assert!(compiler.compile(input).is_err(), "tool output cannot impersonate authenticated input");
}
