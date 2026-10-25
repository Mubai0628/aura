use aura_tool_registry::agent_release::release;

#[test]
fn eleven_memory_tools_have_fixed_builtin_identities_and_precise_capabilities() {
    let release = release().expect("existing release is valid");
    for (name, capability, effects) in [
        ("memory.search", "memory_read", false),
        ("memory.read", "memory_read", false),
        ("memory.list_recent", "memory_read", false),
        ("memory.create", "memory_write", true),
        ("memory.update", "memory_write", true),
        ("memory.link", "memory_write", true),
        ("memory.forget", "memory_write", true),
        ("memory.archive", "memory_write", true),
        ("memory.delete", "memory_delete", true),
        ("memory.validate", "memory_read", false),
        ("memory.repair", "memory_read", true),
    ] {
        let entry = release.entry(name).unwrap_or_else(|| panic!("missing L05 builtin {name}"));
        assert_eq!(
            serde_json::to_value(&entry.definition.capabilities).expect("capabilities"),
            serde_json::json!([capability])
        );
        assert_eq!(entry.definition.execution_semantics.may_create_persistent_effects, effects);
        assert!(!entry.definition.execution_semantics.automatic_retry_allowed);
        assert_eq!(entry.definition.tool_id, entry.instance.tool_id);
    }
}
