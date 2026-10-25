use super::*;

/// A load changes the session's membership and nothing about the turn's own list; the
/// registry a later turn builds carries the tool.
#[tokio::test]
async fn registry_search_and_load_apply_the_load_from_the_next_turn() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let frozen = f.registry();
    let entry = aura_tool_registry::agent_release::release()
        .expect("Agent test setup and persisted assertion")
        .entry("http.request")
        .expect("Agent test setup and persisted assertion");
    let batch = f.admit(&[
        ("tools.search", json!({"query":"http.request","limit":5,"cursor":null})),
        ("tools.describe", json!({"tool_id":entry.definition.tool_id})),
        ("tools.load", json!({"tool_ids":[entry.definition.tool_id],"replace":[]})),
        ("tools.list_loaded", json!({})),
    ]);
    let ids = batch.batch.calls.iter().map(|c| c.tool_call_id).collect::<Vec<_>>();
    f.execute(batch).await;
    let search: ToolSearchPageV1 =
        serde_json::from_value(f.output(ids[0])).expect("Agent test setup and persisted assertion");
    assert!(search.results.iter().any(|v| v.tool_id == entry.definition.tool_id));
    let description: ToolDescribeResultV1 =
        serde_json::from_value(f.output(ids[1])).expect("Agent test setup and persisted assertion");
    assert_eq!(&description.schema.input_schema, entry.definition.input_schema.document());
    let loaded: ToolLoadResultV1 =
        serde_json::from_value(f.output(ids[2])).expect("Agent test setup and persisted assertion");
    assert!(loaded.changed);
    assert_eq!(
        loaded.resulting_registry_snapshot_ref, frozen.snapshot_ref,
        "the turn keeps its list"
    );
    let list: ToolListLoadedResultV1 =
        serde_json::from_value(f.output(ids[3])).expect("Agent test setup and persisted assertion");
    assert!(!list.tools.iter().any(|v| v.definition_ref.tool_id == entry.definition.tool_id));
    let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    let db = store.database.connection();
    let membership = aura_persistence_v2::tool::session_tools::load(db, f.turn.session_id)
        .expect("Agent test setup and persisted assertion")
        .expect("Agent test setup and persisted assertion");
    assert!(membership.tool_ids.contains(&entry.definition.tool_id), "the next turn loads it");
    assert_eq!(
        aura_persistence_v2::tool::registry(db, frozen.snapshot_ref.tool_registry_snapshot_id)
            .expect("Agent test setup and persisted assertion")
            .expect("Agent test setup and persisted assertion")
            .as_ref(),
        &frozen
    );
    // Until then, the tool is offered through the entry.
    assert!(frozen.entry_bindings.iter().any(|b| b.tool_id == entry.definition.tool_id));
}

#[tokio::test]
async fn context_returns_actual_sources_and_keeps_tool_history_whole() {
    use aura_context_domain::*;
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["context.read_events"]).await;
    let input = AgentContextReadEventsInput {
        selectors: vec![ContextEventSelector::Event { event_id: f.turn.root_user_event_id }],
        character_limit: 32,
        cursor: None,
    };
    let batch = f.admit(&[(
        "context.read_events",
        serde_json::to_value(&input).expect("Agent test setup and persisted assertion"),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let group = f.execute(batch).await;
    let read: ContextReadEventsResult =
        serde_json::from_value(f.output(call)).expect("Agent test setup and persisted assertion");
    assert_eq!(read.events.len(), 1);
    assert_eq!(read.events[0].event_id, f.turn.root_user_event_id);
    assert_eq!(read.events[0].content, "Read and verify the requested workspace files");
    let mut store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    let group_input = AgentContextReadEventsInput {
        selectors: vec![ContextEventSelector::HistoryAtomicGroup {
            group_ref: group.group_ref().expect("Agent test setup and persisted assertion"),
        }],
        character_limit: 1,
        cursor: None,
    };
    let whole = store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::context::read::events(
                tx,
                f.turn.session_id,
                f.turn.agent_run_id,
                &group_input,
                UtcTimestamp::now(),
            )
        })
        .expect("Agent test setup and persisted assertion");
    assert_eq!(whole.events.len(), 1);
    assert!(whole.next_cursor.is_none());
    assert!(whole.events[0].content.contains(&call.to_string()));
    let result_input =
        AgentContextReadToolResultInput { tool_call_id: call, include_input: true, cursor: None };
    let result = store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::context::read::tool_result(
                tx,
                f.turn.session_id,
                f.turn.agent_run_id,
                &result_input,
                UtcTimestamp::now(),
            )
        })
        .expect("Agent test setup and persisted assertion");
    assert!(result.value.result.contains(&f.turn.root_user_event_id.to_string()));
    assert!(
        store
            .database
            .unit_of_work(|tx| aura_persistence_v2::context::read::events(
                tx,
                SessionId::new(),
                f.turn.agent_run_id,
                &input,
                UtcTimestamp::now()
            ))
            .is_err()
    );
    let forged = AgentContextReadEventsInput { cursor: Some(EventId::new().to_string()), ..input };
    assert!(
        store
            .database
            .unit_of_work(|tx| aura_persistence_v2::context::read::events(
                tx,
                f.turn.session_id,
                f.turn.agent_run_id,
                &forged,
                UtcTimestamp::now()
            ))
            .is_err()
    );
}
