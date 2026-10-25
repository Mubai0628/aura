//! Exact package membership comes from the validated publish plan, never from
//! equality between independent package and Tool semantic versions.

use super::*;
use crate::mutation_journal::load_mutation_journal;
use aura_tool_domain::CanonicalToolPackageToolV1;

pub(crate) struct PublishedMembership {
    plan: ToolPackagePublishPlanV1,
}

impl PublishedMembership {
    pub(crate) fn contains(&self, definition: &ToolDefinitionRecordV1) -> bool {
        self.plan.definitions.iter().any(|published| {
            published.tool_id == definition.tool_id
                && published.tool_version == definition.tool_version
                && published.definition_fingerprint == definition.definition_fingerprint
        })
    }

    fn manifest_tool(
        &self,
        definition: &ToolDefinitionRecordV1,
    ) -> Option<CanonicalToolPackageToolV1> {
        self.contains(definition)
            .then(|| {
                self.plan
                    .canonical_manifest
                    .tools
                    .iter()
                    .find(|tool| Some(&tool.tool_key) == definition.tool_key.as_ref())
                    .cloned()
            })
            .flatten()
    }
}

pub(crate) fn published_membership(
    connection: &rusqlite::Connection,
    package_id: ToolPackageId,
    version: &CanonicalSemVer,
) -> Result<Option<PublishedMembership>, PersistenceError> {
    let evidence = load_optional_record::<ToolPackageVersionEvidenceV1, _>(
        connection,
        "SELECT evidence_json, evidence_hash_json FROM tool_package_version_evidence
         WHERE tool_package_id=?1 AND package_version_json=?2",
        params![package_id.to_string(), encode_json(version)?],
    )?;
    let Some(evidence) = evidence else {
        let has_payload = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_package_payloads
             WHERE tool_package_id=?1 AND package_version_json=?2)",
            params![package_id.to_string(), encode_json(version)?],
            |row| row.get::<_, bool>(0),
        )?;
        return if has_payload { Err(PersistenceError::ToolEntityIntegrity) } else { Ok(None) };
    };
    let journal = load_mutation_journal(connection, evidence.mutation_journal_id)?
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if journal.journal_kind != TOOL_PACKAGE_PUBLISH_JOURNAL_KIND_V1 {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let payload: ToolPackagePublishJournalPayloadV1 = serde_json::from_value(journal.payload)?;
    payload.validate()?;
    let plan = payload.plan;
    if plan.package.tool_package_id != package_id
        || &plan.package_version.package_version != version
        || plan.draft != evidence.draft
        || plan.canonical_manifest != evidence.canonical_manifest
        || plan.validation.evidence_hash != evidence.validation_evidence_hash
        || plan.approval.evidence_hash != evidence.approval_evidence_hash
        || plan.runtime_closure_hash != evidence.runtime_closure_hash
        || plan.tool_contract_fingerprints != evidence.tool_contract_fingerprints
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(Some(PublishedMembership { plan }))
}

pub(crate) fn active_definitions(
    connection: &rusqlite::Connection,
    package: &ToolPackageRecordV1,
    definitions: Vec<ToolDefinitionRecordV1>,
) -> Result<Vec<ToolDefinitionRecordV1>, PersistenceError> {
    let Some(version) = package.active_version.as_ref() else { return Ok(Vec::new()) };
    if let Some(membership) = published_membership(connection, package.tool_package_id, version)? {
        let selected = definitions
            .into_iter()
            .filter(|definition| membership.contains(definition))
            .collect::<Vec<_>>();
        if selected.len() != membership.plan.definitions.len() {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        return Ok(selected);
    }
    // Metadata-only records have no executable payload. Keep their inspectable
    // definitions; their readiness remains false. Published rows never use this path.
    let selected = latest_definition_by_tool_key(&definitions, package.tool_package_id);
    Ok(selected.into_values().cloned().collect())
}

pub(crate) fn manifest_tool(
    connection: &rusqlite::Connection,
    definition: &ToolDefinitionRecordV1,
) -> Result<Option<CanonicalToolPackageToolV1>, PersistenceError> {
    let Some(package_id) = definition.tool_package_id else { return Ok(None) };
    let versions = load_bound_records::<ToolPackageVersionRecordV1, _>(
        connection,
        "SELECT record_json, record_hash_json FROM tool_package_versions WHERE tool_package_id=?1",
        [package_id.to_string()],
    )?;
    let mut published = false;
    for version in versions {
        if let Some(membership) =
            published_membership(connection, package_id, &version.package_version)?
        {
            published = true;
            if let Some(tool) = membership.manifest_tool(definition) {
                return Ok(Some(tool));
            }
        }
    }
    if published { Err(PersistenceError::ToolEntityIntegrity) } else { Ok(None) }
}
