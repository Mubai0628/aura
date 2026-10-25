use super::*;
use aura_security_contract::ResolvedCapability as C;

/// The six Skill tools are parent-only Core reads and turn exposure changes; none of
/// them names a file, process or network capability.
#[test]
fn skill_tools_are_parent_only_registry_reads_and_turn_loads() {
    let at = "2026-09-09T00:00:00Z".parse().expect("release timestamp");
    let entries = definitions::entries(at).expect("domain-owned definitions");
    let skills = entries
        .iter()
        .filter(|entry| entry.handler.name().starts_with("skills."))
        .collect::<Vec<_>>();
    assert_eq!(skills.len(), 6);
    for entry in &skills {
        println!(
            "{} {} {} {}",
            entry.handler.name(),
            entry.definition.tool_id,
            entry.instance.tool_instance_id,
            entry.definition.definition_fingerprint.hex()
        );
    }
    for entry in skills {
        let expected = if matches!(entry.handler.name(), "skills.load" | "skills.unload") {
            vec![C::ToolEphemeralExposureWrite]
        } else {
            vec![C::ToolRegistryRead]
        };
        assert_eq!(entry.definition.capabilities, expected, "{}", entry.handler.name());
        assert_eq!(
            entry.definition.audience,
            aura_tool_contract::ToolAudienceV1::ParentAgentOnly,
            "{}",
            entry.handler.name()
        );
    }
}
