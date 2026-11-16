use super::*;

#[test]
fn an_existing_tool_result_and_summary_keep_their_sources_and_send_printable_text() {
    let mut input = tool_continuation_input(false);
    let raw = "log\u{1b}[31m红色\u{7f}";
    let result = input.recent_history_items.last_mut().expect("historic tool result");
    let LogicalConversationItem::ToolResult { source_event_id, content, content_hash, .. } = result
    else {
        panic!("tool result");
    };
    let source = *source_event_id;
    *content = raw.to_owned();
    *content_hash = ContentHash::sha256(raw.as_bytes());
    let history = input
        .section_inputs
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::RecentHistory)
        .expect("history section");
    let result_fragment = history.fragments.last_mut().expect("result fragment");
    result_fragment.content = raw.to_owned();
    let mut summary = result_fragment.clone();
    summary.source_ref =
        aura_context_domain::ContextSourceRef::Artifact { artifact_id: ArtifactId::new() };
    summary.content = "\u{feff}saved\u{1b} summary".to_owned();
    input.section_inputs.push(ContextSectionInput {
        kind: ContextSectionKind::HistoricalSnapshotSummary,
        fragments: vec![summary],
        compressibility: ContextCompressibility::CompressibleHistory,
        privacy_class: ContextPrivacyClass::ModelVisible,
    });
    let prepared = compile_prepared(input);
    assert!(prepared.context.instruction_envelope.canonical_text.contains(r"saved\u001b summary"));
    assert!(matches!(
        &prepared.context.conversation_items.last(),
        Some(LogicalConversationItem::ToolResult { source_event_id, content, .. })
            if *source_event_id == source && content == r"log\u001b[31m红色\u007f"
    ));
}

#[test]
fn current_user_controls_are_escaped_in_the_budgeted_request() {
    let mut input = input("core\n".to_owned());
    input.section_inputs[1].fragments[0].content = "\u{feff}hi\r\nthere\t\u{feff}\0".to_owned();
    let prepared = compile_prepared(input);
    assert!(matches!(
        &prepared.context.conversation_items[0],
        LogicalConversationItem::User { content, content_hash, .. }
            if content == "hi\nthere\t\\ufeff\\u0000"
                && *content_hash == ContentHash::sha256(content.as_bytes())
    ));
}
