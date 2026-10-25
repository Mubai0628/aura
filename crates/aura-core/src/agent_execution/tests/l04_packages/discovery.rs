//! Installed package tools are found, described and loaded through the same
//! built-in registry tools as optional built-ins; nothing exposes them directly.
use super::support::*;
use super::*;
use crate::package_program::tests::support::Kind;

#[tokio::test]
async fn l04_installed_package_tools_are_searched_described_and_loaded_like_other_tools() {
    let (f, installed) = Fixture::with_package_exposure(
        PermissionMode::ReadOnly,
        &["tools.search", "tools.describe", "tools.load", "tools.list_loaded"],
        fixture::Packages::Discoverable,
        |service| install(service, Kind::Command, |_| {}),
    )
    .await;
    let tool_id = installed.summary.installed.as_ref().expect("installed").tools[0].tool_id;
    let frozen = f.registry();
    let entry = frozen
        .catalog_entries
        .iter()
        .find(|entry| entry.tool_id == tool_id)
        .expect("an installed package tool is a catalog entry");
    assert_eq!(entry.load_state, ToolLoadStateV1::Discoverable, "never exposed as Core");
    assert!(!frozen.schema_set.tools.iter().any(|schema| schema.tool_id == tool_id));

    let batch = f.admit(&[
        ("tools.search", json!({"query": "package fixture", "limit": 5, "cursor": null})),
        ("tools.describe", json!({"tool_id": tool_id})),
        ("tools.load", json!({"tool_ids": [tool_id], "replace": []})),
    ]);
    let ids = batch.batch.calls.iter().map(|call| call.tool_call_id).collect::<Vec<_>>();
    f.execute(batch).await;
    let search: ToolSearchPageV1 = serde_json::from_value(f.output(ids[0])).expect("search page");
    assert!(search.results.iter().any(|result| result.tool_id == tool_id), "{search:?}");
    let description: ToolDescribeResultV1 =
        serde_json::from_value(f.output(ids[1])).expect("description");
    assert_eq!(description.source, ToolSourceKindV1::Imported);
    let loaded: ToolLoadResultV1 = serde_json::from_value(f.output(ids[2])).expect("load");
    assert!(loaded.changed);
    // The load joins the session's tools from the next turn; this turn keeps its list and
    // reaches the package tool through tools.call.
    assert_eq!(loaded.resulting_registry_snapshot_ref, frozen.snapshot_ref);
    assert!(frozen.entry_bindings.iter().any(|binding| binding.tool_id == tool_id));
    let store = f.service().broker.lock().expect("journal");
    let membership = aura_persistence_v2::tool::session_tools::load(
        store.database.connection(),
        f.turn.session_id,
    )
    .expect("stored")
    .expect("the session's dynamic tools");
    assert!(membership.tool_ids.contains(&tool_id));
}
