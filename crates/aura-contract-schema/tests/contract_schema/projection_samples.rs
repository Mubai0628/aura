use std::collections::BTreeSet;

use aura_contract_schema::{canonical_schema_value, full_projection_samples};
use aura_projection_contract::ProjectionChange;
use serde_json::Value;

#[test]
fn full_projection_samples_cover_every_variant_and_round_trip_through_rust() {
    let schema = canonical_schema_value().expect("Rust contract schema");
    let fixture = full_projection_samples(&schema).expect("full projection samples");
    let projections = fixture["projections"].as_array().expect("projection samples");
    let expected = schema["$defs"]["ProjectionChange"]["oneOf"]
        .as_array()
        .expect("projection variant schema")
        .iter()
        .map(|variant| variant["properties"]["projection_kind"]["const"].clone())
        .collect::<Vec<_>>();
    let actual =
        projections.iter().map(|sample| sample["projection_kind"].clone()).collect::<Vec<_>>();
    let kinds = |values: &[Value]| values.iter().map(Value::to_string).collect::<BTreeSet<_>>();
    assert_eq!(kinds(&expected), kinds(&actual));
    assert!(
        projections.len() > expected.len(),
        "mutually exclusive states need complementary samples"
    );
    for projection in projections {
        let typed: ProjectionChange =
            serde_json::from_value(projection.clone()).expect("typed Rust sample");
        typed.validate().expect("bound projection identity");
        assert_eq!(serde_json::to_value(typed).expect("Rust serialization"), *projection);
    }
    let settings = projections
        .iter()
        .find(|projection| projection["projection_kind"] == "tool_settings")
        .expect("settings sample");
    assert!(settings["payload"]["settings"]["network_proxy"].is_object());
    let prompt = projections
        .iter()
        .find(|projection| projection["projection_kind"] == "prompt_inspector")
        .expect("prompt sample");
    assert!(prompt["payload"]["activity"].is_object());
    assert!(prompt["payload"]["cache_usage"]["actual_input_tokens"].is_number());
}
