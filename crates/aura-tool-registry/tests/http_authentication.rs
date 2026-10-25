#[test]
fn http_request_authentication_guidance_publishes_a_new_definition_and_instance() {
    let release = aura_tool_registry::agent_release::generate_manifest()
        .expect("current builtin definitions");
    let entry = release
        .entries
        .iter()
        .find(|entry| entry.definition.canonical_name.as_str() == "http.request")
        .expect("HTTP request definition");
    // Published the guidance as revision 2; the description was rewritten as revision 3.
    assert!(
        entry.definition.tool_version.definition_revision.value() >= 2,
        "changed model guidance requires a new definition revision"
    );
    assert_ne!(
        entry.instance.tool_instance_id.to_string(),
        "tin_01M2DG020980080021AN94200Z",
        "the old immutable instance remains historical"
    );
    assert!(
        entry
            .definition
            .model_description
            .contains("Authentication material must only come from Tool credentials")
    );
    assert!(entry.definition.model_description.contains("allowed websites"));
    assert!(entry.definition.model_description.contains("before approval"));
}
