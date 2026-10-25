use super::*;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChildRegistryDerivationMaterialV1<'a> {
    kind: &'static str,
    policy_version: u32,
    scope: &'a ToolRegistryScopeV1,
    parent_registry_snapshot_ref: &'a aura_tool_contract::ToolRegistrySnapshotRef,
    workspace_mode_hash: &'a CanonicalHashV1,
    user_subagent_policy_revision: Revision,
    user_subagent_policy_hash: &'a CanonicalHashV1,
    parent_permission_snapshot_id: PermissionSnapshotId,
    parent_permission_snapshot_hash: &'a CanonicalHashV1,
    system_hard_allowed_tool_ids: &'a BTreeSet<ToolId>,
    user_subagent_allowed_tool_ids: &'a BTreeSet<ToolId>,
    parent_permission_allowed_tool_ids: &'a BTreeSet<ToolId>,
    workspace_mode_allowed_tool_ids: &'a BTreeSet<ToolId>,
    parent_delegated_tool_ids: &'a BTreeSet<ToolId>,
    runtime_suitable_tool_ids: &'a BTreeSet<ToolId>,
    capacity_allowed_tool_ids: &'a BTreeSet<ToolId>,
    required_core_tools: Vec<ChildRegistryRequiredCoreMaterialV1<'a>>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChildRegistryRequiredCoreMaterialV1<'a> {
    tool_id: ToolId,
    tool_version: &'a ToolVersion,
    tool_instance_id: aura_domain_common::ToolInstanceId,
    definition_fingerprint: &'a CanonicalHashV1,
    source_contract_fingerprint: &'a ToolSourceContractFingerprintV1,
}

/// Derives a separately identified child `trs_` from one immutable parent
/// Snapshot. Ordinary Tools can only be removed; the only additional Tools
/// accepted here are signed Built-ins whose audience is
/// `child_runtime_required`.
pub fn derive_child_registry_snapshot_v1(
    mut input: ChildRegistryDerivationInputV1,
) -> Result<ChildRegistryDerivationOutputV1, ChildRegistryDerivationErrorV1> {
    validate_child_scope(&input.scope, &input.parent_snapshot.scope)?;
    input
        .parent_snapshot
        .validate()
        .map_err(|_| ChildRegistryDerivationErrorV1::ParentSnapshotInvalid)?;
    let parent_tools = validate_child_parent_tools(
        &input.parent_snapshot,
        std::mem::take(&mut input.parent_tools),
    )?;
    let required_core = validate_child_required_core_candidates(
        &input.parent_snapshot,
        std::mem::take(&mut input.child_runtime_required_candidates),
    )?;

    let required_core_material = required_core
        .values()
        .map(|candidate| ChildRegistryRequiredCoreMaterialV1 {
            tool_id: candidate.definition.tool_id,
            tool_version: &candidate.definition.tool_version,
            tool_instance_id: candidate.instance.tool_instance_id,
            definition_fingerprint: &candidate.definition.definition_fingerprint,
            source_contract_fingerprint: &candidate.instance.source_contract_fingerprint,
        })
        .collect::<Vec<_>>();
    let derivation_hash = canonical_sha256(&ChildRegistryDerivationMaterialV1 {
        kind: "aura.p07.child_registry_derivation",
        policy_version: CHILD_REGISTRY_POLICY_VERSION_V1,
        scope: &input.scope,
        parent_registry_snapshot_ref: &input.parent_snapshot.snapshot_ref,
        workspace_mode_hash: &input.workspace_mode_hash,
        user_subagent_policy_revision: input.user_subagent_policy_revision,
        user_subagent_policy_hash: &input.user_subagent_policy_hash,
        parent_permission_snapshot_id: input.parent_permission_snapshot_id,
        parent_permission_snapshot_hash: &input.parent_permission_snapshot_hash,
        system_hard_allowed_tool_ids: &input.system_hard_allowed_tool_ids,
        user_subagent_allowed_tool_ids: &input.user_subagent_allowed_tool_ids,
        parent_permission_allowed_tool_ids: &input.parent_permission_allowed_tool_ids,
        workspace_mode_allowed_tool_ids: &input.workspace_mode_allowed_tool_ids,
        parent_delegated_tool_ids: &input.parent_delegated_tool_ids,
        runtime_suitable_tool_ids: &input.runtime_suitable_tool_ids,
        capacity_allowed_tool_ids: &input.capacity_allowed_tool_ids,
        required_core_tools: required_core_material,
    })
    .map_err(|_| ChildRegistryDerivationErrorV1::Canonicalization)?;
    let eligibility_context_hash = canonical_sha256(&(
        "aura.p07.child_registry_eligibility",
        CHILD_REGISTRY_POLICY_VERSION_V1,
        &derivation_hash,
    ))
    .map_err(|_| ChildRegistryDerivationErrorV1::Canonicalization)?;

    let mut catalog_entries = Vec::new();
    let mut effective_bindings = Vec::new();
    let mut schemas = Vec::new();
    let mut search_documents = Vec::new();
    let mut exclusions = Vec::new();

    for entry in &input.parent_snapshot.catalog_entries {
        let parent =
            parent_tools.get(&entry.tool_id).expect("validated parent definition coverage");
        let required =
            parent.definition.audience == aura_tool_contract::ToolAudienceV1::ChildRuntimeRequired;
        let reason = if is_child_hard_denied(parent.definition.canonical_name.as_str()) {
            Some(ChildRegistryExclusionReasonV1::SystemHardDenied)
        } else if parent.definition.audience == aura_tool_contract::ToolAudienceV1::ParentAgentOnly
        {
            Some(ChildRegistryExclusionReasonV1::ParentAgentOnly)
        } else if !required && !input.system_hard_allowed_tool_ids.contains(&entry.tool_id) {
            Some(ChildRegistryExclusionReasonV1::SystemHardDenied)
        } else if !required && !input.user_subagent_allowed_tool_ids.contains(&entry.tool_id) {
            Some(ChildRegistryExclusionReasonV1::UserSubagentPolicy)
        } else if !required && !input.parent_permission_allowed_tool_ids.contains(&entry.tool_id) {
            Some(ChildRegistryExclusionReasonV1::ParentPermission)
        } else if !required && !input.workspace_mode_allowed_tool_ids.contains(&entry.tool_id) {
            Some(ChildRegistryExclusionReasonV1::WorkspaceMode)
        } else if !required && !input.parent_delegated_tool_ids.contains(&entry.tool_id) {
            Some(ChildRegistryExclusionReasonV1::ParentDelegation)
        } else if !required && !input.runtime_suitable_tool_ids.contains(&entry.tool_id) {
            Some(ChildRegistryExclusionReasonV1::RuntimeUnsuitable)
        } else if !required && !input.capacity_allowed_tool_ids.contains(&entry.tool_id) {
            Some(ChildRegistryExclusionReasonV1::CapacityOrTokenBudget)
        } else {
            None
        };
        if let Some(reason) = reason {
            exclusions.push(ChildRegistryExclusionV1 {
                tool_id: entry.tool_id,
                tool_version: entry.tool_version.clone(),
                reason,
            });
            continue;
        }
        append_child_parent_tool(
            &input.parent_snapshot,
            entry,
            parent,
            &mut catalog_entries,
            &mut effective_bindings,
            &mut schemas,
            &mut search_documents,
        )?;
    }

    for candidate in required_core.values() {
        catalog_entries.push(catalog_entry(candidate, ToolLoadStateV1::Core));
        effective_bindings.push(effective_binding(candidate, ToolExposureV1::Core));
        schemas.push(schema_snapshot(candidate)?);
        search_documents.push(search_document(candidate));
    }

    catalog_entries.sort_by_key(|entry| entry.tool_id);
    effective_bindings.sort_by_key(|binding| binding.tool_id);
    schemas.sort_by_key(|schema| schema.tool_id);
    search_documents.sort_by_key(|document| document.tool_id);
    exclusions.sort_by(|left, right| {
        left.tool_id.cmp(&right.tool_id).then_with(|| left.reason.cmp(&right.reason))
    });

    let snapshot = ToolRegistrySnapshotV1::new(
        ToolRegistrySnapshotId::new(),
        input.registry_revision,
        input.scope,
        Some(input.parent_snapshot.snapshot_ref),
        Some(derivation_hash),
        eligibility_context_hash,
        schemas,
        catalog_entries,
        effective_bindings,
        input.created_at,
    )
    .map_err(|_| ChildRegistryDerivationErrorV1::SnapshotInvalid)?;
    ToolSearchCorpusV1::new(snapshot.clone(), search_documents.clone())
        .map_err(|_| ChildRegistryDerivationErrorV1::SnapshotInvalid)?;
    Ok(ChildRegistryDerivationOutputV1 { snapshot, search_documents, exclusions })
}

pub(super) fn validate_child_scope(
    child: &ToolRegistryScopeV1,
    parent: &ToolRegistryScopeV1,
) -> Result<(), ChildRegistryDerivationErrorV1> {
    let (
        ToolRegistryScopeV1::ChildAgent {
            workspace_id: child_workspace,
            session_id: child_session,
            turn_task_id: child_turn,
            child_agent_run_id,
        },
        ToolRegistryScopeV1::Session {
            workspace_id: parent_workspace,
            session_id: parent_session,
            turn_task_id: parent_turn,
            agent_run_id,
        },
    ) = (child, parent)
    else {
        return Err(ChildRegistryDerivationErrorV1::InvalidScope);
    };
    if child_workspace != parent_workspace
        || child_session != parent_session
        || child_turn != parent_turn
        || child_agent_run_id == agent_run_id
    {
        return Err(ChildRegistryDerivationErrorV1::InvalidScope);
    }
    Ok(())
}

pub(super) fn validate_child_parent_tools(
    parent: &ToolRegistrySnapshotV1,
    tools: Vec<ChildRegistryParentToolV1>,
) -> Result<BTreeMap<ToolId, ChildRegistryParentToolV1>, ChildRegistryDerivationErrorV1> {
    let mut indexed = BTreeMap::new();
    for tool in tools {
        tool.definition
            .validate()
            .map_err(|_| ChildRegistryDerivationErrorV1::ParentDefinitionMismatch)?;
        tool.instance
            .overrides
            .validate()
            .map_err(|_| ChildRegistryDerivationErrorV1::ParentDefinitionMismatch)?;
        if tool.definition.tool_id != tool.instance.tool_id
            || tool.definition.tool_version != tool.instance.tool_version
            || indexed.insert(tool.definition.tool_id, tool).is_some()
        {
            return Err(ChildRegistryDerivationErrorV1::ParentDefinitionMismatch);
        }
    }
    if indexed.len() != parent.catalog_entries.len() {
        return Err(ChildRegistryDerivationErrorV1::ParentDefinitionMismatch);
    }
    for entry in &parent.catalog_entries {
        let Some(tool) = indexed.get(&entry.tool_id) else {
            return Err(ChildRegistryDerivationErrorV1::ParentDefinitionMismatch);
        };
        if tool.definition.tool_version != entry.tool_version
            || tool.instance.tool_instance_id != entry.tool_instance_id
            || tool.definition.canonical_name != entry.canonical_name
            || tool.definition.display_name != entry.display_name
            || tool.definition.model_summary != entry.summary
            || tool.definition.source_kind != entry.source
            || tool.definition.definition_fingerprint != entry.definition_fingerprint
            || tool.instance.source_contract_fingerprint != entry.source_contract_fingerprint
        {
            return Err(ChildRegistryDerivationErrorV1::ParentDefinitionMismatch);
        }
    }
    Ok(indexed)
}

pub(super) fn validate_child_required_core_candidates(
    parent: &ToolRegistrySnapshotV1,
    mut candidates: Vec<RegistryCandidateV1>,
) -> Result<BTreeMap<ToolId, RegistryCandidateV1>, ChildRegistryDerivationErrorV1> {
    validate_candidates(&mut candidates)
        .map_err(|_| ChildRegistryDerivationErrorV1::InvalidRequiredCoreTool)?;
    let mut indexed = BTreeMap::new();
    for candidate in candidates {
        if candidate.definition.source_kind != ToolSourceKindV1::BuiltIn
            || candidate.definition.audience
                != aura_tool_contract::ToolAudienceV1::ChildRuntimeRequired
            || !candidate.active_for_unpinned
            || ineligibility_reason(&candidate).is_some()
        {
            return Err(ChildRegistryDerivationErrorV1::InvalidRequiredCoreTool);
        }
        if parent.catalog_entries.iter().any(|entry| entry.tool_id == candidate.definition.tool_id)
            || indexed.insert(candidate.definition.tool_id, candidate).is_some()
        {
            return Err(ChildRegistryDerivationErrorV1::RequiredCoreToolConflict);
        }
    }
    Ok(indexed)
}

pub(super) fn append_child_parent_tool(
    parent_snapshot: &ToolRegistrySnapshotV1,
    entry: &ToolRegistryCatalogEntryV1,
    parent: &ChildRegistryParentToolV1,
    catalog_entries: &mut Vec<ToolRegistryCatalogEntryV1>,
    effective_bindings: &mut Vec<EffectiveToolBindingSnapshot>,
    schemas: &mut Vec<ToolSchemaSnapshotV1>,
    search_documents: &mut Vec<ToolSearchDocumentV1>,
) -> Result<(), ChildRegistryDerivationErrorV1> {
    catalog_entries.push(entry.clone());
    search_documents.push(ToolSearchDocumentV1 {
        tool_id: entry.tool_id,
        tool_version: entry.tool_version.clone(),
        definition_fingerprint: entry.definition_fingerprint.clone(),
        model_description: parent.definition.model_description.clone(),
    });
    if entry.load_state != ToolLoadStateV1::Discoverable {
        let binding = parent_snapshot
            .effective_bindings
            .iter()
            .find(|binding| binding.tool_id == entry.tool_id)
            .ok_or(ChildRegistryDerivationErrorV1::ParentDefinitionMismatch)?;
        let schema = parent_snapshot
            .schema_set
            .tools
            .iter()
            .find(|schema| schema.tool_id == entry.tool_id)
            .ok_or(ChildRegistryDerivationErrorV1::ParentDefinitionMismatch)?;
        effective_bindings.push(binding.clone());
        schemas.push(schema.clone());
    }
    Ok(())
}

pub(super) fn is_child_hard_denied(canonical_name: &str) -> bool {
    canonical_name.starts_with("task_lineage.")
        || canonical_name == "normal.complete"
        || canonical_name == "mode.disposition"
        || canonical_name.starts_with("plugin.chrome-control.")
        || canonical_name.starts_with("browser.")
}
