//! Media is discoverable file reading, with the model choosing when and how to decode.
use aura_security_contract::ResolvedCapability;
use aura_tool_registry::agent_release::release;

#[test]
fn media_tools_are_discoverable_read_only_and_never_automatically_replayed() {
    let release = release().expect("current verified release");
    for name in ["media.probe", "media.frames"] {
        let entry = release.entry(name).unwrap_or_else(|| panic!("{name} must be discoverable"));
        assert_eq!(entry.handler.name(), name);
        assert_eq!(entry.definition.capabilities, vec![ResolvedCapability::WorkspaceFileRead]);
        assert!(!entry.definition.execution_semantics.may_create_persistent_effects);
        assert!(!entry.definition.execution_semantics.automatic_retry_allowed);
        assert!(entry.definition.execution_semantics.repeat_is_idempotent);
        assert!(entry.instance.enabled);
        let input = entry.definition.input_schema.document();
        assert_eq!(input["additionalProperties"], false);
        assert_eq!(input["properties"]["path"]["type"], "string");
        assert!(
            input["required"]
                .as_array()
                .expect("required fields")
                .iter()
                .any(|value| value == "path")
        );
        assert!(entry.definition.model_description.contains("external"));
    }
}

#[test]
fn the_frames_contract_offers_times_count_sheet_and_bounded_image_size() {
    let release = release().expect("current verified release");
    let entry = release.entry("media.frames").expect("the model can discover frame extraction");
    let properties = &entry.definition.input_schema.document()["properties"];
    for field in ["times", "count", "sheet", "max_side"] {
        assert!(properties.get(field).is_some(), "frame selection must expose {field}");
    }
    let description = &entry.definition.model_description;
    for requirement in ["12", "1024", "audio", "shown_to_model"] {
        assert!(description.contains(requirement), "describe {requirement} explicitly");
    }
}
