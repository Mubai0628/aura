use super::*;
use aura_security_contract::ResolvedCapability as C;

/// Two Skill tools, a search and a load by name: parent-only Core reads that change nothing and
/// name no file, process or network capability.
#[test]
fn skill_tools_are_a_parent_only_search_and_load() {
    let at = "2026-09-09T00:00:00Z".parse().expect("release timestamp");
    let entries = definitions::entries(at).expect("domain-owned definitions");
    let skills = entries
        .iter()
        .filter(|entry| entry.handler.name().starts_with("skills."))
        .collect::<Vec<_>>();
    assert_eq!(
        skills.iter().map(|entry| entry.handler.name()).collect::<Vec<_>>(),
        ["skills.search", "skills.load"]
    );
    for entry in skills {
        let name = entry.handler.name();
        assert_eq!(entry.definition.capabilities, [C::ToolRegistryRead], "{name}");
        assert_eq!(
            entry.definition.audience,
            aura_tool_contract::ToolAudienceV1::ParentAgentOnly,
            "{name}"
        );
        let semantics = &entry.definition.execution_semantics;
        assert!(
            !semantics.may_create_persistent_effects && semantics.repeat_is_idempotent,
            "{name}"
        );
    }
}
