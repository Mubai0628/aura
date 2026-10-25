use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SelectedBy {
    Core,
    SessionPin,
    WorkspacePin,
    Ephemeral,
    Active,
}

pub fn build_parent_registry_snapshot(
    input: ParentRegistryBuildInputV1,
) -> Result<ParentRegistryBuildOutputV1, RegistryBuildError> {
    build_parent(input, false)
}

/// Retain existing session membership when the user lowers the dynamic limit.
/// Additions are checked by the mutation boundary against the actual limit.
pub fn build_retained_parent_registry_snapshot(
    input: ParentRegistryBuildInputV1,
) -> Result<ParentRegistryBuildOutputV1, RegistryBuildError> {
    build_parent(input, true)
}

pub(super) fn build_parent(
    mut input: ParentRegistryBuildInputV1,
    retain_existing: bool,
) -> Result<ParentRegistryBuildOutputV1, RegistryBuildError> {
    validate_parent_scope(&input)?;
    input.settings.validate().map_err(|_| RegistryBuildError::SnapshotInvalid)?;
    canonicalize_selections(&mut input)?;
    validate_candidates(&mut input.candidates)?;

    let eligibility_context_hash = compute_eligibility_context_hash(&input)?;
    let workspace_pins = pins_by_tool(&input.workspace_pins)?;
    let session_pins = pins_by_tool(&input.session_pins)?;
    let ephemeral_loads = ephemeral_by_tool(&input.ephemeral_loads)?;
    let candidates = candidates_by_tool(&input.candidates);

    let mut tool_ids = BTreeSet::new();
    tool_ids.extend(candidates.keys().copied());
    tool_ids.extend(workspace_pins.keys().copied());
    tool_ids.extend(session_pins.keys().copied());
    tool_ids.extend(ephemeral_loads.keys().copied());

    let mut catalog_entries = Vec::new();
    let mut effective_bindings = Vec::new();
    let mut entry_bindings = Vec::new();
    let mut schemas = Vec::new();
    let mut search_documents = Vec::new();
    let mut exclusions = Vec::new();

    for tool_id in tool_ids {
        let tool_candidates = candidates.get(&tool_id).map(Vec::as_slice).unwrap_or(&[]);
        let session_pin = session_pins.get(&tool_id).copied();
        let workspace_pin = workspace_pins.get(&tool_id).copied();
        let ephemeral = ephemeral_loads.get(&tool_id).copied();

        let (requested_version, selected_by) = if let Some(pin) = session_pin {
            if let Some(ephemeral) = ephemeral
                && ephemeral.tool_version != pin.tool_version
            {
                exclusions.push(RegistryExclusionV1 {
                    tool_id,
                    requested_version: Some(ephemeral.tool_version.clone()),
                    reason: RegistryExclusionReasonV1::EphemeralShadowedByPin,
                });
            }
            if !resolved_pin_or_exclude(pin, &mut exclusions) {
                continue;
            }
            (Some(&pin.tool_version), SelectedBy::SessionPin)
        } else if let Some(pin) = workspace_pin {
            if let Some(ephemeral) = ephemeral
                && ephemeral.tool_version != pin.tool_version
            {
                exclusions.push(RegistryExclusionV1 {
                    tool_id,
                    requested_version: Some(ephemeral.tool_version.clone()),
                    reason: RegistryExclusionReasonV1::EphemeralShadowedByPin,
                });
            }
            if !resolved_pin_or_exclude(pin, &mut exclusions) {
                continue;
            }
            (Some(&pin.tool_version), SelectedBy::WorkspacePin)
        } else if let Some(load) = ephemeral {
            (Some(&load.tool_version), SelectedBy::Ephemeral)
        } else {
            (None, SelectedBy::Active)
        };

        let selected = if let Some(version) = requested_version {
            tool_candidates
                .iter()
                .copied()
                .find(|candidate| candidate.definition.tool_version == *version)
        } else {
            let active = tool_candidates
                .iter()
                .copied()
                .filter(|candidate| candidate.active_for_unpinned)
                .collect::<Vec<_>>();
            if active.len() > 1 {
                return Err(RegistryBuildError::DuplicateActiveVersion);
            }
            active.into_iter().next()
        };
        let Some(candidate) = selected else {
            if requested_version.is_some() {
                exclusions.push(RegistryExclusionV1 {
                    tool_id,
                    requested_version: requested_version.cloned(),
                    reason: RegistryExclusionReasonV1::ExactVersionUnavailable,
                });
            }
            continue;
        };

        if candidate.definition.source_kind == ToolSourceKindV1::BuiltIn {
            // The closed Agent release contains optional discoverable Built-ins
            // as well as the mandatory Core set. Old five-tool releases retain
            // their original explicit exposure rule.
            if !input.core_exposed_tool_ids.contains(&tool_id)
                && !matches!(
                    candidate.source_evidence,
                    ToolSourceEligibilityEvidenceV1::VerifiedAgentBuiltinRelease { .. }
                )
            {
                exclusions.push(exclusion(candidate, RegistryExclusionReasonV1::AudienceDenied));
                continue;
            }
        } else if is_reserved_namespace(candidate.definition.canonical_name.as_str()) {
            exclusions.push(exclusion(candidate, RegistryExclusionReasonV1::ReservedNamespace));
            continue;
        }

        if let Some(reason) = ineligibility_reason(candidate) {
            exclusions.push(exclusion(candidate, reason));
            continue;
        }

        let selected_by = if candidate.definition.source_kind == ToolSourceKindV1::BuiltIn
            && input.core_exposed_tool_ids.contains(&tool_id)
        {
            SelectedBy::Core
        } else {
            selected_by
        };
        let load_state = match selected_by {
            SelectedBy::Core => ToolLoadStateV1::Core,
            SelectedBy::SessionPin | SelectedBy::WorkspacePin => ToolLoadStateV1::Pinned,
            SelectedBy::Ephemeral => ToolLoadStateV1::Ephemeral,
            SelectedBy::Active => ToolLoadStateV1::Discoverable,
        };
        catalog_entries.push(ToolRegistryCatalogEntryV1 {
            tool_id,
            tool_version: candidate.definition.tool_version.clone(),
            tool_instance_id: candidate.instance.tool_instance_id,
            canonical_name: candidate.definition.canonical_name.clone(),
            display_name: candidate.definition.display_name.clone(),
            summary: candidate.definition.model_summary.clone(),
            source: candidate.definition.source_kind,
            definition_fingerprint: candidate.definition.definition_fingerprint.clone(),
            source_contract_fingerprint: candidate.instance.source_contract_fingerprint.clone(),
            load_state,
        });
        search_documents.push(ToolSearchDocumentV1 {
            tool_id,
            tool_version: candidate.definition.tool_version.clone(),
            definition_fingerprint: candidate.definition.definition_fingerprint.clone(),
            model_description: candidate.definition.model_description.clone(),
        });
        // A discoverable tool is callable through `tools.call` without its definition
        // being sent, unless it is a registry or turn-protocol tool.
        if load_state == ToolLoadStateV1::Discoverable
            && aura_tool_contract::entry_callable(candidate.definition.canonical_name.as_str())
        {
            entry_bindings.push(EffectiveToolBindingSnapshot {
                tool_id,
                tool_version: candidate.definition.tool_version.clone(),
                tool_instance_id: candidate.instance.tool_instance_id,
                source_kind: candidate.definition.source_kind,
                exposure: ToolExposureV1::UnifiedEntry,
                definition_fingerprint: candidate.definition.definition_fingerprint.clone(),
                source_contract_fingerprint: candidate.instance.source_contract_fingerprint.clone(),
            });
        }
        if load_state != ToolLoadStateV1::Discoverable {
            let exposure = match selected_by {
                SelectedBy::Core => ToolExposureV1::Core,
                SelectedBy::SessionPin => ToolExposureV1::SessionPin,
                SelectedBy::WorkspacePin => ToolExposureV1::WorkspacePin,
                SelectedBy::Ephemeral => ToolExposureV1::EphemeralLoad,
                SelectedBy::Active => unreachable!("discoverable tools are not exposed"),
            };
            effective_bindings.push(EffectiveToolBindingSnapshot {
                tool_id,
                tool_version: candidate.definition.tool_version.clone(),
                tool_instance_id: candidate.instance.tool_instance_id,
                source_kind: candidate.definition.source_kind,
                exposure,
                definition_fingerprint: candidate.definition.definition_fingerprint.clone(),
                source_contract_fingerprint: candidate.instance.source_contract_fingerprint.clone(),
            });
            schemas.push(
                ToolSchemaSnapshotV1::new(
                    tool_id,
                    candidate.definition.tool_version.clone(),
                    candidate.definition.canonical_name.clone(),
                    candidate.definition.model_description.clone(),
                    candidate.definition.input_schema.clone(),
                    candidate.definition.strict,
                )
                .map_err(|_| RegistryBuildError::SnapshotInvalid)?,
            );
        }
    }

    let dynamic_count = catalog_entries
        .iter()
        .filter(|entry| {
            entry.load_state != ToolLoadStateV1::Core
                && entry.load_state != ToolLoadStateV1::Discoverable
        })
        .count();
    if !retain_existing && dynamic_count > input.settings.max_dynamic_tool_schemas as usize {
        return Err(RegistryBuildError::CapacityRemediationRequired);
    }

    let snapshot = ToolRegistrySnapshotV1::with_entry_bindings(
        ToolRegistrySnapshotId::new(),
        input.registry_revision,
        input.scope,
        None,
        None,
        eligibility_context_hash,
        schemas,
        catalog_entries,
        effective_bindings,
        entry_bindings,
        input.created_at,
    )
    .map_err(|_| RegistryBuildError::SnapshotInvalid)?;
    exclusions.sort_by(|left, right| {
        left.tool_id
            .cmp(&right.tool_id)
            .then_with(|| format!("{:?}", left.reason).cmp(&format!("{:?}", right.reason)))
    });
    Ok(ParentRegistryBuildOutputV1 { snapshot, search_documents, exclusions })
}

pub(super) fn validate_parent_scope(
    input: &ParentRegistryBuildInputV1,
) -> Result<(), RegistryBuildError> {
    match &input.scope {
        ToolRegistryScopeV1::Workspace { .. } => {
            if !input.session_pins.is_empty() || !input.ephemeral_loads.is_empty() {
                return Err(RegistryBuildError::InvalidScope);
            }
        }
        ToolRegistryScopeV1::Session { session_id, turn_task_id, .. } => {
            if input
                .ephemeral_loads
                .iter()
                .any(|load| load.session_id != *session_id || load.turn_task_id != *turn_task_id)
            {
                return Err(RegistryBuildError::InvalidScope);
            }
        }
        ToolRegistryScopeV1::ChildAgent { .. } => return Err(RegistryBuildError::InvalidScope),
    }
    Ok(())
}

pub(super) fn canonicalize_selections(
    input: &mut ParentRegistryBuildInputV1,
) -> Result<(), RegistryBuildError> {
    input.workspace_pins.sort_by_key(|pin| pin.tool_id);
    input.session_pins.sort_by_key(|pin| pin.tool_id);
    input.ephemeral_loads.sort_by_key(|load| load.tool_id);
    let unique = |ids: Vec<ToolId>| ids.windows(2).all(|pair| pair[0] < pair[1]);
    if !unique(input.workspace_pins.iter().map(|pin| pin.tool_id).collect())
        || !unique(input.session_pins.iter().map(|pin| pin.tool_id).collect())
        || !unique(input.ephemeral_loads.iter().map(|load| load.tool_id).collect())
    {
        return Err(RegistryBuildError::DuplicateSelection);
    }
    Ok(())
}

pub(super) fn resolved_pin_or_exclude(
    pin: &ToolPinV1,
    exclusions: &mut Vec<RegistryExclusionV1>,
) -> bool {
    let reason = match pin.state {
        ToolPinResolutionStateV1::Resolved => return true,
        ToolPinResolutionStateV1::Unresolved => RegistryExclusionReasonV1::PinUnresolved,
        ToolPinResolutionStateV1::Degraded => RegistryExclusionReasonV1::PinDegraded,
    };
    exclusions.push(RegistryExclusionV1 {
        tool_id: pin.tool_id,
        requested_version: Some(pin.tool_version.clone()),
        reason,
    });
    false
}

pub(super) fn exclusion(
    candidate: &RegistryCandidateV1,
    reason: RegistryExclusionReasonV1,
) -> RegistryExclusionV1 {
    RegistryExclusionV1 {
        tool_id: candidate.definition.tool_id,
        requested_version: Some(candidate.definition.tool_version.clone()),
        reason,
    }
}

pub(super) fn pins_by_tool(
    pins: &[ToolPinV1],
) -> Result<BTreeMap<ToolId, &ToolPinV1>, RegistryBuildError> {
    let map = pins.iter().map(|pin| (pin.tool_id, pin)).collect::<BTreeMap<_, _>>();
    if map.len() != pins.len() {
        return Err(RegistryBuildError::DuplicateSelection);
    }
    Ok(map)
}

pub(super) fn ephemeral_by_tool(
    loads: &[ToolEphemeralLoadRecordV1],
) -> Result<BTreeMap<ToolId, &ToolEphemeralLoadRecordV1>, RegistryBuildError> {
    let map = loads.iter().map(|load| (load.tool_id, load)).collect::<BTreeMap<_, _>>();
    if map.len() != loads.len() {
        return Err(RegistryBuildError::DuplicateSelection);
    }
    Ok(map)
}

pub(super) fn candidates_by_tool(
    candidates: &[RegistryCandidateV1],
) -> BTreeMap<ToolId, Vec<&RegistryCandidateV1>> {
    let mut by_tool = BTreeMap::<_, Vec<_>>::new();
    for candidate in candidates {
        by_tool.entry(candidate.definition.tool_id).or_default().push(candidate);
    }
    by_tool
}
