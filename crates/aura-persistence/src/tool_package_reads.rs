//! Read-only access to the retired P07 package tables. Packages moved to the
//! V2 store; nothing writes these tables any more. The shared V1 tool authority
//!
//! still reads them, so these readers stay until that authority is retired.
use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{
    CorrelationId, EventId, MutationJournalId, ToolConformanceRunId, ToolId, ToolPackageId,
    UtcTimestamp,
};
use aura_tool_contract::{
    PackageRelativePath, PackageRuntimeClosureHashV1, ToolContractFingerprintV1, ToolKey,
};
use aura_tool_domain::{
    CanonicalToolPackageManifestV1, ConformanceDraftRevisionRefV1, ToolConformanceStatusV1,
    ToolDefinitionRecordV1, ToolDraftApprovalDecisionV1, ToolDraftApprovalEvidenceV1,
    ToolDraftValidationEvidenceV1, ToolInstanceConfigurationRecordV1,
    ToolInstanceCredentialBindingRecordV1, ToolInstanceRecordV1, ToolPackageLifecycleV1,
    ToolPackageRecordV1, ToolPackageVersionRecordV1,
};
use aura_tool_package::ToolPackageStoreV1;
use aura_tool_projection::{
    ToolDefinitionSummaryView, ToolInstanceConfigurationProjectionV1, ToolPackageListProjection,
    ToolPackageProjection, ToolPackageSummaryView, ToolPackageVersionSummaryView,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::{
    PersistenceCoordinator, PersistenceError,
    tool_builtin_release::{
        credential_slot_binding_summary, credential_slot_binding_summary_for_runtime,
        package_manifest_tool, tool_definition_projection_drafts,
    },
    tool_entities::{encode_json, load_bound_records, load_optional_record},
    tool_projection::ToolProjectionDraft,
};

#[path = "tool_package_membership.rs"]
pub(crate) mod membership;

/// The journal kind historical P07 package publications were recorded under.
pub const TOOL_PACKAGE_PUBLISH_JOURNAL_KIND_V1: &str = "p07.tool_package_publish.v1";
#[cfg(debug_assertions)]
pub(crate) const INSTALLED_VERSION_REFERENCE_KIND: &str = "installed_version";
pub(crate) const PAYLOAD_RETENTION_DAYS: i64 = 30;
const TOOL_PACKAGE_PUBLISH_JOURNAL_SCHEMA_VERSION_V1: u32 = 1;

// The historical publish journal shape, read by package membership.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ToolPackagePublishOperationV1 {
    Install,
    Update,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolPackagePublishPlanV1 {
    schema_version: u32,
    operation: ToolPackagePublishOperationV1,
    draft: ConformanceDraftRevisionRefV1,
    validation: ToolDraftValidationEvidenceV1,
    approval: ToolDraftApprovalEvidenceV1,
    canonical_manifest: CanonicalToolPackageManifestV1,
    runtime_closure_json: serde_json::Value,
    runtime_closure_hash: PackageRuntimeClosureHashV1,
    tool_contract_fingerprints: Vec<PersistedToolContractFingerprintEntryV1>,
    package: ToolPackageRecordV1,
    package_version: ToolPackageVersionRecordV1,
    definitions: Vec<ToolDefinitionRecordV1>,
    instances: Vec<ToolInstanceRecordV1>,
    previous_package: Option<ToolPackageRecordV1>,
    previous_package_version: Option<ToolPackageVersionRecordV1>,
    retired_tool_ids: Vec<ToolId>,
    package_store_relative_path: PackageRelativePath,
    event_id: EventId,
    draft_applied_event_id: EventId,
    correlation_id: CorrelationId,
    applied_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolPackagePublishJournalPayloadV1 {
    plan: ToolPackagePublishPlanV1,
    plan_hash: CanonicalSha256HashV1,
}

impl ToolPackagePublishJournalPayloadV1 {
    /// Historical journals are read back, never written: only validation remains.
    fn validate(&self) -> Result<(), PersistenceError> {
        if self.plan.schema_version != TOOL_PACKAGE_PUBLISH_JOURNAL_SCHEMA_VERSION_V1
            || canonical_sha256(&self.plan)
                .map_err(|_| PersistenceError::ToolPackagePublishEvidenceConflict)?
                != self.plan_hash
        {
            return Err(PersistenceError::ToolPackagePublishEvidenceConflict);
        }
        validate_publish_plan(&self.plan)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedToolContractFingerprintEntryV1 {
    pub(crate) tool_key: ToolKey,
    pub(crate) fingerprint: ToolContractFingerprintV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolPackageVersionEvidenceV1 {
    pub(crate) draft: ConformanceDraftRevisionRefV1,
    pub(crate) validation_evidence_hash: CanonicalSha256HashV1,
    pub(crate) approval_evidence_hash: CanonicalSha256HashV1,
    pub(crate) approval_decision: ToolDraftApprovalDecisionV1,
    pub(crate) canonical_manifest: CanonicalToolPackageManifestV1,
    pub(crate) canonical_manifest_hash: CanonicalSha256HashV1,
    pub(crate) runtime_closure_hash: PackageRuntimeClosureHashV1,
    pub(crate) tool_contract_fingerprints: Vec<PersistedToolContractFingerprintEntryV1>,
    pub(crate) conformance_run_ids: Vec<ToolConformanceRunId>,
    pub(crate) mutation_journal_id: MutationJournalId,
}

impl PersistenceCoordinator {
    /// Resolves only an authoritative SQLite-published payload. Directory
    /// enumeration is intentionally excluded from the Runtime path.
    pub fn resolve_installed_tool_package_payload(
        &self,
        tool_package_id: ToolPackageId,
        package_version: &CanonicalSemVer,
    ) -> Result<std::path::PathBuf, PersistenceError> {
        let encoded_version = encode_json(package_version)?;
        let relative = self
            .with_reader(|connection| {
                connection
                    .query_row(
                        "SELECT package_store_relative_path FROM tool_package_payloads
                     WHERE tool_package_id=?1 AND package_version_json=?2
                       AND state='published'",
                        params![tool_package_id.to_string(), encoded_version],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .map(|value| {
                        PackageRelativePath::new(value)
                            .map_err(|_| PersistenceError::ToolEntityIntegrity)
                    })
                    .transpose()
            })?
            .ok_or(PersistenceError::ToolPackagePayloadIntegrity)?;
        ToolPackageStoreV1::open(self.tool_package_store_root())
            .and_then(|store| store.resolve_published_payload(&relative))
            .map_err(|_| PersistenceError::ToolPackagePayloadIntegrity)
    }
}

#[cfg(debug_assertions)]
pub(crate) fn installed_version_reference_id(
    package_id: ToolPackageId,
    version: &CanonicalSemVer,
) -> Result<String, PersistenceError> {
    Ok(format!("{}:{}", package_id, encode_json(version)?))
}

pub(crate) fn package_projection_drafts(
    transaction: &Transaction<'_>,
    package_id: ToolPackageId,
) -> Result<Vec<ToolProjectionDraft>, PersistenceError> {
    let summaries = load_tool_package_summaries(transaction)?;
    let package = load_optional_record::<ToolPackageRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_packages WHERE tool_package_id=?1",
        [package_id.to_string()],
    )?
    .ok_or(PersistenceError::ToolEntityNotFound)?;
    let selected = package_projection(transaction, &package)?;
    let tool_ids = load_bound_records::<ToolDefinitionRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_definitions
         WHERE tool_package_id=?1 ORDER BY tool_id, tool_version_json",
        [package_id.to_string()],
    )?
    .into_iter()
    .map(|definition| definition.tool_id)
    .collect::<BTreeSet<_>>();
    let mut projections = vec![
        ToolProjectionDraft::ToolPackageList(ToolPackageListProjection { packages: summaries }),
        ToolProjectionDraft::ToolPackage(Box::new(selected)),
    ];
    projections.extend(tool_definition_projection_drafts(transaction, Some(&tool_ids))?);
    Ok(projections)
}

pub(crate) fn load_tool_package_summaries(
    transaction: &Transaction<'_>,
) -> Result<Vec<ToolPackageSummaryView>, PersistenceError> {
    let packages = load_bound_records::<ToolPackageRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_packages ORDER BY tool_package_id",
        [],
    )?;
    packages
        .into_iter()
        .map(|package| {
            let detail = package_projection(transaction, &package)?;
            let trusted = detail.active_security_fingerprint.is_some()
                && detail.active_security_fingerprint == detail.trusted_security_fingerprint;
            Ok(ToolPackageSummaryView {
                tool_package_id: package.tool_package_id,
                package_key: package.package_key.clone(),
                active_version: package.active_version.clone(),
                lifecycle: package.lifecycle,
                deletion_pending: package.deletion_pending,
                trusted,
                tool_count: u32::try_from(detail.tools.len())
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                enabled_tool_count: u32::try_from(
                    detail
                        .tools
                        .iter()
                        .filter(|tool| tool.instances.iter().any(|instance| instance.enabled))
                        .count(),
                )
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                eligible_tool_count: u32::try_from(
                    detail
                        .tools
                        .iter()
                        .filter(|tool| tool.instances.iter().any(|instance| instance.ready))
                        .count(),
                )
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                entity_revision: package.entity_revision,
                updated_at: package.updated_at,
            })
        })
        .collect()
}

fn package_projection(
    transaction: &Transaction<'_>,
    package: &ToolPackageRecordV1,
) -> Result<ToolPackageProjection, PersistenceError> {
    let versions = load_bound_records::<ToolPackageVersionRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_package_versions
         WHERE tool_package_id=?1 ORDER BY installed_at, package_version_json",
        [package.tool_package_id.to_string()],
    )?;
    let version_views = versions
        .iter()
        .map(|version| {
            let state = transaction
                .query_row(
                    "SELECT state FROM tool_package_payloads
                     WHERE tool_package_id=?1 AND package_version_json=?2",
                    params![
                        package.tool_package_id.to_string(),
                        encode_json(&version.package_version)?,
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok(ToolPackageVersionSummaryView {
                package_version: version.package_version.clone(),
                package_fingerprint: version.package_fingerprint.clone(),
                package_security_fingerprint: version.package_security_fingerprint.clone(),
                conformance_run_ids: version.conformance_run_ids.clone(),
                conformance_status: ToolConformanceStatusV1::Passed,
                installed_at: version.installed_at,
                payload_available: state.as_deref() == Some("published"),
                payload_gc_eligible_at: version.payload_gc_eligible_at,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    let active_security_fingerprint = package
        .active_version
        .as_ref()
        .and_then(|active| versions.iter().find(|version| &version.package_version == active))
        .map(|version| version.package_security_fingerprint.clone());
    let active_payload_available = package.active_version.as_ref().is_some_and(|active| {
        version_views
            .iter()
            .any(|version| &version.package_version == active && version.payload_available)
    });
    let trusted_security_fingerprint =
        load_optional_record::<aura_tool_domain::ToolPackageTrustRecordV1, _>(
            transaction,
            "SELECT record_json, record_hash_json FROM tool_package_trust
         WHERE tool_package_id=?1",
            [package.tool_package_id.to_string()],
        )?
        .filter(|trust| trust.trusted)
        .map(|trust| trust.package_security_fingerprint);
    let package_trusted = active_security_fingerprint.is_some()
        && active_security_fingerprint == trusted_security_fingerprint;
    let definitions = load_bound_records::<ToolDefinitionRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_definitions
         WHERE tool_package_id=?1 AND retired_at IS NULL
         ORDER BY tool_id, definition_revision",
        [package.tool_package_id.to_string()],
    )?;
    let instances = load_bound_records::<ToolInstanceRecordV1, _>(
        transaction,
        "SELECT i.record_json, i.record_hash_json FROM tool_instances i
         JOIN tool_definitions d
           ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json
         WHERE d.tool_package_id=?1 AND d.retired_at IS NULL
         ORDER BY i.tool_id, i.tool_instance_id",
        [package.tool_package_id.to_string()],
    )?;
    let configurations = load_bound_records::<ToolInstanceConfigurationRecordV1, _>(
        transaction,
        "SELECT c.record_json, c.record_hash_json
         FROM tool_instance_configurations c
         JOIN tool_instances i ON i.tool_instance_id=c.tool_instance_id
         JOIN tool_definitions d
           ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json
         WHERE d.tool_package_id=?1 AND d.retired_at IS NULL
         ORDER BY c.tool_instance_id",
        [package.tool_package_id.to_string()],
    )?
    .into_iter()
    .map(|record| (record.tool_instance_id, record))
    .collect::<BTreeMap<_, _>>();
    let credential_bindings = load_bound_records::<ToolInstanceCredentialBindingRecordV1, _>(
        transaction,
        "SELECT c.record_json, c.record_hash_json
         FROM tool_instance_credential_bindings c
         JOIN tool_instances i ON i.tool_instance_id=c.tool_instance_id
         JOIN tool_definitions d
           ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json
         WHERE d.tool_package_id=?1 AND d.retired_at IS NULL
         ORDER BY c.tool_instance_id",
        [package.tool_package_id.to_string()],
    )?
    .into_iter()
    .map(|record| (record.tool_instance_id, record))
    .collect::<BTreeMap<_, _>>();
    let tools = membership::active_definitions(transaction, package, definitions)?
        .into_iter()
        .map(|definition| {
            let manifest_tool = package_manifest_tool(transaction, &definition)?;
            let instance_views = instances
                .iter()
                .filter(|instance| {
                    instance.tool_id == definition.tool_id
                        && instance.tool_version == definition.tool_version
                })
                .map(|instance| aura_tool_projection::ToolDefinitionInstanceView {
                    tool_instance_id: instance.tool_instance_id,
                    tool_version: instance.tool_version.clone(),
                    enabled: instance.enabled,
                    trusted: package_trusted,
                    runtime_health: instance.runtime_health,
                    ready: package_trusted
                        && active_payload_available
                        && instance.enabled
                        && instance.runtime_health
                            == aura_tool_domain::ToolRuntimeHealthV1::Healthy
                        && package.lifecycle == ToolPackageLifecycleV1::Installed
                        && !package.deletion_pending,
                    entity_revision: instance.entity_revision,
                    configuration: configurations.get(&instance.tool_instance_id).map(|record| {
                        ToolInstanceConfigurationProjectionV1 {
                            safe_non_secret_value: record.configuration.clone(),
                            configuration_revision: record.configuration_revision,
                            configuration_hash: record.configuration_hash.clone(),
                        }
                    }),
                    credential_slot_binding: match manifest_tool.as_ref() {
                        Some(tool) => credential_slot_binding_summary(
                            tool,
                            credential_bindings.get(&instance.tool_instance_id),
                        ),
                        None => credential_slot_binding_summary_for_runtime(
                            &definition.runtime,
                            credential_bindings.get(&instance.tool_instance_id),
                        ),
                    },
                })
                .collect::<Vec<_>>();
            if instance_views.is_empty() {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            Ok(ToolDefinitionSummaryView {
                tool_id: definition.tool_id,
                tool_version: definition.tool_version,
                canonical_name: definition.canonical_name,
                display_name: definition.display_name,
                model_summary: definition.model_summary,
                audience: definition.audience,
                entity_revision: definition.entity_revision,
                instances: instance_views,
            })
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    Ok(ToolPackageProjection {
        tool_package_id: package.tool_package_id,
        package_key: package.package_key.clone(),
        active_version: package.active_version.clone(),
        lifecycle: package.lifecycle,
        deletion_pending: package.deletion_pending,
        active_security_fingerprint,
        trusted_security_fingerprint,
        versions: version_views,
        tools,
        entity_revision: package.entity_revision,
        created_at: package.created_at,
        updated_at: package.updated_at,
        safe_error: None,
    })
}

fn latest_definition_by_tool_key<'a>(
    definitions: &'a [ToolDefinitionRecordV1],
    package_id: ToolPackageId,
) -> BTreeMap<ToolKey, &'a ToolDefinitionRecordV1> {
    let mut latest: BTreeMap<ToolKey, &'a ToolDefinitionRecordV1> = BTreeMap::new();
    for definition in
        definitions.iter().filter(|definition| definition.tool_package_id == Some(package_id))
    {
        let Some(tool_key) = definition.tool_key.clone() else {
            continue;
        };
        match latest.get(&tool_key) {
            Some(previous)
                if previous.tool_version.definition_revision
                    >= definition.tool_version.definition_revision => {}
            _ => {
                latest.insert(tool_key, definition);
            }
        }
    }
    latest
}

fn validate_publish_plan(plan: &ToolPackagePublishPlanV1) -> Result<(), PersistenceError> {
    plan.validation.validate().map_err(|_| PersistenceError::ToolPackagePublishEvidenceConflict)?;
    plan.approval.validate().map_err(|_| PersistenceError::ToolPackagePublishEvidenceConflict)?;
    plan.canonical_manifest
        .validate()
        .map_err(|_| PersistenceError::ToolPackagePublishEvidenceConflict)?;
    for definition in &plan.definitions {
        definition.validate().map_err(|_| PersistenceError::ToolPackagePublishEvidenceConflict)?;
    }
    for instance in &plan.instances {
        instance
            .overrides
            .validate()
            .map_err(|_| PersistenceError::ToolPackagePublishEvidenceConflict)?;
    }
    if plan.draft != plan.validation.draft
        || plan.draft != plan.approval.draft
        || plan.package.tool_package_id != plan.package_version.tool_package_id
        || plan.package.package_key != plan.canonical_manifest.package_key
        || plan.package.active_version.as_ref() != Some(&plan.canonical_manifest.package_version)
        || plan.package_version.package_version != plan.canonical_manifest.package_version
        || plan.package_version.package_fingerprint != plan.validation.package_fingerprint
        || plan.package_version.package_security_fingerprint
            != plan.validation.package_security_fingerprint
        || plan.package_version.manifest_hash != plan.validation.canonical_manifest_hash
        || plan.package_version.package_store_relative_path != plan.package_store_relative_path
        || plan.package_version.conformance_run_ids != plan.approval.conformance_run_ids
        || plan.runtime_closure_hash != plan.validation.runtime_closure_hash
        || canonical_sha256(&plan.runtime_closure_json)
            .map_err(|_| PersistenceError::ToolPackagePublishEvidenceConflict)?
            != plan.runtime_closure_hash.0
        || plan.approval.canonical_manifest_hash != plan.validation.canonical_manifest_hash
        || plan.approval.runtime_closure_hash != plan.validation.runtime_closure_hash
        || plan.approval.package_fingerprint != plan.validation.package_fingerprint
        || plan.approval.package_security_fingerprint
            != plan.validation.package_security_fingerprint
        || plan.approval.decision == ToolDraftApprovalDecisionV1::Reject
        || plan.definitions.len() != plan.canonical_manifest.tools.len()
        || plan.instances.len() != plan.definitions.len()
        || plan.tool_contract_fingerprints.len() != plan.canonical_manifest.tools.len()
        || !plan.tool_contract_fingerprints.iter().zip(&plan.canonical_manifest.tools).all(
            |(entry, tool)| {
                entry.tool_key == tool.tool_key
                    && entry.fingerprint.package_runtime_closure_hash == plan.runtime_closure_hash
            },
        )
        || !plan.definitions.iter().all(|definition| {
            definition.tool_package_id == Some(plan.package.tool_package_id)
                && definition.package_key.as_ref() == Some(&plan.package.package_key)
        })
        || !plan.instances.iter().all(|instance| {
            plan.definitions.iter().any(|definition| {
                definition.tool_id == instance.tool_id
                    && definition.tool_version == instance.tool_version
            })
        })
    {
        return Err(PersistenceError::ToolPackagePublishEvidenceConflict);
    }
    match plan.operation {
        ToolPackagePublishOperationV1::Install
            if plan.previous_package.is_some() || plan.previous_package_version.is_some() =>
        {
            Err(PersistenceError::ToolPackagePublishEvidenceConflict)
        }
        ToolPackagePublishOperationV1::Update
            if plan.previous_package.is_none() || plan.previous_package_version.is_none() =>
        {
            Err(PersistenceError::ToolPackagePublishEvidenceConflict)
        }
        _ => Ok(()),
    }
}
