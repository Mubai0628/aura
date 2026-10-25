use super::*;

pub(super) fn catalog_entry(
    candidate: &RegistryCandidateV1,
    load_state: ToolLoadStateV1,
) -> ToolRegistryCatalogEntryV1 {
    ToolRegistryCatalogEntryV1 {
        tool_id: candidate.definition.tool_id,
        tool_version: candidate.definition.tool_version.clone(),
        tool_instance_id: candidate.instance.tool_instance_id,
        canonical_name: candidate.definition.canonical_name.clone(),
        display_name: candidate.definition.display_name.clone(),
        summary: candidate.definition.model_summary.clone(),
        source: candidate.definition.source_kind,
        definition_fingerprint: candidate.definition.definition_fingerprint.clone(),
        source_contract_fingerprint: candidate.instance.source_contract_fingerprint.clone(),
        load_state,
    }
}

pub(super) fn effective_binding(
    candidate: &RegistryCandidateV1,
    exposure: ToolExposureV1,
) -> EffectiveToolBindingSnapshot {
    EffectiveToolBindingSnapshot {
        tool_id: candidate.definition.tool_id,
        tool_version: candidate.definition.tool_version.clone(),
        tool_instance_id: candidate.instance.tool_instance_id,
        source_kind: candidate.definition.source_kind,
        exposure,
        definition_fingerprint: candidate.definition.definition_fingerprint.clone(),
        source_contract_fingerprint: candidate.instance.source_contract_fingerprint.clone(),
    }
}

pub(super) fn schema_snapshot(
    candidate: &RegistryCandidateV1,
) -> Result<ToolSchemaSnapshotV1, ChildRegistryDerivationErrorV1> {
    ToolSchemaSnapshotV1::new(
        candidate.definition.tool_id,
        candidate.definition.tool_version.clone(),
        candidate.definition.canonical_name.clone(),
        candidate.definition.model_description.clone(),
        candidate.definition.input_schema.clone(),
        candidate.definition.strict,
    )
    .map_err(|_| ChildRegistryDerivationErrorV1::SnapshotInvalid)
}

pub(super) fn search_document(candidate: &RegistryCandidateV1) -> ToolSearchDocumentV1 {
    ToolSearchDocumentV1 {
        tool_id: candidate.definition.tool_id,
        tool_version: candidate.definition.tool_version.clone(),
        definition_fingerprint: candidate.definition.definition_fingerprint.clone(),
        model_description: candidate.definition.model_description.clone(),
    }
}
