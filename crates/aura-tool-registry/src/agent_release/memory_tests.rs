use super::*;

/// The schemas once allowed any byte for `importance` (valid is 1–5), so the
/// model sent 128 and 180, and every such note was refused. The tools that take the field now
/// say its range, and their published revision follows the changed contract.
#[test]
fn memory_writes_state_the_ranges_of_their_numbers() {
    let at = "2026-09-09T00:00:00Z".parse().expect("release timestamp");
    let entries = definitions::entries(at).expect("domain-owned definitions");
    for name in ["memory.create", "memory.update"] {
        let entry =
            entries.iter().find(|entry| entry.handler.name() == name).expect("a memory write tool");
        println!(
            "{} {} {} {} {}",
            name,
            entry.definition.tool_id,
            entry.instance.tool_instance_id,
            entry.definition.tool_version.definition_revision.value(),
            entry.definition.definition_fingerprint.hex()
        );
        let schema = serde_json::to_string(&entry.definition.input_schema).expect("schema");
        for range in [r#""minimum":1"#, r#""maximum":5"#, r#""maximum":1,"#] {
            assert!(schema.contains(range), "{name} states {range}: {schema}");
        }
        let published = revisions::current(name).expect("stable identity");
        assert_eq!(published.fingerprint, entry.definition.definition_fingerprint.hex(), "{name}");
    }
}
