mod package_scope;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use aura_domain_common::ToolInstanceId;
use aura_domain_common::jcs::{CanonicalHashV1, canonical_json_jcs, canonical_sha256};
use aura_domain_common::{CorrelationId, EventId, Revision, UtcTimestamp};
use aura_permission_domain::{PackageRuntimeScopeV1, ResolvedScope};
use aura_projection_contract::ProjectionChange;
use aura_tool_contract::{
    CompiledToolSchemaV1, PackageKey, PackageSecurityFingerprintV1, ToolDefinitionSnapshotRef,
    ToolLifecycleEventV1, ToolLoadRequestV1, ToolLoadResultV1, ToolLoadStateV1,
    ToolRegistryScopeV1, ToolRegistrySnapshotCreatedPayloadV1, ToolRegistrySnapshotRef,
    ToolSettingsSnapshotV1, ToolSourceContractFingerprintV1,
};
use aura_tool_domain::{
    CanonicalToolPackageManifestV1, CanonicalToolPackageRuntimeV1, ToolDefinitionRecordV1,
    ToolEphemeralLoadRecordV1, ToolInstanceConfigurationRecordV1,
    ToolInstanceCredentialBindingRecordV1, ToolInstanceRecordV1, ToolPackageLifecycleV1,
    ToolPackageTrustRecordV1,
};
use aura_tool_package::verify_installed_tool_package_v1;
use aura_tool_projection::{
    EffectiveToolView, SessionEffectiveToolRegistryProjection, SessionLoadedToolsProjection,
    SessionToolPinsProjectionV1, ToolDefinitionSourceSelectionRefV1, ToolPinProjectionEntryV1,
    WorkspaceToolRegistrySummaryProjection,
};
use aura_tool_registry::{
    ParentRegistryBuildInputV1, ParentRegistryBuildOutputV1, RegistryCandidateV1,
    RegistryExclusionV1, ToolInstanceExecutionBindingV1, ToolSearchCorpusV1,
    ToolSourceEligibilityEvidenceV1, build_parent_registry_snapshot,
};
use aura_tool_scheduler::{ValidatedToolCallV1, security_source_for_admitted_tool_call_v1};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(debug_assertions)]
use crate::tool_package_reads::{INSTALLED_VERSION_REFERENCE_KIND, installed_version_reference_id};
use crate::{
    EventActor, PersistenceCoordinator, PersistenceError, ToolEntityMutationCommit,
    ToolProjectionDraft,
    coordinator::insert_event,
    tool::{encode_canonical, insert_tool_registry_search_corpus, sqlite_revision},
    tool_entities::{
        active_package_security_fingerprint, advance_registry_revision, decode_json,
        decode_verified_record, encode_json, encode_record, load_optional_record,
        next_entity_revision, read_registry_revision, tool_event,
    },
    tool_package_reads::{ToolPackageVersionEvidenceV1, package_projection_drafts},
    tool_projection::insert_tool_projections,
};

#[derive(Clone, Debug)]
pub struct ToolPackageTrustIntentApplyRequestV1 {
    pub tool_package_id: aura_domain_common::ToolPackageId,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ToolInstanceConfigurationChangeRequestV1 {
    pub tool_instance_id: ToolInstanceId,
    pub expected_revision: Option<Revision>,
    pub configuration: Value,
    pub secret_scan_evidence_hash: CanonicalHashV1,
    pub changed_at: UtcTimestamp,
}

#[derive(Clone, Debug)]
pub struct ToolRegistryBuildRequestV1 {
    pub scope: ToolRegistryScopeV1,
    pub settings: ToolSettingsSnapshotV1,
    pub mode_permission_context_hash: CanonicalHashV1,
    pub core_exposed_tool_ids: BTreeSet<aura_domain_common::ToolId>,
    /// Built-in/MCP/Plugin candidates have separately verified source evidence.
    /// Installed Package candidates are always reconstructed from SQLite plus
    /// live immutable-Payload verification and must not be supplied here.
    pub non_package_candidates: Vec<RegistryCandidateV1>,
    pub created_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ToolRegistryBuildCommitV1 {
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub exclusions: Vec<RegistryExclusionV1>,
    pub event_id: EventId,
    pub projection_changes: Vec<ProjectionChange>,
}

#[derive(Clone, Debug)]
pub struct ToolRegistryMutationCommitV1 {
    pub result: ToolLoadResultV1,
    pub projection_changes: Vec<ProjectionChange>,
}

#[derive(Clone, Debug)]
pub struct ExactToolDefinitionDescriptionV1 {
    pub definition: aura_tool_domain::ToolDefinitionRecordV1,
    pub instance: aura_tool_domain::ToolInstanceRecordV1,
}

/// Execution-relevant instance state frozen into the exact immutable
/// Registry Snapshot. Configuration and credential changes create a later
/// Snapshot and never rewrite this record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRegistryExecutionAuthorityRecordV1 {
    pub registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub tool_id: aura_domain_common::ToolId,
    pub tool_version: aura_tool_contract::ToolVersion,
    pub tool_instance_id: ToolInstanceId,
    pub instance_entity_revision: Revision,
    pub instance_configuration: Option<ToolInstanceConfigurationRecordV1>,
    pub credential_binding: Option<ToolInstanceCredentialBindingRecordV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_binding: Option<ToolInstanceExecutionBindingV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactToolPackageExecutionAuthorityV1 {
    tool_package_id: aura_domain_common::ToolPackageId,
    package_version: aura_domain_common::CanonicalSemVer,
    runtime_root: PathBuf,
    manifest_tool: aura_tool_domain::CanonicalToolPackageToolV1,
    canonical_manifest_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
    runtime_closure_hash: aura_tool_contract::PackageRuntimeClosureHashV1,
    package_security_fingerprint: PackageSecurityFingerprintV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactToolExecutionAuthorityV1 {
    pub registry: aura_tool_contract::ToolRegistrySnapshotV1,
    pub definition: ToolDefinitionRecordV1,
    pub instance: ToolInstanceRecordV1,
    pub frozen: ToolRegistryExecutionAuthorityRecordV1,
    pub package: Option<ExactToolPackageExecutionAuthorityV1>,
}

impl ExactToolPackageExecutionAuthorityV1 {
    #[must_use]
    pub const fn tool_package_id(&self) -> aura_domain_common::ToolPackageId {
        self.tool_package_id
    }

    #[must_use]
    pub const fn package_version(&self) -> &aura_domain_common::CanonicalSemVer {
        &self.package_version
    }

    #[must_use]
    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    #[must_use]
    pub const fn manifest_tool(&self) -> &aura_tool_domain::CanonicalToolPackageToolV1 {
        &self.manifest_tool
    }

    #[must_use]
    pub const fn canonical_manifest_hash(&self) -> &aura_domain_common::jcs::CanonicalSha256HashV1 {
        &self.canonical_manifest_hash
    }

    #[must_use]
    pub const fn runtime_closure_hash(&self) -> &aura_tool_contract::PackageRuntimeClosureHashV1 {
        &self.runtime_closure_hash
    }

    #[must_use]
    pub const fn package_security_fingerprint(&self) -> &PackageSecurityFingerprintV1 {
        &self.package_security_fingerprint
    }
}

impl ExactToolExecutionAuthorityV1 {
    #[must_use]
    pub const fn builtin_execution_binding(&self) -> Option<ToolInstanceExecutionBindingV1> {
        self.frozen.execution_binding
    }
}

#[derive(Clone)]
struct LoadedToolPackageVersionEvidenceV1 {
    tool_package_id: aura_domain_common::ToolPackageId,
    package_version: aura_domain_common::CanonicalSemVer,
    evidence: ToolPackageVersionEvidenceV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct PayloadBindingHashInput<'a> {
    tool_registry_snapshot_id: aura_domain_common::ToolRegistrySnapshotId,
    tool_id: aura_domain_common::ToolId,
    tool_package_id: aura_domain_common::ToolPackageId,
    package_version: &'a aura_domain_common::CanonicalSemVer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EphemeralMutationV1 {
    Load,
    Unload,
}

impl PersistenceCoordinator {
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn seed_tool_registry_runtime_authority_for_test(
        &self,
        snapshot_ref: ToolRegistrySnapshotRef,
        definition: ToolDefinitionRecordV1,
        instance: ToolInstanceRecordV1,
        package_version: aura_tool_domain::ToolPackageVersionRecordV1,
    ) -> Result<(), PersistenceError> {
        if definition.tool_id != instance.tool_id
            || definition.tool_version != instance.tool_version
            || definition.tool_package_id != Some(package_version.tool_package_id)
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (definition_json, definition_hash_json) = encode_record(&definition)?;
            let (instance_json, instance_hash_json) = encode_record(&instance)?;
            transaction.execute(
                "INSERT INTO tool_registry_definition_records(
                    tool_registry_snapshot_id, tool_id, tool_version_json,
                    definition_json, definition_hash_json, instance_json, instance_hash_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    snapshot_ref.tool_registry_snapshot_id.to_string(),
                    definition.tool_id.to_string(),
                    encode_canonical(&definition.tool_version)?,
                    definition_json,
                    definition_hash_json,
                    instance_json,
                    instance_hash_json,
                ],
            )?;
            let frozen = ToolRegistryExecutionAuthorityRecordV1 {
                registry_snapshot_ref: snapshot_ref.clone(),
                tool_id: definition.tool_id,
                tool_version: definition.tool_version.clone(),
                tool_instance_id: instance.tool_instance_id,
                instance_entity_revision: instance.entity_revision,
                instance_configuration: None,
                credential_binding: None,
                execution_binding: None,
            };
            let (authority_json, authority_hash_json) = encode_record(&frozen)?;
            transaction.execute(
                "INSERT INTO tool_registry_execution_authority_records(
                    tool_registry_snapshot_id, tool_id, authority_json, authority_hash_json
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    snapshot_ref.tool_registry_snapshot_id.to_string(),
                    definition.tool_id.to_string(),
                    authority_json,
                    authority_hash_json,
                ],
            )?;
            let binding_hash = canonical_sha256(&PayloadBindingHashInput {
                tool_registry_snapshot_id: snapshot_ref.tool_registry_snapshot_id,
                tool_id: definition.tool_id,
                tool_package_id: package_version.tool_package_id,
                package_version: &package_version.package_version,
            })
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
            transaction.execute(
                "INSERT INTO tool_registry_package_payload_bindings(
                    tool_registry_snapshot_id, tool_id, tool_package_id,
                    package_version_json, binding_hash_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    snapshot_ref.tool_registry_snapshot_id.to_string(),
                    definition.tool_id.to_string(),
                    package_version.tool_package_id.to_string(),
                    encode_canonical(&package_version.package_version)?,
                    encode_json(&binding_hash)?,
                ],
            )?;
            let (payload_record_json, payload_record_hash_json) = encode_record(&package_version)?;
            transaction.execute(
                "INSERT OR IGNORE INTO tool_package_payloads(
                    tool_package_id, package_version_json, package_fingerprint_json,
                    package_security_fingerprint_json, package_store_relative_path,
                    state, mutation_journal_id, record_json, record_hash_json,
                    published_at, quarantined_at, deleted_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'published', NULL, ?6, ?7, ?8, NULL, NULL)",
                params![
                    package_version.tool_package_id.to_string(),
                    encode_json(&package_version.package_version)?,
                    encode_json(&package_version.package_fingerprint)?,
                    encode_json(&package_version.package_security_fingerprint)?,
                    package_version.package_store_relative_path.to_string(),
                    payload_record_json,
                    payload_record_hash_json,
                    package_version.installed_at.to_string(),
                ],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO tool_package_payload_strong_references(
                    reference_kind, reference_id, tool_package_id, package_version_json,
                    created_at, released_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                params![
                    INSTALLED_VERSION_REFERENCE_KIND,
                    installed_version_reference_id(
                        package_version.tool_package_id,
                        &package_version.package_version,
                    )?,
                    package_version.tool_package_id.to_string(),
                    encode_json(&package_version.package_version)?,
                    package_version.installed_at.to_string(),
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    /// Consumes only the exact W5 `install_and_trust` intent. Failure leaves
    /// the Package installed/enabled but untrusted so the same fingerprint may
    /// be retried explicitly without replaying Package publication.
    pub fn apply_tool_package_trust_intent(
        &self,
        request: ToolPackageTrustIntentApplyRequestV1,
    ) -> Result<ToolEntityMutationCommit, PersistenceError> {
        if !request.package_security_fingerprint.uses_blake3() {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let active =
                active_package_security_fingerprint(&transaction, request.tool_package_id)?
                    .ok_or(PersistenceError::ToolEntityStateConflict)?;
            if active != request.package_security_fingerprint {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            let encoded_fingerprint = encode_json(&request.package_security_fingerprint)?;
            let state = transaction
                .query_row(
                    "SELECT state FROM tool_package_trust_intents
                     WHERE tool_package_id=?1 AND package_security_fingerprint_json=?2",
                    params![request.tool_package_id.to_string(), encoded_fingerprint],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or(PersistenceError::ToolEntityNotFound)?;
            let current = load_optional_record::<ToolPackageTrustRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_package_trust
                 WHERE tool_package_id=?1",
                [request.tool_package_id.to_string()],
            )?;
            if state == "applied"
                && current.as_ref().is_some_and(|record| {
                    record.trusted
                        && record.package_security_fingerprint
                            == request.package_security_fingerprint
                })
            {
                return Ok(ToolEntityMutationCommit {
                    changed: false,
                    registry_revision: read_registry_revision(&transaction)?,
                    event_id: None,
                    projection_changes: Vec::new(),
                });
            }
            if state != "pending" && state != "failed" {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            let entity_revision =
                next_entity_revision(current.as_ref().map(|record| record.entity_revision))?;
            let record = ToolPackageTrustRecordV1 {
                tool_package_id: request.tool_package_id,
                package_security_fingerprint: request.package_security_fingerprint.clone(),
                trusted: true,
                entity_revision,
                reviewed_at: request.changed_at,
            };
            let (record_json, record_hash_json) = encode_record(&record)?;
            transaction.execute(
                "INSERT INTO tool_package_trust(
                    tool_package_id, package_security_fingerprint_json, trusted,
                    entity_revision, record_json, record_hash_json, reviewed_at
                 ) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6)
                 ON CONFLICT(tool_package_id) DO UPDATE SET
                    package_security_fingerprint_json=excluded.package_security_fingerprint_json,
                    trusted=1, entity_revision=excluded.entity_revision,
                    record_json=excluded.record_json, record_hash_json=excluded.record_hash_json,
                    reviewed_at=excluded.reviewed_at",
                params![
                    request.tool_package_id.to_string(),
                    encode_json(&request.package_security_fingerprint)?,
                    sqlite_revision(entity_revision)?,
                    record_json,
                    record_hash_json,
                    request.changed_at.to_string(),
                ],
            )?;
            transaction.execute(
                "UPDATE tool_package_trust_intents SET state='applied', updated_at=?1
                 WHERE tool_package_id=?2 AND package_security_fingerprint_json=?3
                   AND state IN ('pending','failed')",
                params![
                    request.changed_at.to_string(),
                    request.tool_package_id.to_string(),
                    encode_json(&request.package_security_fingerprint)?,
                ],
            )?;
            let registry_revision = advance_registry_revision(&transaction, request.changed_at)?;
            let event_id = EventId::new();
            let event = tool_event(
                event_id,
                EventActor::User,
                request.correlation_id,
                ToolLifecycleEventV1::ToolTrustChanged(
                    aura_tool_contract::ToolTrustChangedPayloadV1 {
                        tool_package_id: request.tool_package_id,
                        package_security_fingerprint: request.package_security_fingerprint,
                        trusted: true,
                        entity_revision,
                        changed_at: request.changed_at,
                    },
                ),
                request.changed_at,
            )?;
            insert_event(&transaction, &event)?;
            let projections = package_projection_drafts(&transaction, request.tool_package_id)?;
            let (projection_changes, _) =
                insert_tool_projections(&transaction, runtime_instance_id, projections)?;
            transaction.commit()?;
            Ok(ToolEntityMutationCommit {
                changed: true,
                registry_revision,
                event_id: Some(event_id),
                projection_changes,
            })
        })
    }

    pub fn commit_tool_instance_configuration(
        &self,
        request: ToolInstanceConfigurationChangeRequestV1,
    ) -> Result<ToolEntityMutationCommit, PersistenceError> {
        let schema = self
            .instance_configuration_schema(request.tool_instance_id)?
            .ok_or(PersistenceError::ToolEntityStateConflict)?;
        let validator = CompiledToolSchemaV1::compile(schema.canonical_document())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if !validator.validate(&request.configuration).valid {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            ensure_instance_mutable(&transaction, request.tool_instance_id)?;
            let current = load_optional_record::<ToolInstanceConfigurationRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_instance_configurations
                 WHERE tool_instance_id=?1",
                [request.tool_instance_id.to_string()],
            )?;
            validate_optional_revision(
                current.as_ref().map(|record| record.configuration_revision),
                request.expected_revision,
            )?;
            let next_revision =
                next_entity_revision(current.as_ref().map(|record| record.configuration_revision))?;
            let record = ToolInstanceConfigurationRecordV1::new(
                request.tool_instance_id,
                next_revision,
                schema.schema_hash().clone(),
                request.configuration,
                request.secret_scan_evidence_hash,
                request.changed_at,
            )
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            let (record_json, record_hash_json) = encode_record(&record)?;
            transaction.execute(
                "INSERT INTO tool_instance_configurations(
                    tool_instance_id, configuration_revision,
                    instance_config_schema_hash_json, configuration_json,
                    configuration_hash_json, secret_scan_evidence_hash_json,
                    record_json, record_hash_json, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(tool_instance_id) DO UPDATE SET
                    configuration_revision=excluded.configuration_revision,
                    instance_config_schema_hash_json=excluded.instance_config_schema_hash_json,
                    configuration_json=excluded.configuration_json,
                    configuration_hash_json=excluded.configuration_hash_json,
                    secret_scan_evidence_hash_json=excluded.secret_scan_evidence_hash_json,
                    record_json=excluded.record_json, record_hash_json=excluded.record_hash_json,
                    updated_at=excluded.updated_at",
                params![
                    request.tool_instance_id.to_string(),
                    sqlite_revision(next_revision)?,
                    encode_json(&record.instance_config_schema_hash)?,
                    encode_canonical(&record.configuration)?,
                    encode_json(&record.configuration_hash)?,
                    encode_json(&record.secret_scan_evidence_hash)?,
                    record_json,
                    record_hash_json,
                    request.changed_at.to_string(),
                ],
            )?;
            let registry_revision = advance_registry_revision(&transaction, request.changed_at)?;
            transaction.commit()?;
            Ok(ToolEntityMutationCommit {
                changed: true,
                registry_revision,
                event_id: None,
                projection_changes: Vec::new(),
            })
        })
    }

    /// Reconstructs Package eligibility from authoritative SQLite records and
    /// freshly re-read immutable Package bytes, then atomically commits the
    /// new `trs_`, lifecycle Event, scope head, and renderer projection.
    pub fn build_and_commit_tool_registry_snapshot(
        &self,
        request: ToolRegistryBuildRequestV1,
    ) -> Result<ToolRegistryBuildCommitV1, PersistenceError> {
        request.settings.validate().map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
        let (workspace_id, session_id, turn_task_id) = scope_ids(&request.scope);
        let authority = self.tool_authority_snapshot(workspace_id, session_id, turn_task_id)?;
        let version_evidence = self.load_tool_package_version_evidence()?;
        let builtin_authority = self.validated_signed_builtin_release_authority()?;
        let builtin_release_hash = aura_tool_registry::embedded_builtin_tool_definition_set_v1()
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?
            .signed_release_hash()
            .clone();
        let mut candidates = request.non_package_candidates.clone();
        if candidates.iter().any(|candidate| {
            candidate.definition.tool_package_id.is_some()
                || candidate.definition.source_kind == aura_tool_contract::ToolSourceKindV1::BuiltIn
        }) {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        candidates.extend(builtin_authority.iter().map(|entry| RegistryCandidateV1 {
            definition: entry.definition.clone(),
            instance: entry.instance.clone(),
            source_evidence: ToolSourceEligibilityEvidenceV1::VerifiedBuiltinRelease {
                release_manifest_hash: builtin_release_hash.clone(),
                verification_evidence_hash:
                    entry.release_entry.source_contract_fingerprint.fingerprint().clone(),
            },
            active_for_unpinned: true,
        }));
        candidates.extend(self.package_registry_candidates(
            &authority,
            &version_evidence,
            workspace_id,
            session_id,
        )?);
        let projection_candidates = candidates.clone();
        let builtin_execution_bindings = builtin_authority
            .iter()
            .map(|entry| (entry.instance.tool_instance_id, entry.execution_binding))
            .collect::<BTreeMap<_, _>>();
        let mut core_exposed_tool_ids = request.core_exposed_tool_ids;
        core_exposed_tool_ids
            .extend(builtin_authority.iter().map(|entry| entry.definition.tool_id));
        let input = ParentRegistryBuildInputV1 {
            registry_revision: authority.registry_revision,
            scope: request.scope.clone(),
            settings: request.settings,
            mode_permission_context_hash: request.mode_permission_context_hash,
            core_exposed_tool_ids,
            candidates,
            workspace_pins: authority
                .workspace_pins
                .iter()
                .filter_map(pin_value)
                .cloned()
                .collect(),
            session_pins: authority.session_pins.iter().filter_map(pin_value).cloned().collect(),
            ephemeral_loads: authority.ephemeral_loads.clone(),
            created_at: request.created_at,
        };
        let output = build_parent_registry_snapshot(input)
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
        let corpus =
            ToolSearchCorpusV1::new(output.snapshot.clone(), output.search_documents.clone())
                .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
        let runtime_instance_id = self.runtime_instance_id();
        let snapshot_ref = output.snapshot.snapshot_ref.clone();
        let exclusions = output.exclusions.clone();
        let projections = registry_projections(
            &output,
            &projection_candidates,
            &authority,
            session_id,
            workspace_id,
            turn_task_id,
        )?;
        let scope = request.scope;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            if read_registry_revision(&transaction)? != snapshot_ref.registry_revision {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            insert_tool_registry_search_corpus(&transaction, &corpus)?;
            insert_registry_definition_records(
                &transaction,
                &corpus.snapshot,
                &projection_candidates,
                &version_evidence,
            )?;
            insert_registry_execution_authority_records(
                &transaction,
                &corpus.snapshot,
                &projection_candidates,
                &authority,
                &builtin_execution_bindings,
            )?;
            let scope_hash =
                canonical_sha256(&scope).map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
            transaction.execute(
                "INSERT INTO tool_registry_scope_heads(
                    scope_hash_json, scope_json, tool_registry_snapshot_id,
                    registry_revision, content_hash_json, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(scope_hash_json) DO UPDATE SET
                    scope_json=excluded.scope_json,
                    tool_registry_snapshot_id=excluded.tool_registry_snapshot_id,
                    registry_revision=excluded.registry_revision,
                    content_hash_json=excluded.content_hash_json,
                    updated_at=excluded.updated_at",
                params![
                    encode_json(&scope_hash)?,
                    encode_canonical(&scope)?,
                    snapshot_ref.tool_registry_snapshot_id.to_string(),
                    sqlite_revision(snapshot_ref.registry_revision)?,
                    encode_json(&snapshot_ref.content_hash)?,
                    request.created_at.to_string(),
                ],
            )?;
            let event_id = EventId::new();
            let mut event = tool_event(
                event_id,
                EventActor::SystemRuntime,
                request.correlation_id,
                ToolLifecycleEventV1::ToolRegistrySnapshotCreated(
                    ToolRegistrySnapshotCreatedPayloadV1 {
                        registry_snapshot_ref: snapshot_ref.clone(),
                        scope: scope.clone(),
                        tool_set_hash: corpus.snapshot.schema_set.tool_set_hash.clone(),
                        binding_set_hash: canonical_sha256(&corpus.snapshot.effective_bindings)
                            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?,
                        created_at: request.created_at,
                    },
                ),
                request.created_at,
            )?;
            event.workspace_id = workspace_id;
            event.session_id = session_id;
            event.turn_task_id = turn_task_id;
            insert_event(&transaction, &event)?;
            let (projection_changes, _) =
                insert_tool_projections(&transaction, runtime_instance_id, projections)?;
            transaction.commit()?;
            Ok(ToolRegistryBuildCommitV1 {
                registry_snapshot_ref: snapshot_ref,
                exclusions,
                event_id,
                projection_changes,
            })
        })
    }

    /// Resolves a Tool definition only from the exact immutable Registry
    /// Snapshot named by the caller. This never substitutes a newer version,
    /// instance, or Snapshot.
    pub fn describe_tool_definition_exact(
        &self,
        definition_ref: &ToolDefinitionSnapshotRef,
    ) -> Result<Option<ExactToolDefinitionDescriptionV1>, PersistenceError> {
        if self.read_tool_registry_search_corpus(&definition_ref.registry_snapshot_ref)?.is_none() {
            return Ok(None);
        }
        let snapshot_id =
            definition_ref.registry_snapshot_ref.tool_registry_snapshot_id.to_string();
        let tool_id = definition_ref.tool_id.to_string();
        let tool_version = encode_canonical(&definition_ref.tool_version)?;
        self.with_reader(move |connection| {
            let row = connection
                .query_row(
                    "SELECT definition_json, definition_hash_json,
                            instance_json, instance_hash_json
                     FROM tool_registry_definition_records
                     WHERE tool_registry_snapshot_id=?1 AND tool_id=?2
                       AND tool_version_json=?3",
                    params![snapshot_id, tool_id, tool_version],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()?;
            let Some((definition_json, definition_hash_json, instance_json, instance_hash_json)) =
                row
            else {
                return Ok(None);
            };
            let definition = crate::tool_entities::decode_verified_record(
                &definition_json,
                &definition_hash_json,
            )?;
            let instance =
                crate::tool_entities::decode_verified_record(&instance_json, &instance_hash_json)?;
            Ok(Some(ExactToolDefinitionDescriptionV1 { definition, instance }))
        })
    }

    /// Resolves a global projection-only source reference into an exact
    /// `trs_` binding owned by the requested target scope. The source
    /// reference itself is never accepted by admission or execution.
    pub fn resolve_source_selection_in_target_registry(
        &self,
        source_selection_ref: &ToolDefinitionSourceSelectionRefV1,
        target_snapshot_ref: &ToolRegistrySnapshotRef,
        target_scope: &ToolRegistryScopeV1,
    ) -> Result<ToolDefinitionSnapshotRef, PersistenceError> {
        let corpus = self
            .read_tool_registry_search_corpus(target_snapshot_ref)?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
        if &corpus.snapshot.scope != target_scope {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        let tool_id = source_selection_ref.tool_id();
        let (tool_version, tool_instance_id, tool_instance_revision, definition_fingerprint) =
            match source_selection_ref {
                ToolDefinitionSourceSelectionRefV1::SignedBuiltinRelease {
                    tool_version,
                    tool_instance_id,
                    tool_instance_revision,
                    definition_fingerprint,
                    ..
                }
                | ToolDefinitionSourceSelectionRefV1::ActivePackageVersion {
                    tool_version,
                    tool_instance_id,
                    tool_instance_revision,
                    definition_fingerprint,
                    ..
                }
                | ToolDefinitionSourceSelectionRefV1::ExternalSourceAuthority {
                    tool_version,
                    tool_instance_id,
                    tool_instance_revision,
                    definition_fingerprint,
                    ..
                } => (
                    tool_version,
                    *tool_instance_id,
                    *tool_instance_revision,
                    definition_fingerprint,
                ),
            };
        let catalog_entry = corpus
            .snapshot
            .catalog_entries
            .iter()
            .find(|entry| {
                entry.tool_id == tool_id
                    && &entry.tool_version == tool_version
                    && entry.tool_instance_id == tool_instance_id
            })
            .ok_or(PersistenceError::ToolEntityStateConflict)?;
        if &catalog_entry.definition_fingerprint != definition_fingerprint {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
        let definition_ref = ToolDefinitionSnapshotRef {
            tool_id,
            tool_version: tool_version.clone(),
            registry_snapshot_ref: target_snapshot_ref.clone(),
        };
        let exact = self
            .load_exact_tool_execution_authority(&definition_ref, None)?
            .ok_or(PersistenceError::ToolEntityStateConflict)?;
        if exact.instance.tool_instance_id != tool_instance_id
            || exact.instance.entity_revision != tool_instance_revision
            || &exact.definition.definition_fingerprint != definition_fingerprint
        {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
        match source_selection_ref {
            ToolDefinitionSourceSelectionRefV1::SignedBuiltinRelease {
                signed_builtin_release_hash,
                source_contract_fingerprint,
                ..
            } => {
                let release = aura_tool_registry::embedded_builtin_tool_definition_set_v1()
                    .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
                let authority = self.validated_signed_builtin_release_authority()?;
                let matches = release.signed_release_hash() == signed_builtin_release_hash.hash()
                    && authority.iter().any(|entry| {
                        entry.definition == exact.definition
                            && entry.instance == exact.instance
                            && &entry.release_entry.source_contract_fingerprint
                                == source_contract_fingerprint
                    });
                if !matches {
                    return Err(PersistenceError::ToolEntityStateConflict);
                }
            }
            ToolDefinitionSourceSelectionRefV1::ActivePackageVersion {
                tool_package_id,
                package_version,
                package_revision,
                package_fingerprint,
                package_security_fingerprint,
                ..
            } => {
                let authority = self.tool_authority_snapshot(None, None, None)?;
                let package = authority
                    .packages
                    .iter()
                    .find(|package| package.tool_package_id == *tool_package_id)
                    .ok_or(PersistenceError::ToolEntityStateConflict)?;
                let version = authority
                    .package_versions
                    .iter()
                    .find(|version| {
                        version.tool_package_id == *tool_package_id
                            && version.package_version == *package_version
                    })
                    .ok_or(PersistenceError::ToolEntityStateConflict)?;
                if package.entity_revision != *package_revision
                    || package.active_version.as_ref() != Some(package_version)
                    || package.lifecycle != ToolPackageLifecycleV1::Installed
                    || package.deletion_pending
                    || version.package_fingerprint != *package_fingerprint
                    || version.package_security_fingerprint != *package_security_fingerprint
                    || exact.definition.tool_package_id != Some(*tool_package_id)
                {
                    return Err(PersistenceError::ToolEntityStateConflict);
                }
            }
            ToolDefinitionSourceSelectionRefV1::ExternalSourceAuthority { .. } => {
                // No external source authority is persisted in P07 yet. It
                // cannot be reinterpreted as Package authority or executed.
                return Err(PersistenceError::ToolEntityStateConflict);
            }
        }
        Ok(definition_ref)
    }

    /// Loads every execution-relevant fact from the exact immutable `trs_`.
    /// With no admitted batch, the Package must still accept new work. With an
    /// admitted batch, a live payload lease permits an already admitted call
    /// to finish across Package deletion without adopting newer state.
    pub fn load_exact_tool_execution_authority(
        &self,
        definition_ref: &ToolDefinitionSnapshotRef,
        admitted_batch_id: Option<aura_domain_common::ToolBatchId>,
    ) -> Result<Option<ExactToolExecutionAuthorityV1>, PersistenceError> {
        let Some(corpus) =
            self.read_tool_registry_search_corpus(&definition_ref.registry_snapshot_ref)?
        else {
            return Ok(None);
        };
        let Some(description) = self.describe_tool_definition_exact(definition_ref)? else {
            return Ok(None);
        };
        let snapshot_id = definition_ref.registry_snapshot_ref.tool_registry_snapshot_id;
        let tool_id = definition_ref.tool_id;
        let frozen = self.with_reader(move |connection| {
            let row = connection
                .query_row(
                    "SELECT authority_json, authority_hash_json
                     FROM tool_registry_execution_authority_records
                     WHERE tool_registry_snapshot_id=?1 AND tool_id=?2",
                    params![snapshot_id.to_string(), tool_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            row.map(|(record, hash)| decode_verified_record(&record, &hash)).transpose()
        })?;
        let Some(frozen): Option<ToolRegistryExecutionAuthorityRecordV1> = frozen else {
            return Err(PersistenceError::ToolRegistryIntegrity);
        };
        if frozen.registry_snapshot_ref != definition_ref.registry_snapshot_ref
            || frozen.tool_id != definition_ref.tool_id
            || frozen.tool_version != definition_ref.tool_version
            || frozen.tool_instance_id != description.instance.tool_instance_id
            || frozen.instance_entity_revision != description.instance.entity_revision
            || description.definition.validate().is_err()
            || description.instance.tool_id != description.definition.tool_id
            || description.instance.tool_version != description.definition.tool_version
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        if frozen.instance_configuration.as_ref().is_some_and(|record| {
            record.tool_instance_id != frozen.tool_instance_id || record.validate().is_err()
        }) || frozen.credential_binding.as_ref().is_some_and(|record| {
            record.tool_instance_id != frozen.tool_instance_id || record.validate().is_err()
        }) {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }

        let authority = self.tool_authority_snapshot(None, None, None)?;
        let current_instance = authority
            .instances
            .iter()
            .find(|instance| instance.tool_instance_id == frozen.tool_instance_id)
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        if !current_instance.enabled
            || current_instance.runtime_health != aura_tool_domain::ToolRuntimeHealthV1::Healthy
        {
            return Err(PersistenceError::ToolEntityStateConflict);
        }

        let is_builtin =
            description.definition.source_kind == aura_tool_contract::ToolSourceKindV1::BuiltIn;
        if is_builtin {
            if current_instance.source_kind != aura_tool_contract::ToolSourceKindV1::BuiltIn
                || current_instance.tool_id != description.definition.tool_id
                || frozen.execution_binding.is_none()
            {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
        } else if frozen.execution_binding.is_some() {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }

        let package = match description.definition.tool_package_id {
            None => None,
            Some(tool_package_id) => {
                let binding = self.load_registry_payload_binding(
                    &definition_ref.registry_snapshot_ref,
                    definition_ref.tool_id,
                )?;
                let (bound_package_id, package_version) =
                    binding.ok_or(PersistenceError::ToolRegistryIntegrity)?;
                if bound_package_id != tool_package_id {
                    return Err(PersistenceError::ToolRegistryIntegrity);
                }
                let package_record = authority
                    .packages
                    .iter()
                    .find(|package| package.tool_package_id == tool_package_id)
                    .ok_or(PersistenceError::ToolEntityIntegrity)?;
                let version_record = authority
                    .package_versions
                    .iter()
                    .find(|version| {
                        version.tool_package_id == tool_package_id
                            && version.package_version == package_version
                    })
                    .ok_or(PersistenceError::ToolEntityIntegrity)?;
                let deletion_lease = admitted_batch_id
                    .map(|batch_id| {
                        self.has_live_batch_payload_lease(
                            batch_id,
                            definition_ref.tool_id,
                            tool_package_id,
                            &package_version,
                        )
                    })
                    .transpose()?
                    .unwrap_or(false);
                if package_record.lifecycle != ToolPackageLifecycleV1::Installed && !deletion_lease
                {
                    return Err(PersistenceError::ToolEntityStateConflict);
                }
                if package_record.deletion_pending && !deletion_lease {
                    return Err(PersistenceError::ToolEntityStateConflict);
                }
                if !package_record.deletion_pending
                    && package_record.lifecycle == ToolPackageLifecycleV1::Installed
                {
                    let trusted = authority.trust_records.iter().any(|trust| {
                        trust.tool_package_id == tool_package_id
                            && trust.trusted
                            && trust.package_security_fingerprint
                                == version_record.package_security_fingerprint
                    });
                    if !trusted {
                        return Err(PersistenceError::ToolEntityStateConflict);
                    }
                }

                let evidence = self
                    .load_tool_package_version_evidence()?
                    .into_iter()
                    .find(|evidence| {
                        evidence.tool_package_id == tool_package_id
                            && evidence.package_version == package_version
                    })
                    .ok_or(PersistenceError::ToolEntityIntegrity)?;
                let tool_key = description
                    .definition
                    .tool_key
                    .as_ref()
                    .ok_or(PersistenceError::ToolEntityIntegrity)?;
                let manifest_tool = evidence
                    .evidence
                    .canonical_manifest
                    .tools
                    .iter()
                    .find(|tool| &tool.tool_key == tool_key)
                    .cloned()
                    .ok_or(PersistenceError::ToolEntityIntegrity)?;
                validate_frozen_instance_inputs(&frozen, &manifest_tool)?;
                if let Some(credential_binding) = &frozen.credential_binding {
                    let (workspace_id, session_id, _) = scope_ids(&corpus.snapshot.scope);
                    if !self.tool_credential_binding_ready_v1(
                        credential_binding,
                        &manifest_tool.runtime.to_runtime_descriptor_v1(),
                        Some(tool_package_id),
                        description.definition.tool_id,
                        workspace_id,
                        session_id,
                    )? {
                        return Err(PersistenceError::ToolEntityStateConflict);
                    }
                }
                let manifest_bytes = canonical_json_jcs(&evidence.evidence.canonical_manifest)
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
                let runtime_root =
                    self.resolve_installed_tool_package_payload(tool_package_id, &package_version)?;
                let installed = verify_installed_tool_package_v1(
                    &runtime_root,
                    &manifest_bytes,
                    &evidence.evidence.canonical_manifest,
                )
                .map_err(|_| PersistenceError::ToolPackagePayloadIntegrity)?;
                if installed.runtime_closure().canonical_hash().ok().as_ref()
                    != Some(&evidence.evidence.runtime_closure_hash)
                    || installed.fingerprint_evidence().package_security_fingerprint()
                        != &version_record.package_security_fingerprint
                    || installed.fingerprint_evidence().package_fingerprint()
                        != &version_record.package_fingerprint
                {
                    return Err(PersistenceError::ToolPackagePayloadIntegrity);
                }
                Some(ExactToolPackageExecutionAuthorityV1 {
                    tool_package_id,
                    package_version,
                    runtime_root,
                    manifest_tool,
                    canonical_manifest_hash: evidence.evidence.canonical_manifest_hash,
                    runtime_closure_hash: evidence.evidence.runtime_closure_hash,
                    package_security_fingerprint: version_record
                        .package_security_fingerprint
                        .clone(),
                })
            }
        };
        Ok(Some(ExactToolExecutionAuthorityV1 {
            registry: corpus.snapshot,
            definition: description.definition,
            instance: description.instance,
            frozen,
            package,
        }))
    }

    fn load_registry_payload_binding(
        &self,
        snapshot_ref: &ToolRegistrySnapshotRef,
        tool_id: aura_domain_common::ToolId,
    ) -> Result<
        Option<(aura_domain_common::ToolPackageId, aura_domain_common::CanonicalSemVer)>,
        PersistenceError,
    > {
        let snapshot_id = snapshot_ref.tool_registry_snapshot_id;
        self.with_reader(move |connection| {
            let row = connection
                .query_row(
                    "SELECT tool_package_id, package_version_json, binding_hash_json
                     FROM tool_registry_package_payload_bindings
                     WHERE tool_registry_snapshot_id=?1 AND tool_id=?2",
                    params![snapshot_id.to_string(), tool_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            let Some((package_id, version_json, hash_json)) = row else {
                return Ok(None);
            };
            let package_id = package_id.parse()?;
            let package_version = decode_json(&version_json)?;
            let actual_hash: CanonicalHashV1 = decode_json(&hash_json)?;
            let expected_hash = canonical_sha256(&PayloadBindingHashInput {
                tool_registry_snapshot_id: snapshot_id,
                tool_id,
                tool_package_id: package_id,
                package_version: &package_version,
            })
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
            if actual_hash != expected_hash {
                return Err(PersistenceError::ToolRegistryIntegrity);
            }
            Ok(Some((package_id, package_version)))
        })
    }

    fn has_live_batch_payload_lease(
        &self,
        tool_batch_id: aura_domain_common::ToolBatchId,
        tool_id: aura_domain_common::ToolId,
        tool_package_id: aura_domain_common::ToolPackageId,
        package_version: &aura_domain_common::CanonicalSemVer,
    ) -> Result<bool, PersistenceError> {
        let package_version = encode_json(package_version)?;
        self.with_reader(move |connection| {
            connection
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM tool_batch_payload_leases lease
                        JOIN tool_calls call ON call.tool_batch_id=lease.tool_batch_id
                        WHERE lease.tool_batch_id=?1 AND call.tool_id=?2
                          AND lease.tool_package_id=?3
                          AND lease.package_version_json=?4
                          AND lease.released_at IS NULL
                    )",
                    params![
                        tool_batch_id.to_string(),
                        tool_id.to_string(),
                        tool_package_id.to_string(),
                        package_version,
                    ],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(PersistenceError::from)
        })
    }

    pub fn list_loaded_tool_definitions_exact(
        &self,
        snapshot_ref: &ToolRegistrySnapshotRef,
    ) -> Result<Option<Vec<ToolDefinitionSnapshotRef>>, PersistenceError> {
        let Some(corpus) = self.read_tool_registry_search_corpus(snapshot_ref)? else {
            return Ok(None);
        };
        Ok(Some(
            corpus
                .snapshot
                .effective_bindings
                .iter()
                .map(|binding| ToolDefinitionSnapshotRef {
                    tool_id: binding.tool_id,
                    tool_version: binding.tool_version.clone(),
                    registry_snapshot_ref: snapshot_ref.clone(),
                })
                .collect(),
        ))
    }

    pub fn load_tools_ephemerally(
        &self,
        request: ToolLoadRequestV1,
        registry_request: ToolRegistryBuildRequestV1,
    ) -> Result<ToolLoadResultV1, PersistenceError> {
        self.load_tools_ephemerally_with_projections(request, registry_request)
            .map(|commit| commit.result)
    }

    pub fn load_tools_ephemerally_with_projections(
        &self,
        request: ToolLoadRequestV1,
        registry_request: ToolRegistryBuildRequestV1,
    ) -> Result<ToolRegistryMutationCommitV1, PersistenceError> {
        self.mutate_ephemeral_tools(request, registry_request, EphemeralMutationV1::Load)
    }

    pub fn unload_tools_ephemerally(
        &self,
        request: ToolLoadRequestV1,
        registry_request: ToolRegistryBuildRequestV1,
    ) -> Result<ToolLoadResultV1, PersistenceError> {
        self.unload_tools_ephemerally_with_projections(request, registry_request)
            .map(|commit| commit.result)
    }

    pub fn unload_tools_ephemerally_with_projections(
        &self,
        request: ToolLoadRequestV1,
        registry_request: ToolRegistryBuildRequestV1,
    ) -> Result<ToolRegistryMutationCommitV1, PersistenceError> {
        self.mutate_ephemeral_tools(request, registry_request, EphemeralMutationV1::Unload)
    }

    fn mutate_ephemeral_tools(
        &self,
        request: ToolLoadRequestV1,
        registry_request: ToolRegistryBuildRequestV1,
        mutation: EphemeralMutationV1,
    ) -> Result<ToolRegistryMutationCommitV1, PersistenceError> {
        let (workspace_id, session_id, turn_task_id) = match &registry_request.scope {
            ToolRegistryScopeV1::Session { workspace_id, session_id, turn_task_id, .. } => {
                (*workspace_id, *session_id, *turn_task_id)
            }
            ToolRegistryScopeV1::Workspace { .. } | ToolRegistryScopeV1::ChildAgent { .. } => {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
        };
        registry_request
            .settings
            .validate()
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
        if request.definitions.is_empty() {
            return Err(PersistenceError::InvalidRequest(
                "Tool load/unload requires at least one exact definition".to_owned(),
            ));
        }
        let source_ref = request.definitions[0].registry_snapshot_ref.clone();
        if request
            .definitions
            .iter()
            .any(|definition| definition.registry_snapshot_ref != source_ref)
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        let mut identities = request
            .definitions
            .iter()
            .map(|definition| (definition.tool_id, definition.tool_version.clone()))
            .collect::<Vec<_>>();
        identities.sort_by(|left, right| {
            left.0.cmp(&right.0).then_with(|| {
                encode_canonical(&left.1)
                    .expect("validated ToolVersion is canonical")
                    .cmp(&encode_canonical(&right.1).expect("validated ToolVersion is canonical"))
            })
        });
        if identities.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(PersistenceError::InvalidRequest(
                "Tool load/unload definitions must be unique".to_owned(),
            ));
        }
        let source = self
            .read_tool_registry_search_corpus(&source_ref)?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
        if source.snapshot.scope != registry_request.scope {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        for definition in &request.definitions {
            source
                .snapshot
                .catalog_entries
                .iter()
                .find(|entry| {
                    entry.tool_id == definition.tool_id
                        && entry.tool_version == definition.tool_version
                })
                .ok_or(PersistenceError::ToolEntityNotFound)?;
        }

        let mut authority =
            self.tool_authority_snapshot(Some(workspace_id), Some(session_id), Some(turn_task_id))?;
        let current_revision = authority.registry_revision;
        let mut changed = false;
        for definition in &request.definitions {
            let source_entry = source
                .snapshot
                .catalog_entries
                .iter()
                .find(|entry| {
                    entry.tool_id == definition.tool_id
                        && entry.tool_version == definition.tool_version
                })
                .ok_or(PersistenceError::ToolEntityNotFound)?;
            let existing = authority
                .ephemeral_loads
                .iter()
                .find(|load| load.tool_id == definition.tool_id)
                .cloned();
            match mutation {
                EphemeralMutationV1::Load => {
                    if source_entry.load_state != ToolLoadStateV1::Discoverable {
                        continue;
                    }
                    if existing
                        .as_ref()
                        .is_some_and(|load| load.tool_version == definition.tool_version)
                    {
                        continue;
                    }
                    authority.ephemeral_loads.retain(|load| load.tool_id != definition.tool_id);
                    authority.ephemeral_loads.push(ToolEphemeralLoadRecordV1 {
                        turn_task_id,
                        session_id,
                        tool_id: definition.tool_id,
                        tool_version: definition.tool_version.clone(),
                        entity_revision: next_entity_revision(
                            existing.as_ref().map(|load| load.entity_revision),
                        )?,
                        loaded_at: registry_request.created_at,
                    });
                    changed = true;
                }
                EphemeralMutationV1::Unload => {
                    if existing
                        .as_ref()
                        .is_some_and(|load| load.tool_version == definition.tool_version)
                    {
                        authority.ephemeral_loads.retain(|load| load.tool_id != definition.tool_id);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            let current =
                self.read_tool_registry_scope_head(&registry_request.scope)?.unwrap_or(source_ref);
            return Ok(ToolRegistryMutationCommitV1 {
                result: ToolLoadResultV1 {
                    changed: false,
                    resulting_registry_snapshot_ref: current,
                },
                projection_changes: Vec::new(),
            });
        }

        authority.registry_revision = next_entity_revision(Some(current_revision))?;
        let version_evidence = self.load_tool_package_version_evidence()?;
        let builtin_authority = self.validated_signed_builtin_release_authority()?;
        let builtin_release_hash = aura_tool_registry::embedded_builtin_tool_definition_set_v1()
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?
            .signed_release_hash()
            .clone();
        let mut candidates = registry_request.non_package_candidates.clone();
        if candidates.iter().any(|candidate| {
            candidate.definition.tool_package_id.is_some()
                || candidate.definition.source_kind == aura_tool_contract::ToolSourceKindV1::BuiltIn
        }) {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        candidates.extend(builtin_authority.iter().map(|entry| RegistryCandidateV1 {
            definition: entry.definition.clone(),
            instance: entry.instance.clone(),
            source_evidence: ToolSourceEligibilityEvidenceV1::VerifiedBuiltinRelease {
                release_manifest_hash: builtin_release_hash.clone(),
                verification_evidence_hash:
                    entry.release_entry.source_contract_fingerprint.fingerprint().clone(),
            },
            active_for_unpinned: true,
        }));
        candidates.extend(self.package_registry_candidates(
            &authority,
            &version_evidence,
            Some(workspace_id),
            Some(session_id),
        )?);
        let builtin_execution_bindings = builtin_authority
            .iter()
            .map(|entry| (entry.instance.tool_instance_id, entry.execution_binding))
            .collect::<BTreeMap<_, _>>();
        let mut core_exposed_tool_ids = registry_request.core_exposed_tool_ids;
        core_exposed_tool_ids
            .extend(builtin_authority.iter().map(|entry| entry.definition.tool_id));
        let output = build_parent_registry_snapshot(ParentRegistryBuildInputV1 {
            registry_revision: authority.registry_revision,
            scope: registry_request.scope.clone(),
            settings: registry_request.settings,
            mode_permission_context_hash: registry_request.mode_permission_context_hash,
            core_exposed_tool_ids,
            candidates: candidates.clone(),
            workspace_pins: authority
                .workspace_pins
                .iter()
                .filter_map(pin_value)
                .cloned()
                .collect(),
            session_pins: authority.session_pins.iter().filter_map(pin_value).cloned().collect(),
            ephemeral_loads: authority.ephemeral_loads.clone(),
            created_at: registry_request.created_at,
        })
        .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
        if mutation == EphemeralMutationV1::Load
            && request.definitions.iter().any(|definition| {
                !output.snapshot.effective_bindings.iter().any(|binding| {
                    binding.tool_id == definition.tool_id
                        && binding.tool_version == definition.tool_version
                })
            })
        {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
        let corpus =
            ToolSearchCorpusV1::new(output.snapshot.clone(), output.search_documents.clone())
                .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
        let projections = registry_projections(
            &output,
            &candidates,
            &authority,
            Some(session_id),
            Some(workspace_id),
            Some(turn_task_id),
        )?;
        let snapshot_ref = output.snapshot.snapshot_ref.clone();
        let scope = registry_request.scope;
        let expected_source_ref = source_ref;
        let updated_loads = authority.ephemeral_loads.clone();
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            if read_registry_revision(&transaction)? != current_revision
                || read_scope_head_in_transaction(&transaction, &scope)?.as_ref()
                    != Some(&expected_source_ref)
            {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            for definition in &request.definitions {
                transaction.execute(
                    "DELETE FROM tool_ephemeral_loads
                     WHERE turn_task_id=?1 AND tool_id=?2",
                    params![turn_task_id.to_string(), definition.tool_id.to_string()],
                )?;
            }
            for load in updated_loads.iter().filter(|load| {
                request.definitions.iter().any(|definition| definition.tool_id == load.tool_id)
            }) {
                let (record_json, record_hash_json) = encode_record(load)?;
                transaction.execute(
                    "INSERT INTO tool_ephemeral_loads(
                        turn_task_id, session_id, tool_id, tool_version_json,
                        entity_revision, record_json, record_hash_json, loaded_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        load.turn_task_id.to_string(),
                        load.session_id.to_string(),
                        load.tool_id.to_string(),
                        encode_canonical(&load.tool_version)?,
                        sqlite_revision(load.entity_revision)?,
                        record_json,
                        record_hash_json,
                        load.loaded_at.to_string(),
                    ],
                )?;
            }
            let advanced = advance_registry_revision(&transaction, registry_request.created_at)?;
            if advanced != snapshot_ref.registry_revision {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            insert_tool_registry_search_corpus(&transaction, &corpus)?;
            insert_registry_definition_records(
                &transaction,
                &corpus.snapshot,
                &candidates,
                &version_evidence,
            )?;
            insert_registry_execution_authority_records(
                &transaction,
                &corpus.snapshot,
                &candidates,
                &authority,
                &builtin_execution_bindings,
            )?;
            write_registry_scope_head(
                &transaction,
                &scope,
                &snapshot_ref,
                registry_request.created_at,
            )?;
            let event_id = EventId::new();
            let mut event = tool_event(
                event_id,
                EventActor::SystemRuntime,
                registry_request.correlation_id,
                ToolLifecycleEventV1::ToolRegistrySnapshotCreated(
                    ToolRegistrySnapshotCreatedPayloadV1 {
                        registry_snapshot_ref: snapshot_ref.clone(),
                        scope: scope.clone(),
                        tool_set_hash: corpus.snapshot.schema_set.tool_set_hash.clone(),
                        binding_set_hash: canonical_sha256(&corpus.snapshot.effective_bindings)
                            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?,
                        created_at: registry_request.created_at,
                    },
                ),
                registry_request.created_at,
            )?;
            event.workspace_id = Some(workspace_id);
            event.session_id = Some(session_id);
            event.turn_task_id = Some(turn_task_id);
            insert_event(&transaction, &event)?;
            let (projection_changes, _) =
                insert_tool_projections(&transaction, runtime_instance_id, projections)?;
            transaction.commit()?;
            Ok(ToolRegistryMutationCommitV1 {
                result: ToolLoadResultV1 {
                    changed: true,
                    resulting_registry_snapshot_ref: snapshot_ref,
                },
                projection_changes,
            })
        })
    }

    pub fn read_tool_registry_scope_head(
        &self,
        scope: &ToolRegistryScopeV1,
    ) -> Result<Option<ToolRegistrySnapshotRef>, PersistenceError> {
        let scope = scope.clone();
        self.with_reader(move |connection| read_scope_head_in_transaction(connection, &scope))
    }

    /// Every scope that currently has a committed Registry head.
    ///
    /// A Registry-affecting change (install, trust, enable, configuration, pin) has
    /// to reach the scopes that are still in use; this is the enumeration, and the
    /// caller decides which of them are still current. Historical or terminated
    /// scopes are never rewritten by that decision — their snapshots stay immutable.
    pub fn list_tool_registry_scope_heads(
        &self,
    ) -> Result<Vec<(ToolRegistryScopeV1, ToolRegistrySnapshotRef)>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT scope_json, tool_registry_snapshot_id, registry_revision,
                        content_hash_json
                 FROM tool_registry_scope_heads
                 ORDER BY scope_hash_json",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(scope_json, snapshot_id, revision, content_hash_json)| {
                    let scope = crate::tool::decode_strict::<ToolRegistryScopeV1>(&scope_json)?;
                    let revision = u64::try_from(revision)
                        .map(Revision::new)
                        .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
                    let snapshot_ref = ToolRegistrySnapshotRef {
                        tool_registry_snapshot_id: snapshot_id.parse()?,
                        registry_revision: revision,
                        content_hash: crate::tool::decode_strict(&content_hash_json)?,
                    };
                    Ok((scope, snapshot_ref))
                })
                .collect()
        })
    }

    /// Whether the scope still describes work in progress: a workspace always does,
    /// a session only while its TurnTask and AgentRun have not reached a terminal
    /// state.
    pub fn tool_registry_scope_is_current(
        &self,
        scope: &ToolRegistryScopeV1,
    ) -> Result<bool, PersistenceError> {
        let (_, _, turn_task_id) = scope_ids(scope);
        let Some(turn_task_id) = turn_task_id else {
            return Ok(true);
        };
        let turn_task_id = turn_task_id.to_string();
        self.with_reader(move |connection| {
            let turn_state = connection
                .query_row(
                    "SELECT state FROM turn_tasks WHERE turn_task_id=?1",
                    [&turn_task_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let Some(turn_state) = turn_state else {
                return Ok(false);
            };
            if turn_task_state_is_terminal(&turn_state)? {
                return Ok(false);
            }
            let (ToolRegistryScopeV1::Session { agent_run_id, .. }
            | ToolRegistryScopeV1::ChildAgent { child_agent_run_id: agent_run_id, .. }) = scope
            else {
                return Ok(true);
            };
            let agent_state = connection
                .query_row(
                    "SELECT state FROM agent_runs WHERE agent_run_id=?1",
                    [agent_run_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let Some(agent_state) = agent_state else {
                return Ok(false);
            };
            Ok(!agent_run_state_is_terminal(&agent_state)?)
        })
    }

    pub fn active_installed_tool_package_manifest(
        &self,
        package_key: &PackageKey,
    ) -> Result<Option<CanonicalToolPackageManifestV1>, PersistenceError> {
        let authority = self.tool_authority_snapshot(None, None, None)?;
        let mut matching = authority.packages.iter().filter(|package| {
            &package.package_key == package_key
                && package.lifecycle == ToolPackageLifecycleV1::Installed
                && !package.deletion_pending
        });
        let Some(package) = matching.next() else {
            return Ok(None);
        };
        if matching.next().is_some() {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let active_version =
            package.active_version.as_ref().ok_or(PersistenceError::ToolEntityIntegrity)?;
        let evidences = self.load_tool_package_version_evidence()?;
        let mut matching = evidences.into_iter().filter(|evidence| {
            evidence.tool_package_id == package.tool_package_id
                && &evidence.package_version == active_version
        });
        let Some(evidence) = matching.next() else {
            return Err(PersistenceError::ToolEntityIntegrity);
        };
        if matching.next().is_some() {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        Ok(Some(evidence.evidence.canonical_manifest))
    }

    fn load_tool_package_version_evidence(
        &self,
    ) -> Result<Vec<LoadedToolPackageVersionEvidenceV1>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT tool_package_id, package_version_json, evidence_json, evidence_hash_json
                 FROM tool_package_version_evidence
                 ORDER BY tool_package_id, package_version_json",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.map(|row| {
                let (package_id, package_version, evidence_json, evidence_hash) = row?;
                Ok(LoadedToolPackageVersionEvidenceV1 {
                    tool_package_id: package_id.parse()?,
                    package_version: crate::tool::decode_strict(&package_version)?,
                    evidence: crate::tool_entities::decode_verified_record(
                        &evidence_json,
                        &evidence_hash,
                    )?,
                })
            })
            .collect()
        })
    }

    fn package_registry_candidates(
        &self,
        authority: &crate::ToolAuthoritySnapshotV1,
        evidences: &[LoadedToolPackageVersionEvidenceV1],
        workspace_id: Option<aura_domain_common::WorkspaceId>,
        session_id: Option<aura_domain_common::SessionId>,
    ) -> Result<Vec<RegistryCandidateV1>, PersistenceError> {
        let configurations = authority
            .instance_configurations
            .iter()
            .map(|record| (record.tool_instance_id, record))
            .collect::<BTreeMap<_, _>>();
        let credential_bindings = authority
            .instance_credential_bindings
            .iter()
            .map(|record| (record.tool_instance_id, record))
            .collect::<BTreeMap<_, _>>();
        let trust = authority
            .trust_records
            .iter()
            .map(|record| (record.tool_package_id, record.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut candidates = Vec::new();
        for definition in &authority.definitions {
            let Some(package_id) = definition.tool_package_id else { continue };
            let package = authority
                .packages
                .iter()
                .find(|package| package.tool_package_id == package_id)
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            let tool_key =
                definition.tool_key.as_ref().ok_or(PersistenceError::ToolEntityIntegrity)?;
            let instance = authority
                .instances
                .iter()
                .find(|instance| {
                    instance.tool_id == definition.tool_id
                        && instance.tool_version == definition.tool_version
                })
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            let (evidence, exact_fingerprint) = evidences
                .iter()
                .filter(|loaded| loaded.tool_package_id == package_id)
                .filter_map(|evidence| {
                    evidence
                        .evidence
                        .tool_contract_fingerprints
                        .iter()
                        .find(|entry| {
                            &entry.tool_key == tool_key
                                && ToolSourceContractFingerprintV1::package(
                                    definition.source_kind,
                                    entry.fingerprint.clone(),
                                )
                                .ok()
                                .as_ref()
                                    == Some(&instance.source_contract_fingerprint)
                        })
                        .map(|entry| (evidence, entry))
                })
                .next()
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            let version = authority
                .package_versions
                .iter()
                .find(|version| {
                    version.tool_package_id == package_id
                        && version.package_version == evidence.package_version
                })
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            let manifest_tool = evidence
                .evidence
                .canonical_manifest
                .tools
                .iter()
                .find(|tool| &tool.tool_key == tool_key)
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            let expected_source = ToolSourceContractFingerprintV1::package(
                definition.source_kind,
                exact_fingerprint.fingerprint.clone(),
            )
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            if instance.source_contract_fingerprint != expected_source {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            let canonical_manifest_bytes =
                canonical_json_jcs(&evidence.evidence.canonical_manifest)
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            let runtime_root =
                self.resolve_installed_tool_package_payload(package_id, &version.package_version);
            let installed = runtime_root.as_ref().ok().and_then(|runtime_root| {
                verify_installed_tool_package_v1(
                    runtime_root,
                    &canonical_manifest_bytes,
                    &evidence.evidence.canonical_manifest,
                )
                .ok()
            });
            let payload_verified = installed.as_ref().is_some_and(|installed| {
                installed.runtime_closure().canonical_hash().ok().as_ref()
                    == Some(&evidence.evidence.runtime_closure_hash)
                    && installed.fingerprint_evidence().package_fingerprint()
                        == &version.package_fingerprint
                    && installed.fingerprint_evidence().package_security_fingerprint()
                        == &version.package_security_fingerprint
                    && installed.fingerprint_evidence().tool_contract_fingerprints().iter().any(
                        |entry| {
                            entry.tool_key() == tool_key
                                && entry.fingerprint() == &exact_fingerprint.fingerprint
                        },
                    )
            });
            let instance_configuration_ready = match &manifest_tool.instance_configuration.0 {
                None => true,
                Some(schema) => {
                    configurations.get(&instance.tool_instance_id).is_some_and(|record| {
                        &record.instance_config_schema_hash == schema.schema_hash()
                            && CompiledToolSchemaV1::compile(schema.canonical_document())
                                .ok()
                                .is_some_and(|validator| {
                                    validator.validate(&record.configuration).valid
                                })
                    })
                }
            };
            let credential_binding_ready = if !runtime_requires_credential(&manifest_tool.runtime) {
                true
            } else if let Some(record) = credential_bindings.get(&instance.tool_instance_id) {
                self.tool_credential_binding_ready_v1(
                    record,
                    &manifest_tool.runtime.to_runtime_descriptor_v1(),
                    Some(package_id),
                    definition.tool_id,
                    workspace_id,
                    session_id,
                )?
            } else {
                false
            };
            let conformance_passed = !conformance_evidence_missing(version)
                && version.conformance_run_ids == evidence.evidence.conformance_run_ids;
            candidates.push(RegistryCandidateV1 {
                definition: definition.clone(),
                instance: instance.clone(),
                source_evidence: ToolSourceEligibilityEvidenceV1::Package {
                    current_security_fingerprint: version.package_security_fingerprint.clone(),
                    trust_record: trust.get(&package_id).cloned(),
                    payload_verified,
                    conformance_passed,
                    instance_configuration_ready,
                    credential_binding_ready,
                    deletion_pending: package.deletion_pending,
                },
                active_for_unpinned: package.lifecycle == ToolPackageLifecycleV1::Installed
                    && !package.deletion_pending
                    && package.active_version.as_ref() == Some(&version.package_version),
            });
        }
        Ok(candidates)
    }

    pub(crate) fn instance_configuration_schema(
        &self,
        tool_instance_id: ToolInstanceId,
    ) -> Result<Option<aura_tool_contract::ToolInputSchemaV1>, PersistenceError> {
        self.instance_manifest_tool(tool_instance_id).map(|tool| tool.instance_configuration.0)
    }

    fn instance_manifest_tool(
        &self,
        tool_instance_id: ToolInstanceId,
    ) -> Result<aura_tool_domain::CanonicalToolPackageToolV1, PersistenceError> {
        let authority = self.tool_authority_snapshot(None, None, None)?;
        let instance = authority
            .instances
            .iter()
            .find(|instance| instance.tool_instance_id == tool_instance_id)
            .ok_or(PersistenceError::ToolEntityNotFound)?;
        let definition = authority
            .definitions
            .iter()
            .find(|definition| {
                definition.tool_id == instance.tool_id
                    && definition.tool_version == instance.tool_version
            })
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        let package_id =
            definition.tool_package_id.ok_or(PersistenceError::ToolEntityStateConflict)?;
        let package = authority
            .packages
            .iter()
            .find(|package| package.tool_package_id == package_id)
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        if package.lifecycle != ToolPackageLifecycleV1::Installed || package.deletion_pending {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
        let tool_key = definition.tool_key.as_ref().ok_or(PersistenceError::ToolEntityIntegrity)?;
        let evidences = self.load_tool_package_version_evidence()?;
        let evidence = evidences
            .into_iter()
            .find(|loaded| {
                loaded.tool_package_id == package_id
                    && loaded.evidence.tool_contract_fingerprints.iter().any(|entry| {
                        &entry.tool_key == tool_key
                            && ToolSourceContractFingerprintV1::package(
                                definition.source_kind,
                                entry.fingerprint.clone(),
                            )
                            .ok()
                            .as_ref()
                                == Some(&instance.source_contract_fingerprint)
                    })
            })
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        evidence
            .evidence
            .canonical_manifest
            .tools
            .into_iter()
            .find(|tool| &tool.tool_key == tool_key)
            .ok_or(PersistenceError::ToolEntityIntegrity)
    }
}

/// TurnTask states that mean the TurnTask is over. The column stores the plain
/// snake_case state name, and a state this build does not know is treated as *not*
/// terminal so a refresh is never skipped for a scope that may still be in use.
fn turn_task_state_is_terminal(state: &str) -> Result<bool, PersistenceError> {
    Ok(matches!(state, "completed" | "failed" | "cancelled" | "recovery_required"))
}

/// AgentRun states that mean the run is over. `recovery_required` is terminal here
/// (the run does not continue by itself) and the reason is reported elsewhere.
fn agent_run_state_is_terminal(state: &str) -> Result<bool, PersistenceError> {
    Ok(matches!(state, "completed" | "cancelled" | "recovery_required"))
}

fn read_scope_head_in_transaction(
    connection: &rusqlite::Connection,
    scope: &ToolRegistryScopeV1,
) -> Result<Option<ToolRegistrySnapshotRef>, PersistenceError> {
    let scope_hash =
        canonical_sha256(scope).map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    let row = connection
        .query_row(
            "SELECT scope_json, tool_registry_snapshot_id, registry_revision,
                    content_hash_json
             FROM tool_registry_scope_heads WHERE scope_hash_json=?1",
            [encode_json(&scope_hash)?],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((scope_json, snapshot_id, revision, content_hash_json)) = row else {
        return Ok(None);
    };
    if crate::tool::decode_strict::<ToolRegistryScopeV1>(&scope_json)? != *scope {
        return Err(PersistenceError::ToolRegistryIntegrity);
    }
    let revision = u64::try_from(revision)
        .map(Revision::new)
        .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    let snapshot_ref = ToolRegistrySnapshotRef {
        tool_registry_snapshot_id: snapshot_id.parse()?,
        registry_revision: revision,
        content_hash: crate::tool::decode_strict(&content_hash_json)?,
    };
    Ok(Some(snapshot_ref))
}

fn write_registry_scope_head(
    transaction: &Transaction<'_>,
    scope: &ToolRegistryScopeV1,
    snapshot_ref: &ToolRegistrySnapshotRef,
    updated_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let scope_hash =
        canonical_sha256(scope).map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
    transaction.execute(
        "INSERT INTO tool_registry_scope_heads(
            scope_hash_json, scope_json, tool_registry_snapshot_id,
            registry_revision, content_hash_json, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(scope_hash_json) DO UPDATE SET
            scope_json=excluded.scope_json,
            tool_registry_snapshot_id=excluded.tool_registry_snapshot_id,
            registry_revision=excluded.registry_revision,
            content_hash_json=excluded.content_hash_json,
            updated_at=excluded.updated_at",
        params![
            encode_json(&scope_hash)?,
            encode_canonical(scope)?,
            snapshot_ref.tool_registry_snapshot_id.to_string(),
            sqlite_revision(snapshot_ref.registry_revision)?,
            encode_json(&snapshot_ref.content_hash)?,
            updated_at.to_string(),
        ],
    )?;
    Ok(())
}

fn insert_registry_definition_records(
    transaction: &Transaction<'_>,
    snapshot: &aura_tool_contract::ToolRegistrySnapshotV1,
    candidates: &[RegistryCandidateV1],
    package_evidences: &[LoadedToolPackageVersionEvidenceV1],
) -> Result<(), PersistenceError> {
    for entry in &snapshot.catalog_entries {
        let matching = candidates
            .iter()
            .filter(|candidate| {
                candidate.definition.tool_id == entry.tool_id
                    && candidate.definition.tool_version == entry.tool_version
                    && candidate.instance.tool_instance_id == entry.tool_instance_id
            })
            .collect::<Vec<_>>();
        let [candidate] = matching.as_slice() else {
            return Err(PersistenceError::ToolRegistryIntegrity);
        };
        if candidate.definition.definition_fingerprint != entry.definition_fingerprint
            || candidate.instance.source_contract_fingerprint != entry.source_contract_fingerprint
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        let (definition_json, definition_hash_json) =
            crate::tool_entities::encode_record(&candidate.definition)?;
        let (instance_json, instance_hash_json) =
            crate::tool_entities::encode_record(&candidate.instance)?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO tool_registry_definition_records(
                tool_registry_snapshot_id, tool_id, tool_version_json,
                definition_json, definition_hash_json, instance_json, instance_hash_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                snapshot.snapshot_ref.tool_registry_snapshot_id.to_string(),
                entry.tool_id.to_string(),
                encode_canonical(&entry.tool_version)?,
                definition_json,
                definition_hash_json,
                instance_json,
                instance_hash_json,
            ],
        )?;
        if inserted != 1 {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        if let Some(tool_package_id) = candidate.definition.tool_package_id {
            let tool_key = candidate
                .definition
                .tool_key
                .as_ref()
                .ok_or(PersistenceError::ToolRegistryIntegrity)?;
            let matches = package_evidences
                .iter()
                .filter(|evidence| evidence.tool_package_id == tool_package_id)
                .filter(|evidence| {
                    evidence.evidence.tool_contract_fingerprints.iter().any(|fingerprint| {
                        &fingerprint.tool_key == tool_key
                            && ToolSourceContractFingerprintV1::package(
                                candidate.definition.source_kind,
                                fingerprint.fingerprint.clone(),
                            )
                            .ok()
                            .as_ref()
                                == Some(&candidate.instance.source_contract_fingerprint)
                    })
                })
                .collect::<Vec<_>>();
            let [evidence] = matches.as_slice() else {
                return Err(PersistenceError::ToolRegistryIntegrity);
            };
            let binding_hash = canonical_sha256(&PayloadBindingHashInput {
                tool_registry_snapshot_id: snapshot.snapshot_ref.tool_registry_snapshot_id,
                tool_id: entry.tool_id,
                tool_package_id,
                package_version: &evidence.package_version,
            })
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
            let inserted = transaction.execute(
                "INSERT INTO tool_registry_package_payload_bindings(
                    tool_registry_snapshot_id, tool_id, tool_package_id,
                    package_version_json, binding_hash_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    snapshot.snapshot_ref.tool_registry_snapshot_id.to_string(),
                    entry.tool_id.to_string(),
                    tool_package_id.to_string(),
                    encode_canonical(&evidence.package_version)?,
                    encode_json(&binding_hash)?,
                ],
            )?;
            if inserted != 1 {
                return Err(PersistenceError::ToolRegistryIntegrity);
            }
        }
    }
    Ok(())
}

fn insert_registry_execution_authority_records(
    transaction: &Transaction<'_>,
    snapshot: &aura_tool_contract::ToolRegistrySnapshotV1,
    candidates: &[RegistryCandidateV1],
    authority: &crate::ToolAuthoritySnapshotV1,
    builtin_execution_bindings: &BTreeMap<ToolInstanceId, ToolInstanceExecutionBindingV1>,
) -> Result<(), PersistenceError> {
    let configurations = authority
        .instance_configurations
        .iter()
        .map(|record| (record.tool_instance_id, record.clone()))
        .collect::<BTreeMap<_, _>>();
    let credential_bindings = authority
        .instance_credential_bindings
        .iter()
        .map(|record| (record.tool_instance_id, record.clone()))
        .collect::<BTreeMap<_, _>>();
    for entry in &snapshot.catalog_entries {
        let matching = candidates
            .iter()
            .filter(|candidate| {
                candidate.definition.tool_id == entry.tool_id
                    && candidate.definition.tool_version == entry.tool_version
                    && candidate.instance.tool_instance_id == entry.tool_instance_id
            })
            .collect::<Vec<_>>();
        let [candidate] = matching.as_slice() else {
            return Err(PersistenceError::ToolRegistryIntegrity);
        };
        let execution_binding = builtin_execution_bindings.get(&entry.tool_instance_id).copied();
        if (candidate.definition.source_kind == aura_tool_contract::ToolSourceKindV1::BuiltIn)
            != execution_binding.is_some()
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        let frozen = ToolRegistryExecutionAuthorityRecordV1 {
            registry_snapshot_ref: snapshot.snapshot_ref.clone(),
            tool_id: entry.tool_id,
            tool_version: entry.tool_version.clone(),
            tool_instance_id: entry.tool_instance_id,
            instance_entity_revision: candidate.instance.entity_revision,
            instance_configuration: configurations.get(&entry.tool_instance_id).cloned(),
            credential_binding: credential_bindings.get(&entry.tool_instance_id).cloned(),
            execution_binding,
        };
        let (authority_json, authority_hash_json) = encode_record(&frozen)?;
        let inserted = transaction.execute(
            "INSERT INTO tool_registry_execution_authority_records(
                tool_registry_snapshot_id, tool_id, authority_json, authority_hash_json
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                snapshot.snapshot_ref.tool_registry_snapshot_id.to_string(),
                entry.tool_id.to_string(),
                authority_json,
                authority_hash_json,
            ],
        )?;
        if inserted != 1 {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
    }
    Ok(())
}

fn validate_optional_revision(
    current: Option<Revision>,
    expected: Option<Revision>,
) -> Result<(), PersistenceError> {
    match (current, expected) {
        (None, None) => Ok(()),
        (Some(current), Some(expected)) if current == expected => Ok(()),
        _ => Err(PersistenceError::ToolEntityRevisionConflict),
    }
}

fn ensure_instance_mutable(
    transaction: &Transaction<'_>,
    tool_instance_id: ToolInstanceId,
) -> Result<(), PersistenceError> {
    let eligible = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM tool_instances i
            JOIN tool_definitions d
              ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json
            JOIN tool_packages p ON p.tool_package_id=d.tool_package_id
            WHERE i.tool_instance_id=?1 AND p.lifecycle='installed'
              AND p.deletion_pending=0
         )",
        [tool_instance_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    eligible.then_some(()).ok_or(PersistenceError::ToolEntityStateConflict)
}

fn runtime_requires_credential(runtime: &CanonicalToolPackageRuntimeV1) -> bool {
    match runtime {
        CanonicalToolPackageRuntimeV1::DeclarativeCommand { command } => match command {
            aura_tool_domain::CanonicalToolPackageDeclarativeCommandV1::Argv {
                credential_slot,
                ..
            }
            | aura_tool_domain::CanonicalToolPackageDeclarativeCommandV1::PowershellScript {
                credential_slot,
                ..
            } => credential_slot.0.is_some(),
        },
        CanonicalToolPackageRuntimeV1::AuraStdio { credential_slot, .. } => {
            credential_slot.0.is_some()
        }
        CanonicalToolPackageRuntimeV1::DeclarativeHttp { request } => {
            request.credential_slot.0.is_some()
        }
    }
}

fn validate_frozen_instance_inputs(
    frozen: &ToolRegistryExecutionAuthorityRecordV1,
    tool: &aura_tool_domain::CanonicalToolPackageToolV1,
) -> Result<(), PersistenceError> {
    match (&tool.instance_configuration.0, &frozen.instance_configuration) {
        (None, None) => {}
        (None, Some(_)) | (Some(_), None) => {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }
        (Some(schema), Some(record)) => {
            if record.instance_config_schema_hash != *schema.schema_hash()
                || !CompiledToolSchemaV1::compile(schema.canonical_document())
                    .map_err(|_| PersistenceError::ToolRegistryIntegrity)?
                    .validate(&record.configuration)
                    .valid
            {
                return Err(PersistenceError::ToolRegistryIntegrity);
            }
        }
    }
    if runtime_requires_credential(&tool.runtime) != frozen.credential_binding.is_some() {
        return Err(PersistenceError::ToolRegistryIntegrity);
    }
    Ok(())
}

fn conformance_evidence_missing(version: &aura_tool_domain::ToolPackageVersionRecordV1) -> bool {
    version.conformance_run_ids.is_empty()
}

fn pin_value(record: &aura_tool_domain::ToolPinRecordV1) -> Option<&aura_tool_contract::ToolPinV1> {
    match record {
        aura_tool_domain::ToolPinRecordV1::Workspace { pin, .. }
        | aura_tool_domain::ToolPinRecordV1::Session { pin, .. } => Some(pin),
    }
}

/// The identities a registry scope resolves to in the authority.
///
/// Shared with the live-enablement consumption port so the scope →
/// identity mapping exists **once**. Two copies could drift, and a drifted copy
/// would read a different authority than the dispatch path uses.
pub(crate) fn scope_ids(
    scope: &ToolRegistryScopeV1,
) -> (
    Option<aura_domain_common::WorkspaceId>,
    Option<aura_domain_common::SessionId>,
    Option<aura_domain_common::TurnTaskId>,
) {
    match scope {
        ToolRegistryScopeV1::Workspace { workspace_id } => (Some(*workspace_id), None, None),
        ToolRegistryScopeV1::Session { workspace_id, session_id, turn_task_id, .. }
        | ToolRegistryScopeV1::ChildAgent { workspace_id, session_id, turn_task_id, .. } => {
            (Some(*workspace_id), Some(*session_id), Some(*turn_task_id))
        }
    }
}

fn registry_projections(
    output: &ParentRegistryBuildOutputV1,
    candidates: &[RegistryCandidateV1],
    authority: &crate::ToolAuthoritySnapshotV1,
    session_id: Option<aura_domain_common::SessionId>,
    workspace_id: Option<aura_domain_common::WorkspaceId>,
    turn_task_id: Option<aura_domain_common::TurnTaskId>,
) -> Result<Vec<ToolProjectionDraft>, PersistenceError> {
    if let (Some(session_id), Some(turn_task_id)) = (session_id, turn_task_id) {
        let tools = output
            .snapshot
            .effective_bindings
            .iter()
            .map(|binding| {
                let entry = output
                    .snapshot
                    .catalog_entries
                    .iter()
                    .find(|entry| entry.tool_id == binding.tool_id)
                    .ok_or(PersistenceError::ToolRegistryIntegrity)?;
                let candidate = candidates
                    .iter()
                    .find(|candidate| {
                        candidate.definition.tool_id == binding.tool_id
                            && candidate.definition.tool_version == binding.tool_version
                    })
                    .ok_or(PersistenceError::ToolRegistryIntegrity)?;
                Ok(EffectiveToolView {
                    tool_id: binding.tool_id,
                    tool_version: binding.tool_version.clone(),
                    tool_instance_id: binding.tool_instance_id,
                    canonical_name: entry.canonical_name.clone(),
                    display_name: entry.display_name.clone(),
                    summary: entry.summary.clone(),
                    source: entry.source,
                    exposure: binding.exposure,
                    audience: candidate.definition.audience,
                    enabled: candidate.instance.enabled,
                    // Every emitted binding has already passed its source's
                    // exact trust evidence in the Registry Builder.
                    trusted: true,
                    runtime_health: candidate.instance.runtime_health,
                    pin_state: authority
                        .session_pins
                        .iter()
                        .chain(authority.workspace_pins.iter())
                        .filter_map(pin_value)
                        .find(|pin| pin.tool_id == binding.tool_id)
                        .map(|pin| pin.state),
                    eligible: true,
                    safe_ineligibility_reason: None,
                })
            })
            .collect::<Result<Vec<_>, PersistenceError>>()?;
        return Ok(vec![
            ToolProjectionDraft::SessionEffectiveToolRegistry(
                SessionEffectiveToolRegistryProjection {
                    session_id,
                    registry_snapshot_ref: output.snapshot.snapshot_ref.clone(),
                    tools: tools.clone(),
                },
            ),
            ToolProjectionDraft::SessionLoadedTools(SessionLoadedToolsProjection {
                session_id,
                turn_task_id,
                registry_snapshot_ref: output.snapshot.snapshot_ref.clone(),
                tools,
            }),
            ToolProjectionDraft::SessionToolPins(SessionToolPinsProjectionV1 {
                session_id,
                pin_set_revision: Revision::INITIAL,
                pins: session_pin_projection_entries(authority, output),
            }),
        ]);
    }
    Ok(workspace_id
        .map(|workspace_id| {
            vec![ToolProjectionDraft::WorkspaceToolRegistrySummary(
                WorkspaceToolRegistrySummaryProjection {
                    workspace_id,
                    registry_snapshot_ref: Some(output.snapshot.snapshot_ref.clone()),
                    discoverable_tool_count: u32::try_from(
                        output
                            .snapshot
                            .catalog_entries
                            .iter()
                            .filter(|entry| {
                                entry.load_state
                                    == aura_tool_contract::ToolLoadStateV1::Discoverable
                            })
                            .count(),
                    )
                    .unwrap_or(u32::MAX),
                    exposed_tool_count: u32::try_from(output.snapshot.effective_bindings.len())
                        .unwrap_or(u32::MAX),
                    unresolved_pin_count: u32::try_from(
                        output
                            .exclusions
                            .iter()
                            .filter(|exclusion| {
                                matches!(
                            exclusion.reason,
                            aura_tool_registry::RegistryExclusionReasonV1::PinUnresolved
                                | aura_tool_registry::RegistryExclusionReasonV1::PinDegraded
                        )
                            })
                            .count(),
                    )
                    .unwrap_or(u32::MAX),
                    capacity_remediation_required: false,
                    safe_issue: None,
                },
            )]
        })
        .unwrap_or_default())
}

fn session_pin_projection_entries(
    authority: &crate::ToolAuthoritySnapshotV1,
    output: &ParentRegistryBuildOutputV1,
) -> Vec<ToolPinProjectionEntryV1> {
    let mut pins = authority
        .session_pins
        .iter()
        .filter_map(pin_value)
        .cloned()
        .map(|pin| {
            let mut entry = ToolPinProjectionEntryV1::from(pin);
            entry.resolved_binding = output
                .snapshot
                .effective_bindings
                .iter()
                .find(|binding| {
                    binding.tool_id == entry.tool_id && binding.tool_version == entry.tool_version
                })
                .cloned();
            entry
        })
        .collect::<Vec<_>>();
    pins.sort_by_key(|pin| pin.tool_id);
    pins
}
