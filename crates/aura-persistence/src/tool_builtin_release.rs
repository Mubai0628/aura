mod http_credentials;
pub(crate) use http_credentials::{
    clear_rebind_reason, credential_slot_binding_summary,
    credential_slot_binding_summary_for_runtime,
};
use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{CorrelationId, EventId, Revision, UtcTimestamp};
use aura_domain_common::{ToolId, ToolInstanceId};
use aura_projection_contract::ProjectionChange;
use aura_tool_contract::{ToolRuntimeDescriptorV1, ToolSchemaSnapshotV1, ToolSourceKindV1};
use aura_tool_domain::{
    CanonicalToolPackageToolV1, ToolDefinitionRecordV1, ToolInstanceConfigurationRecordV1,
    ToolInstanceCredentialBindingRecordV1, ToolInstanceRecordV1, ToolPackageLifecycleV1,
    ToolPackageRecordV1, ToolPackageTrustRecordV1, ToolPackageVersionRecordV1, ToolRuntimeHealthV1,
};
use aura_tool_projection::{
    SignedBuiltinReleaseHashV1, ToolBoundCredentialSummaryV1, ToolCredentialSlotBindingSummaryV1,
    ToolDefinitionInstanceView, ToolDefinitionProjection, ToolDefinitionSelectionCandidateV1,
    ToolDefinitionSourceSelectionRefV1, ToolDefinitionVersionView,
    ToolInstanceConfigurationProjectionV1, resolve_tool_definition_current_selection_v1,
};
use aura_tool_registry::{
    SignedBuiltinReleaseEntryV1, ToolInstanceExecutionBindingV1,
    embedded_builtin_tool_definition_set_v1,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::json;

use crate::{
    EventActor, PersistenceCoordinator, PersistenceError, PersistentEventDraft,
    coordinator::insert_event,
    tool_entities::{
        advance_registry_revision, decode_json, decode_verified_record, encode_json, encode_record,
        insert_definition, insert_instance, load_all_records, read_registry_revision,
        sqlite_revision,
    },
    tool_projection::{ToolProjectionDraft, insert_tool_projections},
};

const SIGNED_BUILTIN_RELEASE_EVENT_V1: &str = "SignedBuiltinReleaseMaterialized";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedBuiltinReleaseAuthorityEntryV1 {
    pub release_entry: SignedBuiltinReleaseEntryV1,
    pub definition: ToolDefinitionRecordV1,
    pub instance: ToolInstanceRecordV1,
    pub execution_binding: ToolInstanceExecutionBindingV1,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignedBuiltinReleaseMaterializationCommitV1 {
    pub changed: bool,
    pub release_revision: Revision,
    pub signed_release_hash: CanonicalHashV1,
    pub registry_revision: Revision,
    pub event_id: Option<EventId>,
    pub eligible_instance_ids: Vec<ToolInstanceId>,
    pub quarantined_instance_ids: Vec<ToolInstanceId>,
    pub projection_changes: Vec<ProjectionChange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryMutationPlanV1 {
    Insert,
    Upgrade,
    RefreshReleaseBinding,
}

#[derive(Clone)]
struct SignedBuiltinReleaseMaterializationSourceV1 {
    release_revision: Revision,
    signed_release_hash: CanonicalHashV1,
    source_hash: String,
    entries: Vec<SignedBuiltinReleaseEntryV1>,
}

enum StoredRecordV1<T> {
    Missing,
    Valid(T),
    Corrupted,
}

struct StoredExecutionBindingV1 {
    binding: ToolInstanceExecutionBindingV1,
    binding_revision: Revision,
    signed_release_hash: CanonicalHashV1,
}

impl PersistenceCoordinator {
    /// Idempotently materializes the embedded signed Built-in release into
    /// real Tool Definition/Instance records plus an internal Core-handler
    /// binding. Exact restart performs no write; identity or fingerprint
    /// conflicts quarantine only the affected fixed Instance ID.
    pub fn materialize_signed_builtin_release(
        &self,
        materialized_at: UtcTimestamp,
        correlation_id: CorrelationId,
    ) -> Result<SignedBuiltinReleaseMaterializationCommitV1, PersistenceError> {
        let release = embedded_builtin_tool_definition_set_v1()
            .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))?;
        let mut commit = self.materialize_signed_builtin_release_source(
            SignedBuiltinReleaseMaterializationSourceV1 {
                release_revision: release.release_revision(),
                signed_release_hash: release.signed_release_hash().clone(),
                source_hash: release.source_hash().hex.clone(),
                entries: release.entries().to_vec(),
            },
            materialized_at,
            correlation_id,
        )?;
        if let Some((revision, projections)) =
            self.materialize_http_credential_target(materialized_at)?
        {
            commit.changed = true;
            commit.registry_revision = revision;
            commit.projection_changes.extend(projections);
        }
        Ok(commit)
    }

    fn materialize_signed_builtin_release_source(
        &self,
        source: SignedBuiltinReleaseMaterializationSourceV1,
        materialized_at: UtcTimestamp,
        correlation_id: CorrelationId,
    ) -> Result<SignedBuiltinReleaseMaterializationCommitV1, PersistenceError> {
        let SignedBuiltinReleaseMaterializationSourceV1 {
            release_revision,
            signed_release_hash,
            source_hash,
            entries,
        } = source;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let state = load_release_state(&transaction)?;
            if matches!(state, StoredReleaseStateV1::Corrupted) {
                return Ok(SignedBuiltinReleaseMaterializationCommitV1 {
                    changed: false,
                    release_revision,
                    signed_release_hash,
                    registry_revision: read_registry_revision(&transaction)?,
                    event_id: None,
                    eligible_instance_ids: Vec::new(),
                    quarantined_instance_ids: entries
                        .iter()
                        .map(|entry| entry.tool_instance_id)
                        .collect(),
                    projection_changes: Vec::new(),
                });
            }
            let same_release = matches!(
                &state,
                StoredReleaseStateV1::Valid {
                    release_revision: stored_revision,
                    signed_release_hash: stored_hash,
                    source_hash: stored_source_hash,
                } if *stored_revision == release_revision
                    && *stored_hash == signed_release_hash
                    && stored_source_hash == &source_hash
            );
            if same_release {
                let (eligible_instance_ids, quarantined_instance_ids) =
                    validate_release_entries(&transaction, &entries, &signed_release_hash)?;
                return Ok(SignedBuiltinReleaseMaterializationCommitV1 {
                    changed: false,
                    release_revision,
                    signed_release_hash,
                    registry_revision: read_registry_revision(&transaction)?,
                    event_id: None,
                    eligible_instance_ids,
                    quarantined_instance_ids,
                    projection_changes: Vec::new(),
                });
            }
            let previous_release_hash = match &state {
                StoredReleaseStateV1::Missing => None,
                StoredReleaseStateV1::Valid {
                    release_revision: stored_revision,
                    signed_release_hash: stored_hash,
                    ..
                } if *stored_revision < release_revision => Some(stored_hash.clone()),
                StoredReleaseStateV1::Valid { .. } | StoredReleaseStateV1::Corrupted => {
                    return Ok(SignedBuiltinReleaseMaterializationCommitV1 {
                        changed: false,
                        release_revision,
                        signed_release_hash,
                        registry_revision: read_registry_revision(&transaction)?,
                        event_id: None,
                        eligible_instance_ids: Vec::new(),
                        quarantined_instance_ids: entries
                            .iter()
                            .map(|entry| entry.tool_instance_id)
                            .collect(),
                        projection_changes: Vec::new(),
                    });
                }
            };

            let mut applicable = Vec::new();
            let mut quarantined_instance_ids = Vec::new();
            for entry in &entries {
                match plan_entry_mutation(
                    &transaction,
                    entry,
                    &signed_release_hash,
                    previous_release_hash.as_ref(),
                )? {
                    Some(plan) => applicable.push((entry.clone(), plan)),
                    None => quarantined_instance_ids.push(entry.tool_instance_id),
                }
            }
            for (entry, plan) in &applicable {
                apply_entry_mutation(
                    &transaction,
                    entry,
                    *plan,
                    &signed_release_hash,
                    previous_release_hash.as_ref(),
                    materialized_at,
                )?;
            }

            let registry_revision = advance_registry_revision(&transaction, materialized_at)?;
            transaction.execute(
                "INSERT INTO signed_builtin_release_state(
                    singleton, release_revision, signed_release_hash_json, source_hash,
                    registry_revision, materialized_at
                 ) VALUES (1, ?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(singleton) DO UPDATE SET
                    release_revision=excluded.release_revision,
                    signed_release_hash_json=excluded.signed_release_hash_json,
                    source_hash=excluded.source_hash,
                    registry_revision=excluded.registry_revision,
                    materialized_at=excluded.materialized_at",
                params![
                    sqlite_revision(release_revision)?,
                    encode_json(&signed_release_hash)?,
                    source_hash,
                    sqlite_revision(registry_revision)?,
                    materialized_at.to_string(),
                ],
            )?;
            let (eligible_instance_ids, validation_quarantined) =
                validate_release_entries(&transaction, &entries, &signed_release_hash)?;
            for instance_id in validation_quarantined {
                if !quarantined_instance_ids.contains(&instance_id) {
                    quarantined_instance_ids.push(instance_id);
                }
            }
            quarantined_instance_ids.sort();
            let event_id = EventId::new();
            let event = PersistentEventDraft {
                event_id,
                workspace_id: None,
                session_id: None,
                turn_task_id: None,
                agent_run_id: None,
                event_type: SIGNED_BUILTIN_RELEASE_EVENT_V1.to_owned(),
                event_version: 1,
                actor: EventActor::SystemRuntime,
                correlation_id,
                causation_id: None,
                atomic_group_id: None,
                payload_json: Some(json!({
                    "release_revision": release_revision,
                    "signed_release_hash": signed_release_hash,
                    "source_hash": source_hash,
                    "registry_revision": registry_revision,
                    "eligible_instance_ids": eligible_instance_ids,
                    "quarantined_instance_ids": quarantined_instance_ids,
                    "materialized_at": materialized_at,
                })),
                payload_artifact_ref: None,
                searchable_text: None,
                created_at: materialized_at,
            };
            insert_event(&transaction, &event)?;
            let projection_drafts = tool_definition_projection_drafts(&transaction, None)?;
            let (projection_changes, _) =
                insert_tool_projections(&transaction, runtime_instance_id, projection_drafts)?;
            transaction.commit()?;
            Ok(SignedBuiltinReleaseMaterializationCommitV1 {
                changed: true,
                release_revision,
                signed_release_hash,
                registry_revision,
                event_id: Some(event_id),
                eligible_instance_ids,
                quarantined_instance_ids,
                projection_changes,
            })
        })
    }

    /// Returns only Built-ins whose persisted Definition, stable Instance,
    /// source fingerprint, release hash, and unique closed handler all match.
    pub fn validated_signed_builtin_release_authority(
        &self,
    ) -> Result<Vec<SignedBuiltinReleaseAuthorityEntryV1>, PersistenceError> {
        let release = embedded_builtin_tool_definition_set_v1()
            .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))?;
        let release_hash = release.signed_release_hash().clone();
        let release_revision = release.release_revision();
        let source_hash = release.source_hash().hex.clone();
        let entries = release.entries().to_vec();
        self.with_reader(move |connection| {
            if !matches!(
                load_release_state(connection)?,
                StoredReleaseStateV1::Valid {
                    release_revision: stored_revision,
                    signed_release_hash: stored_hash,
                    source_hash: stored_source_hash,
                } if stored_revision == release_revision
                    && stored_hash == release_hash
                    && stored_source_hash == source_hash
            ) {
                return Ok(Vec::new());
            }
            entries
                .iter()
                .filter_map(|entry| match load_validated_entry(connection, entry, &release_hash) {
                    Ok(Some(authority)) => Some(Ok(authority)),
                    Ok(None) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect()
        })
    }
}

enum StoredReleaseStateV1 {
    Missing,
    Valid { release_revision: Revision, signed_release_hash: CanonicalHashV1, source_hash: String },
    Corrupted,
}

fn load_release_state(
    connection: &rusqlite::Connection,
) -> Result<StoredReleaseStateV1, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT release_revision, signed_release_hash_json, source_hash
             FROM signed_builtin_release_state WHERE singleton=1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        )
        .optional()?;
    let Some((revision, hash, source_hash)) = row else {
        return Ok(StoredReleaseStateV1::Missing);
    };
    let Ok(revision) = u64::try_from(revision) else {
        return Ok(StoredReleaseStateV1::Corrupted);
    };
    let Ok(signed_release_hash) = decode_json::<CanonicalHashV1>(&hash) else {
        return Ok(StoredReleaseStateV1::Corrupted);
    };
    if revision == 0
        || source_hash.len() != 64
        || !source_hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Ok(StoredReleaseStateV1::Corrupted);
    }
    Ok(StoredReleaseStateV1::Valid {
        release_revision: Revision::new(revision),
        signed_release_hash,
        source_hash,
    })
}

fn plan_entry_mutation(
    transaction: &Transaction<'_>,
    entry: &SignedBuiltinReleaseEntryV1,
    signed_release_hash: &CanonicalHashV1,
    previous_release_hash: Option<&CanonicalHashV1>,
) -> Result<Option<EntryMutationPlanV1>, PersistenceError> {
    let definition = load_definition(transaction, entry)?;
    let instance = load_instance(transaction, entry.tool_instance_id)?;
    let binding = load_execution_binding(transaction, entry.tool_instance_id)?;
    if matches!(definition, StoredRecordV1::Corrupted)
        || matches!(instance, StoredRecordV1::Corrupted)
        || matches!(binding, StoredRecordV1::Corrupted)
    {
        return Ok(None);
    }
    if matches!(definition, StoredRecordV1::Missing)
        && matches!(instance, StoredRecordV1::Missing)
        && matches!(binding, StoredRecordV1::Missing)
    {
        return Ok(Some(EntryMutationPlanV1::Insert));
    }
    let Some(previous_release_hash) = previous_release_hash else {
        // A transactionally materialized release cannot leave partial records
        // without its release authority row. Never adopt or repair such state.
        return Ok(None);
    };
    let definition_missing = matches!(definition, StoredRecordV1::Missing);
    if let StoredRecordV1::Valid(definition) = &definition {
        let expected = entry
            .definition_record(definition.created_at)
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if definition != &expected {
            return Ok(None);
        }
    }
    match &instance {
        StoredRecordV1::Missing => return Ok(None),
        StoredRecordV1::Valid(instance) => {
            if instance.source_kind != ToolSourceKindV1::BuiltIn
                || instance.tool_id != entry.definition.tool_id
            {
                return Ok(None);
            }
            if instance.tool_version != entry.definition.tool_version {
                if !definition_missing
                    || entry.definition.tool_version.definition_revision
                        <= instance.tool_version.definition_revision
                    || entry.tool_instance_revision <= instance.entity_revision
                {
                    return Ok(None);
                }
                return match binding {
                    StoredRecordV1::Valid(binding)
                        if binding.signed_release_hash == *previous_release_hash
                            && entry.tool_instance_revision > binding.binding_revision =>
                    {
                        Ok(Some(EntryMutationPlanV1::Upgrade))
                    }
                    StoredRecordV1::Missing
                    | StoredRecordV1::Valid(_)
                    | StoredRecordV1::Corrupted => Ok(None),
                };
            }
            if !instance_matches_release(instance, entry) {
                return Ok(None);
            }
        }
        StoredRecordV1::Corrupted => return Ok(None),
    }
    if definition_missing {
        return Ok(None);
    }
    match binding {
        StoredRecordV1::Valid(binding)
            if binding.binding_revision == entry.tool_instance_revision
                && binding.signed_release_hash == *previous_release_hash
                && binding.binding == entry.execution_binding =>
        {
            debug_assert_ne!(binding.signed_release_hash, *signed_release_hash);
            Ok(Some(EntryMutationPlanV1::RefreshReleaseBinding))
        }
        StoredRecordV1::Missing | StoredRecordV1::Valid(_) | StoredRecordV1::Corrupted => Ok(None),
    }
}

fn apply_entry_mutation(
    transaction: &Transaction<'_>,
    entry: &SignedBuiltinReleaseEntryV1,
    plan: EntryMutationPlanV1,
    signed_release_hash: &CanonicalHashV1,
    previous_release_hash: Option<&CanonicalHashV1>,
    materialized_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    if plan != EntryMutationPlanV1::RefreshReleaseBinding
        && matches!(load_definition(transaction, entry)?, StoredRecordV1::Missing)
    {
        let definition = entry
            .definition_record(materialized_at)
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        insert_definition(transaction, &definition)?;
    }
    match plan {
        EntryMutationPlanV1::Insert => {
            insert_instance(transaction, &entry.instance_record(materialized_at))?;
        }
        EntryMutationPlanV1::Upgrade => {
            let current = match load_instance(transaction, entry.tool_instance_id)? {
                StoredRecordV1::Valid(instance) => instance,
                StoredRecordV1::Missing => return Err(PersistenceError::ToolEntityIntegrity),
                StoredRecordV1::Corrupted => return Err(PersistenceError::ToolEntityIntegrity),
            };
            let mut instance = entry.instance_record(current.created_at);
            instance.updated_at = materialized_at;
            let (record_json, record_hash_json) = encode_record(&instance)?;
            let changed = transaction.execute(
                "UPDATE tool_instances SET
                    tool_id=?1, tool_version_json=?2, source_kind='built_in', enabled=1,
                    runtime_health='healthy', entity_revision=?3, record_json=?4,
                    record_hash_json=?5, updated_at=?6
                 WHERE tool_instance_id=?7 AND entity_revision=?8",
                params![
                    instance.tool_id.to_string(),
                    encode_json(&instance.tool_version)?,
                    sqlite_revision(instance.entity_revision)?,
                    record_json,
                    record_hash_json,
                    materialized_at.to_string(),
                    instance.tool_instance_id.to_string(),
                    sqlite_revision(current.entity_revision)?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
        }
        EntryMutationPlanV1::RefreshReleaseBinding => {
            return refresh_execution_binding_release_hash(
                transaction,
                entry,
                previous_release_hash.ok_or(PersistenceError::ToolEntityIntegrity)?,
                signed_release_hash,
                materialized_at,
            );
        }
    }
    match plan {
        EntryMutationPlanV1::Insert => {
            insert_execution_binding(transaction, entry, signed_release_hash, materialized_at)
        }
        EntryMutationPlanV1::Upgrade => update_execution_binding_for_upgrade(
            transaction,
            entry,
            previous_release_hash.ok_or(PersistenceError::ToolEntityIntegrity)?,
            signed_release_hash,
            materialized_at,
        ),
        EntryMutationPlanV1::RefreshReleaseBinding => unreachable!("returned above"),
    }
}

fn insert_execution_binding(
    transaction: &Transaction<'_>,
    entry: &SignedBuiltinReleaseEntryV1,
    signed_release_hash: &CanonicalHashV1,
    updated_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let (binding_json, binding_hash_json) = encode_record(&entry.execution_binding)?;
    transaction.execute(
        "INSERT INTO tool_instance_execution_bindings(
            tool_instance_id, binding_kind, binding_revision, signed_release_hash_json,
            binding_json, binding_hash_json, updated_at
         ) VALUES (?1, 'builtin_core_handler', ?2, ?3, ?4, ?5, ?6)",
        params![
            entry.tool_instance_id.to_string(),
            sqlite_revision(entry.tool_instance_revision)?,
            encode_json(signed_release_hash)?,
            binding_json,
            binding_hash_json,
            updated_at.to_string(),
        ],
    )?;
    Ok(())
}

fn update_execution_binding_for_upgrade(
    transaction: &Transaction<'_>,
    entry: &SignedBuiltinReleaseEntryV1,
    previous_release_hash: &CanonicalHashV1,
    signed_release_hash: &CanonicalHashV1,
    updated_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let (binding_json, binding_hash_json) = encode_record(&entry.execution_binding)?;
    let changed = transaction.execute(
        "UPDATE tool_instance_execution_bindings SET
            binding_kind='builtin_core_handler', binding_revision=?1,
            signed_release_hash_json=?2, binding_json=?3, binding_hash_json=?4,
            updated_at=?5
         WHERE tool_instance_id=?6 AND binding_revision < ?1
           AND signed_release_hash_json=?7",
        params![
            sqlite_revision(entry.tool_instance_revision)?,
            encode_json(signed_release_hash)?,
            binding_json,
            binding_hash_json,
            updated_at.to_string(),
            entry.tool_instance_id.to_string(),
            encode_json(previous_release_hash)?,
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn refresh_execution_binding_release_hash(
    transaction: &Transaction<'_>,
    entry: &SignedBuiltinReleaseEntryV1,
    previous_release_hash: &CanonicalHashV1,
    signed_release_hash: &CanonicalHashV1,
    updated_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let (binding_json, binding_hash_json) = encode_record(&entry.execution_binding)?;
    let changed = transaction.execute(
        "UPDATE tool_instance_execution_bindings SET
            signed_release_hash_json=?1, binding_json=?2, binding_hash_json=?3,
            updated_at=?4
         WHERE tool_instance_id=?5 AND binding_revision=?6
           AND signed_release_hash_json=?7",
        params![
            encode_json(signed_release_hash)?,
            binding_json,
            binding_hash_json,
            updated_at.to_string(),
            entry.tool_instance_id.to_string(),
            sqlite_revision(entry.tool_instance_revision)?,
            encode_json(previous_release_hash)?,
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn validate_release_entries(
    connection: &rusqlite::Connection,
    entries: &[SignedBuiltinReleaseEntryV1],
    signed_release_hash: &CanonicalHashV1,
) -> Result<(Vec<ToolInstanceId>, Vec<ToolInstanceId>), PersistenceError> {
    let mut eligible = Vec::new();
    let mut quarantined = Vec::new();
    for entry in entries {
        if load_validated_entry(connection, entry, signed_release_hash)?.is_some() {
            eligible.push(entry.tool_instance_id);
        } else {
            quarantined.push(entry.tool_instance_id);
        }
    }
    Ok((eligible, quarantined))
}

fn load_validated_entry(
    connection: &rusqlite::Connection,
    entry: &SignedBuiltinReleaseEntryV1,
    signed_release_hash: &CanonicalHashV1,
) -> Result<Option<SignedBuiltinReleaseAuthorityEntryV1>, PersistenceError> {
    let StoredRecordV1::Valid(definition) = load_definition(connection, entry)? else {
        return Ok(None);
    };
    let StoredRecordV1::Valid(instance) = load_instance(connection, entry.tool_instance_id)? else {
        return Ok(None);
    };
    let StoredRecordV1::Valid(binding) =
        load_execution_binding(connection, entry.tool_instance_id)?
    else {
        return Ok(None);
    };
    let expected_definition = entry
        .definition_record(definition.created_at)
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if definition != expected_definition
        || !instance_matches_release(&instance, entry)
        || binding.binding_revision != entry.tool_instance_revision
        || binding.signed_release_hash != *signed_release_hash
        || binding.binding != entry.execution_binding
    {
        return Ok(None);
    }
    Ok(Some(SignedBuiltinReleaseAuthorityEntryV1 {
        release_entry: entry.clone(),
        definition,
        instance,
        execution_binding: binding.binding,
    }))
}

fn instance_matches_release(
    instance: &ToolInstanceRecordV1,
    entry: &SignedBuiltinReleaseEntryV1,
) -> bool {
    instance.tool_instance_id == entry.tool_instance_id
        && instance.tool_id == entry.definition.tool_id
        && instance.tool_version == entry.definition.tool_version
        && instance.source_kind == ToolSourceKindV1::BuiltIn
        && instance.source_contract_fingerprint
            == entry.instance_record(instance.created_at).source_contract_fingerprint
        && instance.enabled
        && instance.runtime_health == ToolRuntimeHealthV1::Healthy
        && instance.overrides == entry.instance_record(instance.created_at).overrides
        && instance.entity_revision == entry.tool_instance_revision
}

fn load_definition(
    connection: &rusqlite::Connection,
    entry: &SignedBuiltinReleaseEntryV1,
) -> Result<StoredRecordV1<ToolDefinitionRecordV1>, PersistenceError> {
    load_record(
        connection,
        "SELECT record_json, record_hash_json FROM tool_definitions
         WHERE tool_id=?1 AND tool_version_json=?2",
        params![entry.definition.tool_id.to_string(), encode_json(&entry.definition.tool_version)?,],
    )
}

fn load_instance(
    connection: &rusqlite::Connection,
    tool_instance_id: ToolInstanceId,
) -> Result<StoredRecordV1<ToolInstanceRecordV1>, PersistenceError> {
    load_record(
        connection,
        "SELECT record_json, record_hash_json FROM tool_instances WHERE tool_instance_id=?1",
        [tool_instance_id.to_string()],
    )
}

fn load_record<T, P>(
    connection: &rusqlite::Connection,
    sql: &str,
    parameters: P,
) -> Result<StoredRecordV1<T>, PersistenceError>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
    P: rusqlite::Params,
{
    let row = connection
        .query_row(sql, parameters, |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .optional()?;
    let Some((record_json, record_hash_json)) = row else {
        return Ok(StoredRecordV1::Missing);
    };
    Ok(match decode_verified_record::<T>(&record_json, &record_hash_json) {
        Ok(record) => StoredRecordV1::Valid(record),
        Err(_) => StoredRecordV1::Corrupted,
    })
}

fn load_execution_binding(
    connection: &rusqlite::Connection,
    tool_instance_id: ToolInstanceId,
) -> Result<StoredRecordV1<StoredExecutionBindingV1>, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT binding_kind, binding_revision, signed_release_hash_json,
                    binding_json, binding_hash_json
             FROM tool_instance_execution_bindings WHERE tool_instance_id=?1",
            [tool_instance_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((kind, revision, release_hash, binding_json, binding_hash_json)) = row else {
        return Ok(StoredRecordV1::Missing);
    };
    if kind != "builtin_core_handler" {
        return Ok(StoredRecordV1::Corrupted);
    }
    let Ok(revision) = u64::try_from(revision) else {
        return Ok(StoredRecordV1::Corrupted);
    };
    if revision == 0 {
        return Ok(StoredRecordV1::Corrupted);
    }
    let Ok(signed_release_hash) = decode_json::<CanonicalHashV1>(&release_hash) else {
        return Ok(StoredRecordV1::Corrupted);
    };
    let Ok(binding) =
        decode_verified_record::<ToolInstanceExecutionBindingV1>(&binding_json, &binding_hash_json)
    else {
        return Ok(StoredRecordV1::Corrupted);
    };
    Ok(StoredRecordV1::Valid(StoredExecutionBindingV1 {
        binding,
        binding_revision: Revision::new(revision),
        signed_release_hash,
    }))
}

pub(crate) fn tool_definition_projection_drafts(
    connection: &rusqlite::Connection,
    only_tool_ids: Option<&BTreeSet<ToolId>>,
) -> Result<Vec<ToolProjectionDraft>, PersistenceError> {
    let definitions = load_all_records::<ToolDefinitionRecordV1>(
        connection,
        "SELECT record_json, record_hash_json FROM tool_definitions
         ORDER BY tool_id, definition_revision, tool_version_json",
    )?;
    let instances = load_all_records::<ToolInstanceRecordV1>(
        connection,
        "SELECT record_json, record_hash_json FROM tool_instances
         ORDER BY tool_id, tool_version_json, tool_instance_id",
    )?;
    let configurations = load_all_records::<ToolInstanceConfigurationRecordV1>(
        connection,
        "SELECT record_json, record_hash_json FROM tool_instance_configurations
         ORDER BY tool_instance_id",
    )?
    .into_iter()
    .map(|record| (record.tool_instance_id, record))
    .collect::<BTreeMap<_, _>>();
    let credential_bindings = load_all_records::<ToolInstanceCredentialBindingRecordV1>(
        connection,
        "SELECT record_json, record_hash_json FROM tool_instance_credential_bindings
         ORDER BY tool_instance_id",
    )?
    .into_iter()
    .map(|record| (record.tool_instance_id, record))
    .collect::<BTreeMap<_, _>>();
    let packages = load_all_records::<ToolPackageRecordV1>(
        connection,
        "SELECT record_json, record_hash_json FROM tool_packages ORDER BY tool_package_id",
    )?
    .into_iter()
    .map(|package| (package.tool_package_id, package))
    .collect::<BTreeMap<_, _>>();
    let package_versions = load_all_records::<ToolPackageVersionRecordV1>(
        connection,
        "SELECT record_json, record_hash_json FROM tool_package_versions
         ORDER BY tool_package_id, package_version_json",
    )?;
    let trust = load_all_records::<ToolPackageTrustRecordV1>(
        connection,
        "SELECT record_json, record_hash_json FROM tool_package_trust ORDER BY tool_package_id",
    )?
    .into_iter()
    .map(|record| (record.tool_package_id, record))
    .collect::<BTreeMap<_, _>>();
    let release = embedded_builtin_tool_definition_set_v1()
        .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))?;
    let release_is_current = matches!(
        load_release_state(connection)?,
        StoredReleaseStateV1::Valid {
            release_revision,
            signed_release_hash,
            source_hash,
        } if release_revision == release.release_revision()
            && signed_release_hash == *release.signed_release_hash()
            && source_hash == release.source_hash().hex
    );
    let mut valid_builtins = BTreeMap::new();
    if release_is_current {
        for entry in release.entries() {
            if let Some(authority) =
                load_validated_entry(connection, entry, release.signed_release_hash())?
            {
                valid_builtins.insert(authority.instance.tool_instance_id, authority);
            }
        }
    }

    let mut by_tool = BTreeMap::<ToolId, Vec<ToolDefinitionRecordV1>>::new();
    for definition in definitions {
        if only_tool_ids.is_none_or(|tool_ids| tool_ids.contains(&definition.tool_id)) {
            by_tool.entry(definition.tool_id).or_default().push(definition);
        }
    }
    by_tool
        .into_iter()
        .map(|(tool_id, mut definitions)| {
            definitions.sort_by(|left, right| {
                left.tool_version
                    .definition_revision
                    .cmp(&right.tool_version.definition_revision)
                    .then_with(|| {
                        left.tool_version.semantic_version.as_ref().map(ToString::to_string).cmp(
                            &right.tool_version.semantic_version.as_ref().map(ToString::to_string),
                        )
                    })
            });
            let canonical_name = definitions[0].canonical_name.clone();
            if definitions.iter().any(|definition| definition.canonical_name != canonical_name) {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            let display_name = definitions
                .last()
                .map(|definition| definition.display_name.clone())
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            let mut selection_candidates = Vec::new();
            let mut entity_revision = Revision::INITIAL;
            let mut version_views = Vec::with_capacity(definitions.len());
            for definition in definitions {
                entity_revision = entity_revision.max(definition.entity_revision);
                let matching_instances = instances
                    .iter()
                    .filter(|instance| {
                        instance.tool_id == definition.tool_id
                            && instance.tool_version == definition.tool_version
                    })
                    .collect::<Vec<_>>();
                let package_facts = package_selection_facts(
                    connection,
                    &definition,
                    &packages,
                    &package_versions,
                    &trust,
                )?;
                let manifest_tool = package_manifest_tool(connection, &definition)?;
                let mut instance_views = Vec::with_capacity(matching_instances.len());
                for instance in matching_instances {
                    entity_revision = entity_revision.max(instance.entity_revision);
                    let builtin_authority = valid_builtins
                        .get(&instance.tool_instance_id)
                        .filter(|authority| authority.definition == definition);
                    let (trusted, payload_available, source_selection_ref) =
                        if let Some(authority) = builtin_authority {
                            (
                                true,
                                true,
                                Some(ToolDefinitionSourceSelectionRefV1::SignedBuiltinRelease {
                                    tool_id: definition.tool_id,
                                    tool_version: definition.tool_version.clone(),
                                    tool_instance_id: instance.tool_instance_id,
                                    tool_instance_revision: instance.entity_revision,
                                    signed_builtin_release_hash: SignedBuiltinReleaseHashV1::new(
                                        release.signed_release_hash().clone(),
                                    ),
                                    definition_fingerprint: definition
                                        .definition_fingerprint
                                        .clone(),
                                    source_contract_fingerprint: authority
                                        .release_entry
                                        .source_contract_fingerprint
                                        .clone(),
                                }),
                            )
                        } else if let Some(package) = &package_facts {
                            (
                                package.trusted,
                                package.payload_available,
                                Some(ToolDefinitionSourceSelectionRefV1::ActivePackageVersion {
                                    tool_package_id: package.package.tool_package_id,
                                    package_version: package.version.package_version.clone(),
                                    package_revision: package.package.entity_revision,
                                    tool_id: definition.tool_id,
                                    tool_version: definition.tool_version.clone(),
                                    tool_instance_id: instance.tool_instance_id,
                                    tool_instance_revision: instance.entity_revision,
                                    definition_fingerprint: definition
                                        .definition_fingerprint
                                        .clone(),
                                    package_fingerprint: package
                                        .version
                                        .package_fingerprint
                                        .clone(),
                                    package_security_fingerprint: package
                                        .version
                                        .package_security_fingerprint
                                        .clone(),
                                }),
                            )
                        } else {
                            (false, false, None)
                        };
                    let ready = trusted
                        && payload_available
                        && instance.enabled
                        && instance.runtime_health == ToolRuntimeHealthV1::Healthy
                        && source_selection_ref.is_some();
                    instance_views.push(ToolDefinitionInstanceView {
                        tool_instance_id: instance.tool_instance_id,
                        tool_version: instance.tool_version.clone(),
                        enabled: instance.enabled,
                        trusted,
                        runtime_health: instance.runtime_health,
                        ready,
                        entity_revision: instance.entity_revision,
                        configuration: configurations.get(&instance.tool_instance_id).map(
                            |record| ToolInstanceConfigurationProjectionV1 {
                                safe_non_secret_value: record.configuration.clone(),
                                configuration_revision: record.configuration_revision,
                                configuration_hash: record.configuration_hash.clone(),
                            },
                        ),
                        credential_slot_binding: http_credentials::projected_binding(
                            connection,
                            instance.tool_instance_id,
                            match manifest_tool.as_ref() {
                                Some(tool) => credential_slot_binding_summary(
                                    tool,
                                    credential_bindings.get(&instance.tool_instance_id),
                                ),
                                None => credential_slot_binding_summary_for_runtime(
                                    &definition.runtime,
                                    credential_bindings.get(&instance.tool_instance_id),
                                ),
                            },
                        )?,
                    });
                    if let Some(source_selection_ref) = source_selection_ref {
                        selection_candidates.push(ToolDefinitionSelectionCandidateV1 {
                            source_selection_ref,
                            ready,
                            explicitly_selected: false,
                        });
                    }
                }
                instance_views.sort_by_key(|instance| instance.tool_instance_id);
                let schema = ToolSchemaSnapshotV1::new(
                    definition.tool_id,
                    definition.tool_version.clone(),
                    definition.canonical_name.clone(),
                    definition.model_description.clone(),
                    definition.input_schema.clone(),
                    definition.strict,
                )
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
                let payload_available = if definition.source_kind == ToolSourceKindV1::BuiltIn {
                    valid_builtins.values().any(|authority| authority.definition == definition)
                } else {
                    package_facts.as_ref().is_some_and(|facts| facts.payload_available)
                };
                version_views.push(ToolDefinitionVersionView {
                    tool_version: definition.tool_version,
                    model_description: definition.model_description,
                    model_summary: definition.model_summary,
                    schema,
                    result_contract: definition.result_contract,
                    source: definition.source_kind,
                    audience: definition.audience,
                    execution_semantics: definition.execution_semantics,
                    instance_configuration_schema: manifest_tool
                        .as_ref()
                        .map(|tool| tool.instance_configuration.clone())
                        .unwrap_or(
                            aura_tool_domain::CanonicalToolPackageInstanceConfigurationSchemaV1(
                                None,
                            ),
                        ),
                    capability_count: u32::try_from(definition.capabilities.len())
                        .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                    resource_claim_count: u32::try_from(definition.resource_claims.len())
                        .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                    payload_available,
                    instances: instance_views,
                });
            }
            Ok(ToolProjectionDraft::ToolDefinition(Box::new(ToolDefinitionProjection {
                tool_id,
                canonical_name,
                display_name,
                versions: version_views,
                current_selection: resolve_tool_definition_current_selection_v1(
                    &selection_candidates,
                ),
                entity_revision,
                safe_error: None,
            })))
        })
        .collect()
}

pub(crate) fn package_manifest_tool(
    connection: &rusqlite::Connection,
    definition: &ToolDefinitionRecordV1,
) -> Result<Option<CanonicalToolPackageToolV1>, PersistenceError> {
    crate::tool_package_reads::membership::manifest_tool(connection, definition)
}

struct PackageSelectionFactsV1<'a> {
    package: &'a ToolPackageRecordV1,
    version: &'a ToolPackageVersionRecordV1,
    trusted: bool,
    payload_available: bool,
}

fn package_selection_facts<'a>(
    connection: &rusqlite::Connection,
    definition: &ToolDefinitionRecordV1,
    packages: &'a BTreeMap<aura_domain_common::ToolPackageId, ToolPackageRecordV1>,
    versions: &'a [ToolPackageVersionRecordV1],
    trust: &BTreeMap<aura_domain_common::ToolPackageId, ToolPackageTrustRecordV1>,
) -> Result<Option<PackageSelectionFactsV1<'a>>, PersistenceError> {
    let Some(tool_package_id) = definition.tool_package_id else {
        return Ok(None);
    };
    let Some(package) = packages.get(&tool_package_id) else {
        return Ok(None);
    };
    if package.lifecycle != ToolPackageLifecycleV1::Installed || package.deletion_pending {
        return Ok(None);
    }
    let Some(active_version) = package.active_version.as_ref() else {
        return Ok(None);
    };
    if let Some(membership) = crate::tool_package_reads::membership::published_membership(
        connection,
        tool_package_id,
        active_version,
    )? && !membership.contains(definition)
    {
        return Ok(None);
    }
    let Some(version) = versions.iter().find(|version| {
        version.tool_package_id == tool_package_id && &version.package_version == active_version
    }) else {
        return Ok(None);
    };
    let payload_available = connection
        .query_row(
            "SELECT state FROM tool_package_payloads
             WHERE tool_package_id=?1 AND package_version_json=?2",
            params![tool_package_id.to_string(), encode_json(&version.package_version)?,],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .as_deref()
        == Some("published");
    let trusted = trust.get(&tool_package_id).is_some_and(|record| {
        record.trusted
            && record.package_security_fingerprint == version.package_security_fingerprint
    });
    Ok(Some(PackageSelectionFactsV1 { package, version, trusted, payload_available }))
}

#[cfg(test)]
mod tests {
    use aura_domain_common::jcs::canonical_sha256;
    use aura_domain_common::{ContentHash, RuntimeInstanceId};
    use aura_tool_contract::{SignedBuiltinToolContractFingerprintV1, ToolVersion};
    use aura_tool_registry::ToolInstanceExecutionBindingV1;
    use rusqlite::Connection;
    use serde::Serialize;
    use tempfile::TempDir;

    use super::*;
    use crate::{PersistenceOptions, PersistenceStartup};

    #[derive(Serialize)]
    #[serde(deny_unknown_fields)]
    struct SyntheticReleaseWireV1<'a> {
        schema_version: u32,
        release_revision: Revision,
        entries: &'a [SignedBuiltinReleaseEntryV1],
    }

    #[derive(Serialize)]
    #[serde(deny_unknown_fields)]
    struct SyntheticSourceContractFingerprintMaterialV1<'a> {
        tool_id: ToolId,
        tool_version: &'a ToolVersion,
        tool_instance_id: ToolInstanceId,
        tool_instance_revision: Revision,
        definition_fingerprint: &'a CanonicalHashV1,
        execution_binding: ToolInstanceExecutionBindingV1,
    }

    fn start(root: &TempDir) -> PersistenceCoordinator {
        match PersistenceCoordinator::start(PersistenceOptions::new(
            root.path().join("aura.db"),
            root.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("start synthetic Built-in release persistence")
        {
            PersistenceStartup::Ready(value) => value,
            PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected synthetic Built-in recovery: {}", value.reason)
            }
        }
    }

    fn synthetic_upgrade_source() -> SignedBuiltinReleaseMaterializationSourceV1 {
        let current = embedded_builtin_tool_definition_set_v1().expect("embedded release");
        let mut entries = current.entries().to_vec();
        let upgraded = entries
            .iter_mut()
            .find(|entry| {
                entry.execution_binding.builtin_handler()
                    == aura_tool_registry::BuiltinCoreHandlerV1::ToolsSearch
            })
            .expect("tools.search release entry");
        let definition_revision = Revision::new(2);
        let tool_version = ToolVersion::new(None, definition_revision).expect("revision two");
        let created_at: UtcTimestamp = "2026-09-06T00:00:00Z".parse().expect("timestamp");
        let current_record = upgraded.definition_record(created_at).expect("current record");
        let upgraded_summary = format!("{} (synthetic revision two)", current_record.model_summary);
        let upgraded_record = ToolDefinitionRecordV1::new(
            current_record.tool_id,
            tool_version.clone(),
            current_record.tool_package_id,
            current_record.package_key,
            current_record.tool_key,
            current_record.canonical_name,
            current_record.display_name,
            current_record.model_description,
            upgraded_summary.clone(),
            current_record.input_schema,
            current_record.strict,
            current_record.result_contract,
            current_record.runtime,
            current_record.capabilities,
            current_record.resource_claims,
            current_record.audience,
            current_record.source_kind,
            current_record.execution_semantics,
            current_record.runtime_file_closure_hash,
            definition_revision,
            created_at,
        )
        .expect("synthetic upgraded definition");
        upgraded.definition.tool_version = tool_version;
        upgraded.definition.model_summary = upgraded_summary;
        upgraded.tool_instance_revision = definition_revision;
        upgraded.definition_fingerprint = upgraded_record.definition_fingerprint;
        let source_fingerprint = canonical_sha256(&SyntheticSourceContractFingerprintMaterialV1 {
            tool_id: upgraded.definition.tool_id,
            tool_version: &upgraded.definition.tool_version,
            tool_instance_id: upgraded.tool_instance_id,
            tool_instance_revision: upgraded.tool_instance_revision,
            definition_fingerprint: &upgraded.definition_fingerprint,
            execution_binding: upgraded.execution_binding,
        })
        .expect("synthetic source fingerprint");
        upgraded.source_contract_fingerprint =
            SignedBuiltinToolContractFingerprintV1::new(source_fingerprint);
        upgraded.definition_record(created_at).expect("upgraded entry is internally consistent");

        let release_revision = Revision::new(2);
        let wire =
            SyntheticReleaseWireV1 { schema_version: 1, release_revision, entries: &entries };
        let signed_release_hash = canonical_sha256(&wire).expect("synthetic release hash");
        let source_bytes = serde_json::to_vec_pretty(&wire).expect("synthetic release source");
        SignedBuiltinReleaseMaterializationSourceV1 {
            release_revision,
            signed_release_hash,
            source_hash: ContentHash::sha256(&source_bytes).hex,
            entries,
        }
    }

    #[test]
    fn genuine_release_upgrade_preserves_fixed_identity_and_old_definition() {
        let root = tempfile::tempdir().expect("synthetic release root");
        let persistence = start(&root);
        let initial = persistence
            .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
            .expect("materialize initial release");
        assert!(initial.changed);
        let initial_search_instance = persistence
            .validated_signed_builtin_release_authority()
            .expect("read original signed authority")
            .into_iter()
            .find(|entry| {
                entry.execution_binding.builtin_handler()
                    == aura_tool_registry::BuiltinCoreHandlerV1::ToolsSearch
            })
            .expect("original search handler is eligible")
            .instance
            .tool_instance_id;

        let source = synthetic_upgrade_source();
        let upgraded_entry = source
            .entries
            .iter()
            .find(|entry| {
                entry.execution_binding.builtin_handler()
                    == aura_tool_registry::BuiltinCoreHandlerV1::ToolsSearch
            })
            .expect("upgraded entry")
            .clone();
        let upgraded_hash_json = encode_json(&source.signed_release_hash).expect("release hash");
        let upgraded = persistence
            .materialize_signed_builtin_release_source(
                source.clone(),
                UtcTimestamp::now(),
                CorrelationId::new(),
            )
            .expect("materialize upgraded release");
        assert!(upgraded.changed);
        assert_eq!(upgraded.release_revision, Revision::new(2));
        assert_eq!(upgraded.eligible_instance_ids, initial.eligible_instance_ids);
        assert!(upgraded.quarantined_instance_ids.is_empty());
        assert_eq!(upgraded_entry.tool_instance_id, initial_search_instance);

        let connection = Connection::open(root.path().join("aura.db")).expect("open database");
        let (instance_json, instance_hash): (String, String) = connection
            .query_row(
                "SELECT record_json, record_hash_json FROM tool_instances
                 WHERE tool_instance_id=?1",
                [upgraded_entry.tool_instance_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read upgraded fixed instance");
        let instance =
            decode_verified_record::<ToolInstanceRecordV1>(&instance_json, &instance_hash)
                .expect("verify upgraded instance");
        assert_eq!(instance.tool_instance_id, upgraded_entry.tool_instance_id);
        assert_eq!(instance.tool_version, upgraded_entry.definition.tool_version);
        assert_eq!(instance.entity_revision, Revision::new(2));
        let retained_definitions: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tool_definitions WHERE tool_id=?1",
                [upgraded_entry.definition.tool_id.to_string()],
                |row| row.get(0),
            )
            .expect("count retained definitions");
        assert_eq!(retained_definitions, 2, "the old exact version remains readable");
        let refreshed_bindings: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tool_instance_execution_bindings
                 WHERE signed_release_hash_json=?1",
                [upgraded_hash_json],
                |row| row.get(0),
            )
            .expect("count release-bound handlers");
        assert_eq!(
            refreshed_bindings,
            i64::try_from(source.entries.len()).expect("bounded signed release"),
            "every unchanged handler binds the new release hash"
        );
        drop(connection);

        let downgrade = persistence
            .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
            .expect("reject embedded downgrade");
        assert!(!downgrade.changed);
        assert!(downgrade.eligible_instance_ids.is_empty());
        assert_eq!(downgrade.quarantined_instance_ids, initial.eligible_instance_ids);
        assert!(downgrade.event_id.is_none());
        assert!(downgrade.projection_changes.is_empty());
    }
}
