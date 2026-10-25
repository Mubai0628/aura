use super::*;

pub(crate) async fn run_p08_builtin_vertical_fixture_v1() {
    let fixture = BuiltinVerticalFixture::new();
    let initial_ref = fixture.initial_registry_snapshot_ref.clone();
    let external_initial = fixture.external_definition_ref(&initial_ref);

    let search = fixture
        .execute(
            &initial_ref,
            BuiltinCoreHandlerV1::ToolsSearch,
            serde_json::to_value(ToolSearchRequestV1 {
                query: ToolSearchQueryV1::new("fixture").expect("search query"),
                limit: 10,
                cursor: None,
            })
            .expect("search arguments"),
        )
        .await;
    assert_eq!(search.handler, BuiltinCoreHandlerV1::ToolsSearch);
    assert_eq!(search.result.side_effect_state, SideEffectStateV1::NoEffect);
    assert!(search.resulting_registry_snapshot_ref.is_none());
    assert_eq!(search.changed, None);

    let describe = fixture
        .execute(
            &initial_ref,
            BuiltinCoreHandlerV1::ToolsDescribe,
            serde_json::to_value(ToolDescribeRequestV1 {
                definition_ref: external_initial.clone(),
            })
            .expect("describe arguments"),
        )
        .await;
    assert_eq!(describe.handler, BuiltinCoreHandlerV1::ToolsDescribe);
    assert_eq!(describe.result.side_effect_state, SideEffectStateV1::NoEffect);
    assert!(describe.resulting_registry_snapshot_ref.is_none());

    let listed = fixture
        .execute(
            &initial_ref,
            BuiltinCoreHandlerV1::ToolsListLoaded,
            serde_json::to_value(ToolListLoadedRequestV1 {
                registry_snapshot_ref: initial_ref.clone(),
            })
            .expect("list-loaded arguments"),
        )
        .await;
    assert_eq!(listed.handler, BuiltinCoreHandlerV1::ToolsListLoaded);
    assert_eq!(listed.result.side_effect_state, SideEffectStateV1::NoEffect);

    let loaded = fixture
        .execute(
            &initial_ref,
            BuiltinCoreHandlerV1::ToolsLoad,
            serde_json::to_value(ToolLoadRequestV1 { definitions: vec![external_initial] })
                .expect("load arguments"),
        )
        .await;
    assert_eq!(loaded.changed, Some(true));
    assert_eq!(loaded.result.side_effect_state, SideEffectStateV1::FullyApplied);
    let loaded_ref = loaded
        .resulting_registry_snapshot_ref
        .clone()
        .expect("changed load returns exact successor Registry");
    assert_ne!(loaded_ref, initial_ref);
    assert_eq!(
        ToolRegistryControlServiceV1
            .list_loaded(&fixture.persistence, &loaded_ref)
            .expect("list loaded after exact Built-in load")
            .iter()
            .filter(|definition| {
                definition.tool_id == fixture.external_candidate.definition.tool_id
            })
            .count(),
        1
    );

    let load_noop = fixture
        .execute(
            &loaded_ref,
            BuiltinCoreHandlerV1::ToolsLoad,
            serde_json::to_value(ToolLoadRequestV1 {
                definitions: vec![fixture.external_definition_ref(&loaded_ref)],
            })
            .expect("no-op load arguments"),
        )
        .await;
    assert_eq!(load_noop.changed, Some(false));
    assert_eq!(load_noop.result.side_effect_state, SideEffectStateV1::NoEffect);
    assert_eq!(load_noop.resulting_registry_snapshot_ref.as_ref(), Some(&loaded_ref));

    let unloaded = fixture
        .execute(
            &loaded_ref,
            BuiltinCoreHandlerV1::ToolsUnload,
            serde_json::to_value(ToolLoadRequestV1 {
                definitions: vec![fixture.external_definition_ref(&loaded_ref)],
            })
            .expect("unload arguments"),
        )
        .await;
    assert_eq!(unloaded.changed, Some(true));
    assert_eq!(unloaded.result.side_effect_state, SideEffectStateV1::FullyApplied);
    let unloaded_ref = unloaded
        .resulting_registry_snapshot_ref
        .clone()
        .expect("changed unload returns exact successor Registry");
    assert_ne!(unloaded_ref, loaded_ref);
    assert!(
        ToolRegistryControlServiceV1
            .list_loaded(&fixture.persistence, &unloaded_ref)
            .expect("list loaded after exact Built-in unload")
            .iter()
            .all(|definition| {
                definition.tool_id != fixture.external_candidate.definition.tool_id
            })
    );

    let unload_noop = fixture
        .execute(
            &unloaded_ref,
            BuiltinCoreHandlerV1::ToolsUnload,
            serde_json::to_value(ToolLoadRequestV1 {
                definitions: vec![fixture.external_definition_ref(&unloaded_ref)],
            })
            .expect("no-op unload arguments"),
        )
        .await;
    assert_eq!(unload_noop.changed, Some(false));
    assert_eq!(unload_noop.result.side_effect_state, SideEffectStateV1::NoEffect);
    assert_eq!(unload_noop.resulting_registry_snapshot_ref.as_ref(), Some(&unloaded_ref));

    let attempt_ids = [
        search.attempt_id,
        describe.attempt_id,
        listed.attempt_id,
        loaded.attempt_id,
        load_noop.attempt_id,
        unloaded.attempt_id,
        unload_noop.attempt_id,
    ];
    assert_eq!(attempt_ids.iter().collect::<std::collections::BTreeSet<_>>().len(), 7);
    let connection = Connection::open(&fixture.database_path).expect("open Built-in database");
    let consumed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tool_builtin_core_handler_dispatch_bindings
                 WHERE state='consumed' AND consumed_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("count consumed P04 Built-in bindings");
    let distinct_operations: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT security_operation_id)
                 FROM tool_builtin_core_handler_dispatch_bindings",
            [],
            |row| row.get(0),
        )
        .expect("count exact P04 Built-in operations");
    let registry_snapshots: i64 = connection
        .query_row("SELECT COUNT(*) FROM tool_registry_snapshots", [], |row| row.get(0))
        .expect("count immutable Registry snapshots");
    let results: i64 = connection
        .query_row("SELECT COUNT(*) FROM tool_results", [], |row| row.get(0))
        .expect("count unique terminal Tool Results");
    assert_eq!(consumed, 7);
    assert_eq!(distinct_operations, 7);
    assert_eq!(registry_snapshots, 3, "initial plus changed load and unload only");
    assert_eq!(results, 7);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_builtins_use_p05_p08_p04_and_exact_atomic_registry_handlers() {
    run_p08_builtin_vertical_fixture_v1().await;
}
