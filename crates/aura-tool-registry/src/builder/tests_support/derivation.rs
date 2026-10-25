use super::*;

#[test]
fn child_registry_is_an_independent_snapshot_and_only_narrows_parent_tools() {
    let first = package_candidate(31, 1, true);
    let second = package_candidate(32, 1, true);
    let allowed = BTreeSet::from([first.definition.tool_id]);
    let input = child_input(vec![first.clone(), second.clone()], allowed);
    let parent_ref = input.parent_snapshot.snapshot_ref.clone();

    let first_output = derive_child_registry_snapshot_v1(input.clone()).expect("child");
    let second_output = derive_child_registry_snapshot_v1(input).expect("second child");

    assert_eq!(first_output.snapshot.parent_registry_snapshot_ref, Some(parent_ref));
    assert_eq!(first_output.snapshot.catalog_entries.len(), 1);
    assert_eq!(first_output.snapshot.catalog_entries[0].tool_id, first.definition.tool_id);
    assert_eq!(first_output.exclusions.len(), 1);
    assert_eq!(first_output.exclusions[0].reason, ChildRegistryExclusionReasonV1::SystemHardDenied);
    assert_ne!(
        first_output.snapshot.snapshot_ref.tool_registry_snapshot_id,
        second_output.snapshot.snapshot_ref.tool_registry_snapshot_id
    );
    assert_eq!(first_output.snapshot.schema_set.tools, second_output.snapshot.schema_set.tools);
}

#[test]
fn child_registry_filters_parent_only_and_injects_only_required_signed_core() {
    let mut parent_only = package_candidate(41, 1, true);
    parent_only.definition = ToolDefinitionRecordV1::new(
        parent_only.definition.tool_id,
        parent_only.definition.tool_version.clone(),
        parent_only.definition.tool_package_id,
        parent_only.definition.package_key.clone(),
        parent_only.definition.tool_key.clone(),
        parent_only.definition.canonical_name.clone(),
        parent_only.definition.display_name.clone(),
        parent_only.definition.model_description.clone(),
        parent_only.definition.model_summary.clone(),
        parent_only.definition.input_schema.clone(),
        parent_only.definition.strict,
        parent_only.definition.result_contract.clone(),
        parent_only.definition.runtime.clone(),
        parent_only.definition.capabilities.clone(),
        parent_only.definition.resource_claims.clone(),
        ToolAudienceV1::ParentAgentOnly,
        parent_only.definition.source_kind,
        parent_only.definition.execution_semantics,
        parent_only.definition.runtime_file_closure_hash.clone(),
        parent_only.definition.entity_revision,
        parent_only.definition.created_at,
    )
    .expect("parent-only definition");
    let allowed = BTreeSet::from([parent_only.definition.tool_id]);
    let mut input = child_input(vec![parent_only.clone()], allowed);
    let required = child_required_core_candidate(42);
    let required_id = required.definition.tool_id;
    input.child_runtime_required_candidates.push(required);

    let output = derive_child_registry_snapshot_v1(input).expect("child");

    assert_eq!(output.snapshot.catalog_entries.len(), 1);
    assert_eq!(output.snapshot.catalog_entries[0].tool_id, required_id);
    assert_eq!(output.snapshot.catalog_entries[0].load_state, ToolLoadStateV1::Core);
    assert_eq!(output.snapshot.effective_bindings[0].exposure, ToolExposureV1::Core);
    assert_eq!(output.exclusions[0].reason, ChildRegistryExclusionReasonV1::ParentAgentOnly);
}

#[test]
fn child_registry_rejects_missing_parent_definition_evidence() {
    let candidate = package_candidate(51, 1, true);
    let allowed = BTreeSet::from([candidate.definition.tool_id]);
    let mut input = child_input(vec![candidate], allowed);
    input.parent_tools.clear();

    assert!(derive_child_registry_snapshot_v1(input).is_err());
}

#[test]
fn child_hard_deny_names_cover_control_and_browser_sources() {
    for name in
        ["task_lineage.reconcile", "normal.complete", "plugin.chrome-control.click", "browser.open"]
    {
        assert!(is_child_hard_denied(name), "{name}");
    }
    assert!(!is_child_hard_denied("context.search"));
}
