use super::*;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EligibilityContextMaterial<'a> {
    scope: &'a ToolRegistryScopeV1,
    settings: &'a ToolSettingsSnapshotV1,
    mode_permission_context_hash: &'a CanonicalHashV1,
    core_exposed_tool_ids: &'a BTreeSet<ToolId>,
    candidate_evidence: Vec<CandidateEvidenceMaterial<'a>>,
    workspace_pins: &'a [ToolPinV1],
    session_pins: &'a [ToolPinV1],
    ephemeral_loads: Vec<EphemeralEvidenceMaterial<'a>>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CandidateEvidenceMaterial<'a> {
    tool_id: ToolId,
    tool_version: &'a ToolVersion,
    tool_instance_id: aura_domain_common::ToolInstanceId,
    definition_fingerprint: &'a CanonicalHashV1,
    source_contract_fingerprint: &'a ToolSourceContractFingerprintV1,
    source_evidence: &'a ToolSourceEligibilityEvidenceV1,
    enabled: bool,
    runtime_health: ToolRuntimeHealthV1,
    entity_revision: Revision,
    active_for_unpinned: bool,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EphemeralEvidenceMaterial<'a> {
    tool_id: ToolId,
    tool_version: &'a ToolVersion,
    entity_revision: Revision,
}

pub(super) fn compute_eligibility_context_hash(
    input: &ParentRegistryBuildInputV1,
) -> Result<CanonicalHashV1, RegistryBuildError> {
    let candidate_evidence = input
        .candidates
        .iter()
        .map(|candidate| CandidateEvidenceMaterial {
            tool_id: candidate.definition.tool_id,
            tool_version: &candidate.definition.tool_version,
            tool_instance_id: candidate.instance.tool_instance_id,
            definition_fingerprint: &candidate.definition.definition_fingerprint,
            source_contract_fingerprint: &candidate.instance.source_contract_fingerprint,
            source_evidence: &candidate.source_evidence,
            enabled: candidate.instance.enabled,
            runtime_health: candidate.instance.runtime_health,
            entity_revision: candidate.instance.entity_revision,
            active_for_unpinned: candidate.active_for_unpinned,
        })
        .collect();
    let ephemeral_loads = input
        .ephemeral_loads
        .iter()
        .map(|load| EphemeralEvidenceMaterial {
            tool_id: load.tool_id,
            tool_version: &load.tool_version,
            entity_revision: load.entity_revision,
        })
        .collect();
    canonical_sha256(&EligibilityContextMaterial {
        scope: &input.scope,
        settings: &input.settings,
        mode_permission_context_hash: &input.mode_permission_context_hash,
        core_exposed_tool_ids: &input.core_exposed_tool_ids,
        candidate_evidence,
        workspace_pins: &input.workspace_pins,
        session_pins: &input.session_pins,
        ephemeral_loads,
    })
    .map_err(|_| RegistryBuildError::SnapshotInvalid)
}
