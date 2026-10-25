use super::*;

#[test]
fn session_pin_overrides_a_different_workspace_version() {
    let tool_id = ToolId::from_bytes([1; 16]);
    let mut input = base_input(vec![package_candidate(1, 1, false), package_candidate(1, 2, true)]);
    input.workspace_pins.push(pin(tool_id, version(1), ToolPinResolutionStateV1::Resolved));
    input.session_pins.push(pin(tool_id, version(2), ToolPinResolutionStateV1::Resolved));

    let output = build_parent_registry_snapshot(input).expect("snapshot");

    assert_eq!(output.snapshot.catalog_entries.len(), 1);
    assert_eq!(output.snapshot.catalog_entries[0].tool_version, version(2));
    assert_eq!(output.snapshot.catalog_entries[0].load_state, ToolLoadStateV1::Pinned);
    assert_eq!(output.snapshot.effective_bindings[0].exposure, ToolExposureV1::SessionPin);
}

#[test]
fn unresolved_session_pin_never_falls_back_to_workspace_pin() {
    let tool_id = ToolId::from_bytes([2; 16]);
    let mut input = base_input(vec![package_candidate(2, 1, false), package_candidate(2, 2, true)]);
    input.workspace_pins.push(pin(tool_id, version(1), ToolPinResolutionStateV1::Resolved));
    input.session_pins.push(pin(tool_id, version(2), ToolPinResolutionStateV1::Unresolved));

    let output = build_parent_registry_snapshot(input).expect("snapshot");

    assert!(output.snapshot.catalog_entries.is_empty());
    assert_eq!(output.exclusions.len(), 1);
    assert_eq!(output.exclusions[0].reason, RegistryExclusionReasonV1::PinUnresolved);
}

#[test]
fn ephemeral_load_cannot_bypass_an_exact_session_pin() {
    let tool_id = ToolId::from_bytes([3; 16]);
    let mut input = base_input(vec![package_candidate(3, 1, false), package_candidate(3, 2, true)]);
    input.session_pins.push(pin(tool_id, version(2), ToolPinResolutionStateV1::Resolved));
    input.ephemeral_loads.push(ToolEphemeralLoadRecordV1 {
        turn_task_id: TurnTaskId::from_bytes([13; 16]),
        session_id: SessionId::from_bytes([12; 16]),
        tool_id,
        tool_version: version(1),
        entity_revision: Revision::INITIAL,
        loaded_at: timestamp(),
    });

    let output = build_parent_registry_snapshot(input).expect("snapshot");

    assert_eq!(output.snapshot.catalog_entries[0].tool_version, version(2));
    assert!(
        output
            .exclusions
            .iter()
            .any(|item| { item.reason == RegistryExclusionReasonV1::EphemeralShadowedByPin })
    );
}

#[test]
fn core_is_free_but_two_dynamic_exposures_require_capacity_remediation() {
    let core = core_candidate(9);
    let core_id = core.definition.tool_id;
    let first_id = ToolId::from_bytes([4; 16]);
    let second_id = ToolId::from_bytes([5; 16]);
    let mut input =
        base_input(vec![core, package_candidate(4, 1, true), package_candidate(5, 1, true)]);
    input.settings =
        ToolSettingsSnapshotV1::new(Revision::INITIAL, 1, 4, 120_000, 3).expect("settings");
    input.core_exposed_tool_ids.insert(core_id);
    input.workspace_pins.push(pin(first_id, version(1), ToolPinResolutionStateV1::Resolved));
    input.session_pins.push(pin(second_id, version(1), ToolPinResolutionStateV1::Resolved));

    assert_eq!(
        build_parent_registry_snapshot(input),
        Err(RegistryBuildError::CapacityRemediationRequired)
    );
}

#[test]
fn discoverable_tools_are_frozen_but_not_added_to_the_schema_set() {
    let output = build_parent_registry_snapshot(base_input(vec![package_candidate(6, 1, true)]))
        .expect("snapshot");

    assert_eq!(output.snapshot.catalog_entries.len(), 1);
    assert_eq!(output.snapshot.catalog_entries[0].load_state, ToolLoadStateV1::Discoverable);
    assert!(output.snapshot.effective_bindings.is_empty());
    assert!(output.snapshot.schema_set.tools.is_empty());
    output.snapshot.validate().expect("internally consistent snapshot");
}

#[test]
fn duplicate_active_versions_are_an_integrity_error_even_when_a_pin_exists() {
    let tool_id = ToolId::from_bytes([7; 16]);
    let mut input = base_input(vec![package_candidate(7, 1, true), package_candidate(7, 2, true)]);
    input.session_pins.push(pin(tool_id, version(2), ToolPinResolutionStateV1::Resolved));

    assert_eq!(
        build_parent_registry_snapshot(input),
        Err(RegistryBuildError::DuplicateActiveVersion)
    );
}
