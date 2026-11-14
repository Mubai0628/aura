use aura_memory_domain::*;
use serde_json::json;

#[test]
fn tool_contracts_round_trip_and_require_scope_relative_targets_and_expected_hashes() {
    let input = json!({"scope":"project","items":[{"target":{"kind":"path","path":"rules/build.md"},"expected_hash":"a".repeat(64),"body":"# 构建约定\n"}]});
    let parsed: MemoryUpdateInput = serde_json::from_value(input.clone()).expect("typed update");
    let round_trip: MemoryUpdateInput =
        serde_json::from_value(serde_json::to_value(parsed).expect("wire")).expect("round trip");
    assert_eq!(round_trip.items[0].expected_hash, "a".repeat(64));
    for mutation in ["missing_hash", "absolute", "foreign_scope", "authority"] {
        let mut changed = input.clone();
        match mutation {
            "missing_hash" => {
                changed["items"][0].as_object_mut().expect("item").remove("expected_hash");
            }
            "absolute" => changed["items"][0]["target"]["path"] = "C:\\owner\\outside.md".into(),
            "foreign_scope" => changed["scope"] = "another_workspace".into(),
            "authority" => changed["workspace_id"] = "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            _ => unreachable!(),
        }
        assert!(serde_json::from_value::<MemoryUpdateInput>(changed).is_err(), "{mutation}");
    }
    assert!(
        serde_json::from_value::<MemoryDeleteInput>(
            json!({"scope":"global","items":[{"target":{"kind":"path","path":"rules/a.md"}}]})
        )
        .is_err()
    );
}
