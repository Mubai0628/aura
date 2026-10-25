use super::*;
#[test]
fn terminal_release_facts_match_the_domain_schemas() {
    let at = "2026-09-09T00:00:00Z".parse().expect("release timestamp");
    let entries = definitions::entries(at).expect("domain-owned definitions");
    let terminals = entries
        .iter()
        .filter(|entry| entry.handler.name().starts_with("terminal."))
        .collect::<Vec<_>>();
    assert_eq!(terminals.len(), 5);
    for entry in &terminals {
        println!(
            "{} {} {} {}",
            entry.handler.name(),
            entry.definition.tool_id,
            entry.instance.tool_instance_id,
            entry.definition.definition_fingerprint.hex()
        );
    }
    for entry in terminals {
        let published = revisions::current(entry.handler.name()).expect("stable identity");
        assert_eq!(published.fingerprint, entry.definition.definition_fingerprint.hex());
    }
    let sensitive = entries
        .iter()
        .find(|v| v.handler == AgentBuiltinHandler::TerminalSensitiveInput)
        .expect("L02 test fact");
    assert_eq!(sensitive.definition.audience, ToolAudienceV1::ParentAgentOnly);
    assert!(sensitive.definition.model_description.contains("never the input"));
}
