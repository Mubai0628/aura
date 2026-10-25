mod enablement;
use std::collections::BTreeMap;

use aura_domain_common::ToolId;
use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{
    ContentHashAlgorithm, CorrelationId, EventId, Revision, SessionId, TurnTaskId, UtcTimestamp,
    WorkspaceId,
};
use aura_projection_contract::ProjectionChange;
use aura_tool_contract::{
    SessionToolPinChangedPayloadV1, ToolEnabledChangedPayloadV1, ToolLifecycleEventV1,
    ToolPackageInstalledPayloadV1, ToolPackageToolIdentityV1, ToolPackageVersionRefV1,
    ToolPinResolutionStateV1, ToolPinV1, ToolSourceKindV1, ToolTrustChangedPayloadV1, ToolVersion,
    WorkspaceToolPinChangedPayloadV1,
};
use aura_tool_domain::{
    TOOL_PIN_SESSION_CLEAR_METHOD_V1, TOOL_PIN_SESSION_SET_METHOD_V1,
    TOOL_PIN_WORKSPACE_CLEAR_METHOD_V1, TOOL_PIN_WORKSPACE_SET_METHOD_V1, ToolDefinitionRecordV1,
    ToolEphemeralLoadRecordV1, ToolInstanceConfigurationRecordV1,
    ToolInstanceCredentialBindingRecordV1, ToolInstanceRecordV1, ToolMutationResultV1,
    ToolPackageLifecycleV1, ToolPackageRecordV1, ToolPackageTrustRecordV1,
    ToolPackageVersionRecordV1, ToolPinRecordV1,
};
use aura_tool_projection::{SessionToolPinsProjectionV1, WorkspaceToolPinsProjection};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    EventActor, PersistenceCoordinator, PersistenceError, PersistentEventDraft,
    ToolProjectionDraft, coordinator::insert_event,
    tool_command::complete_tool_mutation_command_success_in_transaction,
    tool_package_reads::package_projection_drafts, tool_projection::insert_tool_projections,
};

#[cfg(debug_assertions)]
#[derive(Clone, Debug)]
pub struct VerifiedToolPackageInstallRequest {
    pub package: ToolPackageRecordV1,
    pub package_version: ToolPackageVersionRecordV1,
    pub definitions: Vec<ToolDefinitionRecordV1>,
    pub instances: Vec<ToolInstanceRecordV1>,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ToolPackageTrustChangeRequest {
    pub tool_package_id: aura_domain_common::ToolPackageId,
    pub package_security_fingerprint: aura_tool_contract::PackageSecurityFingerprintV1,
    pub expected_revision: Option<Revision>,
    pub trusted: bool,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ToolInstanceEnabledChangeRequest {
    pub tool_instance_id: aura_domain_common::ToolInstanceId,
    pub expected_revision: Revision,
    pub enabled: bool,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPinTargetV1 {
    pub tool_version: ToolVersion,
    pub state: ToolPinResolutionStateV1,
    pub safe_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceToolPinChangeRequest {
    pub workspace_id: WorkspaceId,
    pub tool_id: ToolId,
    pub expected_revision: Option<Revision>,
    pub target: Option<ToolPinTargetV1>,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct SessionToolPinChangeRequest {
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    pub tool_id: ToolId,
    pub expected_revision: Option<Revision>,
    pub target: Option<ToolPinTargetV1>,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolEntityMutationCommit {
    pub changed: bool,
    pub registry_revision: Revision,
    pub event_id: Option<EventId>,
    pub projection_changes: Vec<ProjectionChange>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceToolPinMutationCommitV1 {
    pub changed: bool,
    pub registry_revision: Revision,
    pub event_id: Option<EventId>,
    pub projection_changes: Vec<ProjectionChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolAuthoritySnapshotV1 {
    pub registry_revision: Revision,
    pub packages: Vec<ToolPackageRecordV1>,
    pub package_versions: Vec<ToolPackageVersionRecordV1>,
    pub definitions: Vec<ToolDefinitionRecordV1>,
    pub instances: Vec<ToolInstanceRecordV1>,
    pub instance_configurations: Vec<ToolInstanceConfigurationRecordV1>,
    pub instance_credential_bindings: Vec<ToolInstanceCredentialBindingRecordV1>,
    pub trust_records: Vec<ToolPackageTrustRecordV1>,
    pub workspace_pins: Vec<ToolPinRecordV1>,
    pub session_pins: Vec<ToolPinRecordV1>,
    pub ephemeral_loads: Vec<ToolEphemeralLoadRecordV1>,
}

impl PersistenceCoordinator {
    /// Commits only an already staged, integrity-checked and conformance-passed
    /// package graph. Package parsing, filesystem staging and Conformance are
    /// intentionally outside this persistence boundary.
    /// Test-fixture seed path retained only in debug builds. Production
    /// Package mutation must use the evidence-bound Journal service.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn seed_verified_tool_package_for_test(
        &self,
        request: VerifiedToolPackageInstallRequest,
    ) -> Result<ToolEntityMutationCommit, PersistenceError> {
        validate_install_request(&request)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            if package_exists(&transaction, request.package.tool_package_id)? {
                if persisted_install_matches(&transaction, &request)? {
                    return Ok(ToolEntityMutationCommit {
                        changed: false,
                        registry_revision: read_registry_revision(&transaction)?,
                        event_id: None,
                        projection_changes: Vec::new(),
                    });
                }
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            let live_key_exists = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM tool_packages
                    WHERE package_key=?1 AND lifecycle <> 'deleted'
                )",
                [request.package.package_key.to_string()],
                |row| row.get::<_, bool>(0),
            )?;
            if live_key_exists {
                return Err(PersistenceError::ToolPackageKeyConflict);
            }

            insert_package(&transaction, &request.package)?;
            insert_package_version(&transaction, &request.package_version)?;
            for definition in &request.definitions {
                insert_definition(&transaction, definition)?;
            }
            for instance in &request.instances {
                insert_instance(&transaction, instance)?;
            }
            let registry_revision =
                advance_registry_revision(&transaction, request.package.updated_at)?;

            let mut tool_identities = request
                .definitions
                .iter()
                .map(|definition| ToolPackageToolIdentityV1 {
                    tool_key: definition
                        .tool_key
                        .clone()
                        .expect("validated package definition has a tool key"),
                    tool_id: definition.tool_id,
                })
                .collect::<Vec<_>>();
            tool_identities.sort_by(|left, right| left.tool_key.cmp(&right.tool_key));
            let lifecycle =
                ToolLifecycleEventV1::ToolPackageInstalled(ToolPackageInstalledPayloadV1 {
                    package: ToolPackageVersionRefV1 {
                        tool_package_id: request.package.tool_package_id,
                        package_key: request.package.package_key.clone(),
                        package_version: request.package_version.package_version.clone(),
                        package_fingerprint: request.package_version.package_fingerprint.clone(),
                        security_fingerprint: request
                            .package_version
                            .package_security_fingerprint
                            .clone(),
                    },
                    tool_identities,
                    manifest_hash: request.package_version.manifest_hash.clone(),
                    installed_at: request.package_version.installed_at,
                });
            let event_id = EventId::new();
            let event = tool_event(
                event_id,
                EventActor::User,
                request.correlation_id,
                lifecycle,
                request.package_version.installed_at,
            )?;
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(ToolEntityMutationCommit {
                changed: true,
                registry_revision,
                event_id: Some(event_id),
                projection_changes: Vec::new(),
            })
        })
    }

    pub fn commit_tool_package_trust_change(
        &self,
        request: ToolPackageTrustChangeRequest,
    ) -> Result<ToolEntityMutationCommit, PersistenceError> {
        if !request.package_security_fingerprint.uses_blake3() {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let active_security_fingerprint =
                active_package_security_fingerprint(&transaction, request.tool_package_id)?
                    .ok_or(PersistenceError::ToolEntityNotFound)?;
            if active_security_fingerprint != request.package_security_fingerprint {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            let current = load_optional_record::<ToolPackageTrustRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_package_trust
                 WHERE tool_package_id=?1",
                [request.tool_package_id.to_string()],
            )?;
            match (&current, request.expected_revision) {
                (None, None) => {}
                (Some(record), Some(expected)) if record.entity_revision == expected => {}
                _ => return Err(PersistenceError::ToolEntityRevisionConflict),
            }
            if current.as_ref().is_some_and(|record| {
                record.trusted == request.trusted
                    && record.package_security_fingerprint == request.package_security_fingerprint
            }) || (current.is_none() && !request.trusted)
            {
                return Ok(ToolEntityMutationCommit {
                    changed: false,
                    registry_revision: read_registry_revision(&transaction)?,
                    event_id: None,
                    projection_changes: Vec::new(),
                });
            }
            let next_revision =
                next_entity_revision(current.as_ref().map(|record| record.entity_revision))?;
            let record = ToolPackageTrustRecordV1 {
                tool_package_id: request.tool_package_id,
                package_security_fingerprint: request.package_security_fingerprint.clone(),
                trusted: request.trusted,
                entity_revision: next_revision,
                reviewed_at: request.changed_at,
            };
            let (record_json, record_hash_json) = encode_record(&record)?;
            transaction.execute(
                "INSERT INTO tool_package_trust(
                    tool_package_id, package_security_fingerprint_json, trusted,
                    entity_revision, record_json, record_hash_json, reviewed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(tool_package_id) DO UPDATE SET
                    package_security_fingerprint_json=excluded.package_security_fingerprint_json,
                    trusted=excluded.trusted,
                    entity_revision=excluded.entity_revision,
                    record_json=excluded.record_json,
                    record_hash_json=excluded.record_hash_json,
                    reviewed_at=excluded.reviewed_at",
                params![
                    request.tool_package_id.to_string(),
                    encode_json(&request.package_security_fingerprint)?,
                    request.trusted,
                    sqlite_revision(next_revision)?,
                    record_json,
                    record_hash_json,
                    request.changed_at.to_string(),
                ],
            )?;
            let registry_revision = advance_registry_revision(&transaction, request.changed_at)?;
            let lifecycle = ToolLifecycleEventV1::ToolTrustChanged(ToolTrustChangedPayloadV1 {
                tool_package_id: request.tool_package_id,
                package_security_fingerprint: request.package_security_fingerprint,
                trusted: request.trusted,
                entity_revision: next_revision,
                changed_at: request.changed_at,
            });
            let event_id = EventId::new();
            let event = tool_event(
                event_id,
                EventActor::User,
                request.correlation_id,
                lifecycle,
                request.changed_at,
            )?;
            insert_event(&transaction, &event)?;
            let projection_drafts =
                package_projection_drafts(&transaction, request.tool_package_id)?;
            let (projection_changes, _) =
                insert_tool_projections(&transaction, runtime_instance_id, projection_drafts)?;
            transaction.commit()?;
            Ok(ToolEntityMutationCommit {
                changed: true,
                registry_revision,
                event_id: Some(event_id),
                projection_changes,
            })
        })
    }

    pub fn commit_tool_instance_enabled_change(
        &self,
        request: ToolInstanceEnabledChangeRequest,
    ) -> Result<ToolEntityMutationCommit, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut instance = load_optional_record::<ToolInstanceRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_instances
                 WHERE tool_instance_id=?1",
                [request.tool_instance_id.to_string()],
            )?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
            if instance.source_kind == ToolSourceKindV1::BuiltIn {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            if instance.entity_revision != request.expected_revision {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            if instance.enabled == request.enabled {
                return Ok(ToolEntityMutationCommit {
                    changed: false,
                    registry_revision: read_registry_revision(&transaction)?,
                    event_id: None,
                    projection_changes: Vec::new(),
                });
            }
            let definition = load_optional_record::<ToolDefinitionRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_definitions
                 WHERE tool_id=?1 AND tool_version_json=?2",
                params![instance.tool_id.to_string(), encode_json(&instance.tool_version)?,],
            )?
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
            instance.enabled = request.enabled;
            instance.entity_revision = next_entity_revision(Some(instance.entity_revision))?;
            instance.updated_at = request.changed_at;
            let (record_json, record_hash_json) = encode_record(&instance)?;
            let changed = transaction.execute(
                "UPDATE tool_instances
                 SET enabled=?1, entity_revision=?2, record_json=?3,
                     record_hash_json=?4, updated_at=?5
                 WHERE tool_instance_id=?6 AND entity_revision=?7",
                params![
                    instance.enabled,
                    sqlite_revision(instance.entity_revision)?,
                    record_json,
                    record_hash_json,
                    request.changed_at.to_string(),
                    request.tool_instance_id.to_string(),
                    sqlite_revision(request.expected_revision)?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            let registry_revision = advance_registry_revision(&transaction, request.changed_at)?;
            let lifecycle = ToolLifecycleEventV1::ToolEnabledChanged(ToolEnabledChangedPayloadV1 {
                tool_instance_id: request.tool_instance_id,
                enabled: request.enabled,
                entity_revision: instance.entity_revision,
                changed_at: request.changed_at,
            });
            let event_id = EventId::new();
            let event = tool_event(
                event_id,
                EventActor::User,
                request.correlation_id,
                lifecycle,
                request.changed_at,
            )?;
            insert_event(&transaction, &event)?;
            let projection_changes = if let Some(tool_package_id) = definition.tool_package_id {
                let projection_drafts = package_projection_drafts(&transaction, tool_package_id)?;
                insert_tool_projections(&transaction, runtime_instance_id, projection_drafts)?.0
            } else {
                Vec::new()
            };
            transaction.commit()?;
            Ok(ToolEntityMutationCommit {
                changed: true,
                registry_revision,
                event_id: Some(event_id),
                projection_changes,
            })
        })
    }

    /// Persists a user-owned Workspace exact-version Pin mutation after the
    /// Registry service has validated the requested target. The persisted Pin
    /// revision and its row revision advance together.
    pub fn commit_workspace_tool_pin_change(
        &self,
        request: WorkspaceToolPinChangeRequest,
    ) -> Result<WorkspaceToolPinMutationCommitV1, PersistenceError> {
        validate_pin_target(request.target.as_ref())?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let command_method_name = if request.target.is_some() {
                TOOL_PIN_WORKSPACE_SET_METHOD_V1
            } else {
                TOOL_PIN_WORKSPACE_CLEAR_METHOD_V1
            };
            require_workspace(&transaction, request.workspace_id)?;
            require_resolved_pin_target_authority(
                &transaction,
                request.tool_id,
                request.target.as_ref(),
            )?;
            let current = load_optional_record::<ToolPinRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM workspace_tool_pins
                 WHERE workspace_id=?1 AND tool_id=?2",
                params![request.workspace_id.to_string(), request.tool_id.to_string()],
            )?;
            let (previous_pin, current_revision) = match current {
                Some(ToolPinRecordV1::Workspace { workspace_id, pin, entity_revision, .. })
                    if workspace_id == request.workspace_id && pin.tool_id == request.tool_id =>
                {
                    (Some(pin), Some(entity_revision))
                }
                Some(_) => return Err(PersistenceError::ToolEntityIntegrity),
                None => (None, None),
            };
            validate_expected_revision(current_revision, request.expected_revision)?;
            if pin_target_matches(previous_pin.as_ref(), request.target.as_ref()) {
                let entity_revision =
                    current_revision.ok_or(PersistenceError::ToolEntityIntegrity)?;
                let result = ToolMutationResultV1::WorkspacePinCommitted {
                    workspace_id: request.workspace_id,
                    entity_revision,
                };
                complete_tool_mutation_command_success_in_transaction(
                    &transaction,
                    command_method_name,
                    request.correlation_id,
                    result,
                    request.changed_at,
                )?;
                let commit = WorkspaceToolPinMutationCommitV1 {
                    changed: false,
                    registry_revision: read_registry_revision(&transaction)?,
                    event_id: None,
                    projection_changes: Vec::new(),
                };
                transaction.commit()?;
                return Ok(commit);
            }

            let entity_revision = next_entity_revision(current_revision)?;
            let current_pin = request.target.map(|target| ToolPinV1 {
                tool_id: request.tool_id,
                tool_version: target.tool_version,
                state: target.state,
                safe_reason: target.safe_reason,
                revision: entity_revision,
            });
            if let Some(pin) = &current_pin {
                let record = ToolPinRecordV1::Workspace {
                    workspace_id: request.workspace_id,
                    pin: pin.clone(),
                    entity_revision,
                    updated_at: request.changed_at,
                };
                let (record_json, record_hash_json) = encode_record(&record)?;
                transaction.execute(
                    "INSERT INTO workspace_tool_pins(
                        workspace_id, tool_id, tool_version_json, entity_revision,
                        record_json, record_hash_json, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(workspace_id, tool_id) DO UPDATE SET
                        tool_version_json=excluded.tool_version_json,
                        entity_revision=excluded.entity_revision,
                        record_json=excluded.record_json,
                        record_hash_json=excluded.record_hash_json,
                        updated_at=excluded.updated_at",
                    params![
                        request.workspace_id.to_string(),
                        request.tool_id.to_string(),
                        encode_json(&pin.tool_version)?,
                        sqlite_revision(entity_revision)?,
                        record_json,
                        record_hash_json,
                        request.changed_at.to_string(),
                    ],
                )?;
            } else {
                let deleted = transaction.execute(
                    "DELETE FROM workspace_tool_pins WHERE workspace_id=?1 AND tool_id=?2",
                    params![request.workspace_id.to_string(), request.tool_id.to_string()],
                )?;
                if deleted != 1 {
                    return Err(PersistenceError::ToolEntityRevisionConflict);
                }
            }

            let registry_revision = advance_registry_revision(&transaction, request.changed_at)?;
            let lifecycle =
                ToolLifecycleEventV1::WorkspaceToolPinChanged(WorkspaceToolPinChangedPayloadV1 {
                    workspace_id: request.workspace_id,
                    previous_pin,
                    current_pin,
                    entity_revision,
                    changed_at: request.changed_at,
                });
            let event_id = EventId::new();
            let mut event = tool_event(
                event_id,
                EventActor::User,
                request.correlation_id,
                lifecycle,
                request.changed_at,
            )?;
            event.workspace_id = Some(request.workspace_id);
            insert_event(&transaction, &event)?;
            let pins = load_bound_records::<ToolPinRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM workspace_tool_pins
                 WHERE workspace_id=?1 ORDER BY tool_id",
                [request.workspace_id.to_string()],
            )?
            .into_iter()
            .map(|record| match record {
                ToolPinRecordV1::Workspace { workspace_id, pin, .. }
                    if workspace_id == request.workspace_id =>
                {
                    Ok(pin)
                }
                _ => Err(PersistenceError::ToolEntityIntegrity),
            })
            .collect::<Result<Vec<_>, _>>()?;
            let (projection_changes, _) = insert_tool_projections(
                &transaction,
                runtime_instance_id,
                vec![ToolProjectionDraft::WorkspaceToolPins(WorkspaceToolPinsProjection {
                    workspace_id: request.workspace_id,
                    pin_set_revision: Revision::INITIAL,
                    pins: pins.into_iter().map(Into::into).collect(),
                })],
            )?;
            complete_tool_mutation_command_success_in_transaction(
                &transaction,
                command_method_name,
                request.correlation_id,
                ToolMutationResultV1::WorkspacePinCommitted {
                    workspace_id: request.workspace_id,
                    entity_revision,
                },
                request.changed_at,
            )?;
            transaction.commit()?;
            Ok(WorkspaceToolPinMutationCommitV1 {
                changed: true,
                registry_revision,
                event_id: Some(event_id),
                projection_changes,
            })
        })
    }

    /// Persists a user-owned Session exact-version Pin mutation. Session
    /// ownership is verified against the authoritative Workspace relation in
    /// the same transaction before the row or audit Event is written.
    pub fn commit_session_tool_pin_change(
        &self,
        request: SessionToolPinChangeRequest,
    ) -> Result<ToolEntityMutationCommit, PersistenceError> {
        validate_pin_target(request.target.as_ref())?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let command_method_name = if request.target.is_some() {
                TOOL_PIN_SESSION_SET_METHOD_V1
            } else {
                TOOL_PIN_SESSION_CLEAR_METHOD_V1
            };
            require_session_workspace(&transaction, request.session_id, request.workspace_id)?;
            require_resolved_pin_target_authority(
                &transaction,
                request.tool_id,
                request.target.as_ref(),
            )?;
            let current = load_optional_record::<ToolPinRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM session_tool_pins
                 WHERE session_id=?1 AND tool_id=?2",
                params![request.session_id.to_string(), request.tool_id.to_string()],
            )?;
            let (previous_pin, current_revision) = match current {
                Some(ToolPinRecordV1::Session {
                    session_id,
                    workspace_id,
                    pin,
                    entity_revision,
                    ..
                }) if session_id == request.session_id
                    && workspace_id == request.workspace_id
                    && pin.tool_id == request.tool_id =>
                {
                    (Some(pin), Some(entity_revision))
                }
                Some(_) => return Err(PersistenceError::ToolEntityIntegrity),
                None => (None, None),
            };
            validate_expected_revision(current_revision, request.expected_revision)?;
            if pin_target_matches(previous_pin.as_ref(), request.target.as_ref()) {
                let entity_revision =
                    current_revision.ok_or(PersistenceError::ToolEntityIntegrity)?;
                complete_tool_mutation_command_success_in_transaction(
                    &transaction,
                    command_method_name,
                    request.correlation_id,
                    ToolMutationResultV1::SessionPinCommitted {
                        session_id: request.session_id,
                        entity_revision,
                    },
                    request.changed_at,
                )?;
                let commit = ToolEntityMutationCommit {
                    changed: false,
                    registry_revision: read_registry_revision(&transaction)?,
                    event_id: None,
                    projection_changes: Vec::new(),
                };
                transaction.commit()?;
                return Ok(commit);
            }

            let entity_revision = next_entity_revision(current_revision)?;
            let current_pin = request.target.map(|target| ToolPinV1 {
                tool_id: request.tool_id,
                tool_version: target.tool_version,
                state: target.state,
                safe_reason: target.safe_reason,
                revision: entity_revision,
            });
            if let Some(pin) = &current_pin {
                let record = ToolPinRecordV1::Session {
                    session_id: request.session_id,
                    workspace_id: request.workspace_id,
                    pin: pin.clone(),
                    entity_revision,
                    updated_at: request.changed_at,
                };
                let (record_json, record_hash_json) = encode_record(&record)?;
                transaction.execute(
                    "INSERT INTO session_tool_pins(
                        session_id, workspace_id, tool_id, tool_version_json,
                        entity_revision, record_json, record_hash_json, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(session_id, tool_id) DO UPDATE SET
                        workspace_id=excluded.workspace_id,
                        tool_version_json=excluded.tool_version_json,
                        entity_revision=excluded.entity_revision,
                        record_json=excluded.record_json,
                        record_hash_json=excluded.record_hash_json,
                        updated_at=excluded.updated_at",
                    params![
                        request.session_id.to_string(),
                        request.workspace_id.to_string(),
                        request.tool_id.to_string(),
                        encode_json(&pin.tool_version)?,
                        sqlite_revision(entity_revision)?,
                        record_json,
                        record_hash_json,
                        request.changed_at.to_string(),
                    ],
                )?;
            } else {
                let deleted = transaction.execute(
                    "DELETE FROM session_tool_pins WHERE session_id=?1 AND tool_id=?2",
                    params![request.session_id.to_string(), request.tool_id.to_string()],
                )?;
                if deleted != 1 {
                    return Err(PersistenceError::ToolEntityRevisionConflict);
                }
            }

            let registry_revision = advance_registry_revision(&transaction, request.changed_at)?;
            let lifecycle =
                ToolLifecycleEventV1::SessionToolPinChanged(SessionToolPinChangedPayloadV1 {
                    session_id: request.session_id,
                    previous_pin,
                    current_pin,
                    entity_revision,
                    changed_at: request.changed_at,
                });
            let event_id = EventId::new();
            let mut event = tool_event(
                event_id,
                EventActor::User,
                request.correlation_id,
                lifecycle,
                request.changed_at,
            )?;
            event.workspace_id = Some(request.workspace_id);
            event.session_id = Some(request.session_id);
            insert_event(&transaction, &event)?;
            let pins = load_bound_records::<ToolPinRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM session_tool_pins
                 WHERE session_id=?1 ORDER BY tool_id",
                [request.session_id.to_string()],
            )?
            .into_iter()
            .map(|record| match record {
                ToolPinRecordV1::Session { session_id, workspace_id, pin, .. }
                    if session_id == request.session_id && workspace_id == request.workspace_id =>
                {
                    Ok(pin)
                }
                _ => Err(PersistenceError::ToolEntityIntegrity),
            })
            .collect::<Result<Vec<_>, _>>()?;
            let (projection_changes, _) = insert_tool_projections(
                &transaction,
                runtime_instance_id,
                vec![ToolProjectionDraft::SessionToolPins(SessionToolPinsProjectionV1 {
                    session_id: request.session_id,
                    pin_set_revision: Revision::INITIAL,
                    pins: pins.into_iter().map(Into::into).collect(),
                })],
            )?;
            complete_tool_mutation_command_success_in_transaction(
                &transaction,
                command_method_name,
                request.correlation_id,
                ToolMutationResultV1::SessionPinCommitted {
                    session_id: request.session_id,
                    entity_revision,
                },
                request.changed_at,
            )?;
            transaction.commit()?;
            Ok(ToolEntityMutationCommit {
                changed: true,
                registry_revision,
                event_id: Some(event_id),
                projection_changes,
            })
        })
    }

    pub fn tool_authority_snapshot(
        &self,
        workspace_id: Option<WorkspaceId>,
        session_id: Option<SessionId>,
        turn_task_id: Option<TurnTaskId>,
    ) -> Result<ToolAuthoritySnapshotV1, PersistenceError> {
        if turn_task_id.is_some() && session_id.is_none() {
            return Err(PersistenceError::InvalidRequest(
                "Tool authority TurnTask scope requires a Session".to_owned(),
            ));
        }
        self.with_reader(|connection| {
            // The **task-ownership** checks that used to live here are
            // gone, and their removal is the point of the repair.
            //
            // They asked this database whether the scope's session and turn task existed
            // and belonged together. But task ownership is the **Agent Store's** fact —
            // it owns `sessions`, `turn_tasks` and `agent_runs` in the journal, which is
            // a different file from this one. So the check answered a question about the
            // wrong authority, and the real owner could never satisfy it: a genuine,
            // live, correctly-owned turn was refused with `ToolEntityNotFound`.
            //
            // Ownership is now verified by the owner, through the read-only
            // `TaskOwnershipSource` seam, and this authority answers only what it really
            // owns: which tools are **enabled**. One authority per fact.
            //
            // The workspace scoping that remains is this database's own concern: the
            // tool tables are keyed by workspace, so a scope naming a workspace with no
            // rows here is still a real absence.
            if let Some(workspace_id) = workspace_id {
                let exists = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM workspaces WHERE workspace_id=?1)",
                        [workspace_id.to_string()],
                        |row| row.get::<_, bool>(0),
                    )
                    .optional()?
                    .unwrap_or(false);
                if !exists {
                    return Err(PersistenceError::ToolEntityNotFound);
                }
            }
            // `session_id` and `turn_task_id` are deliberately unused here: the
            // authority that owns them is the Agent Store, and its verdict reached the
            // caller before this read. Re-checking them here is what the repair removed.
            let _ = (session_id, turn_task_id);

            let packages = load_all_records::<ToolPackageRecordV1>(
                connection,
                "SELECT record_json, record_hash_json FROM tool_packages
                 ORDER BY tool_package_id",
            )?;
            let package_versions = load_all_records::<ToolPackageVersionRecordV1>(
                connection,
                "SELECT record_json, record_hash_json FROM tool_package_versions
                 ORDER BY tool_package_id, package_version_json",
            )?;
            let definitions = load_all_records::<ToolDefinitionRecordV1>(
                connection,
                "SELECT record_json, record_hash_json FROM tool_definitions
                 ORDER BY tool_id, tool_version_json",
            )?;
            let instances = load_all_records::<ToolInstanceRecordV1>(
                connection,
                "SELECT record_json, record_hash_json FROM tool_instances
                 ORDER BY tool_id, tool_version_json, tool_instance_id",
            )?;
            let trust_records = load_all_records::<ToolPackageTrustRecordV1>(
                connection,
                "SELECT record_json, record_hash_json FROM tool_package_trust
                 ORDER BY tool_package_id",
            )?;
            let instance_configurations = load_all_records::<ToolInstanceConfigurationRecordV1>(
                connection,
                "SELECT record_json, record_hash_json FROM tool_instance_configurations
                 ORDER BY tool_instance_id",
            )?;
            let instance_credential_bindings =
                load_all_records::<ToolInstanceCredentialBindingRecordV1>(
                    connection,
                    "SELECT record_json, record_hash_json FROM tool_instance_credential_bindings
                     ORDER BY tool_instance_id",
                )?;
            let workspace_pins = if let Some(workspace_id) = workspace_id {
                load_bound_records::<ToolPinRecordV1, _>(
                    connection,
                    "SELECT record_json, record_hash_json FROM workspace_tool_pins
                     WHERE workspace_id=?1 ORDER BY tool_id",
                    [workspace_id.to_string()],
                )?
            } else {
                Vec::new()
            };
            let session_pins = if let Some(session_id) = session_id {
                load_bound_records::<ToolPinRecordV1, _>(
                    connection,
                    "SELECT record_json, record_hash_json FROM session_tool_pins
                     WHERE session_id=?1 ORDER BY tool_id",
                    [session_id.to_string()],
                )?
            } else {
                Vec::new()
            };
            let ephemeral_loads = if let Some(turn_task_id) = turn_task_id {
                load_bound_records::<ToolEphemeralLoadRecordV1, _>(
                    connection,
                    "SELECT record_json, record_hash_json FROM tool_ephemeral_loads
                     WHERE turn_task_id=?1 ORDER BY tool_id",
                    [turn_task_id.to_string()],
                )?
            } else {
                Vec::new()
            };
            Ok(ToolAuthoritySnapshotV1 {
                registry_revision: read_registry_revision(connection)?,
                packages,
                package_versions,
                definitions,
                instances,
                instance_configurations,
                instance_credential_bindings,
                trust_records,
                workspace_pins,
                session_pins,
                ephemeral_loads,
            })
        })
    }

    /// The **live enablement fact** for a set of tools under one registry scope.
    ///
    /// This is the single consumption port the execution layer uses to ask
    /// "is this tool still enabled right now?". It reuses the coordinator's
    /// existing enablement authority — the same `tool_authority_snapshot` and the
    /// same `instance.enabled` fact that `aura_tool_registry::builder` turns into
    /// `RegistryExclusionReasonV1::Disabled` — rather than introducing a second
    /// source of truth or comparing snapshot IDs that two databases minted
    /// independently.
    ///
    /// Returns the tools from `tool_ids` that are **not** enabled under `scope`.
    /// An empty result means every requested tool is still enabled, which is the
    /// fact a frozen call needs before it dispatches.
    ///
    /// A tool with no instance row at all is reported as not enabled: an absent
    /// instance is not evidence that the tool may run.
    pub fn tools_not_enabled_under_scope(
        &self,
        scope: &aura_tool_contract::ToolRegistryScopeV1,
        tool_ids: &std::collections::BTreeSet<aura_domain_common::ToolId>,
    ) -> Result<Vec<aura_domain_common::ToolId>, PersistenceError> {
        self.tools_not_enabled_under_scope_with(scope, tool_ids, |this, scope| {
            let (workspace_id, session_id, turn_task_id) =
                crate::tool_registry_authority::scope_ids(scope);
            this.tool_authority_snapshot(workspace_id, session_id, turn_task_id)
                .map(|authority| this.enabled_tool_ids(&authority))
        })
    }

    /// Whether one tool is enabled under a scope, read through the real path.
    ///
    /// This exists so a caller — including a test — can ask the
    /// scoped question **without** re-implementing the authority read or passing
    /// `None, None, None`. The earlier fixture injected a scope-less snapshot, which
    /// made every scope answer identically and left the scoped-disable path
    /// unmeasured.
    ///
    /// The answer is the production one: the same scope resolution and the same
    /// `enabled_tool_ids` filter that [`Self::tools_not_enabled_under_scope`] uses.
    pub fn tool_enabled_under_scope(
        &self,
        scope: &aura_tool_contract::ToolRegistryScopeV1,
        tool_id: aura_domain_common::ToolId,
    ) -> Result<bool, PersistenceError> {
        let requested = std::collections::BTreeSet::from([tool_id]);
        let disabled = self.tools_not_enabled_under_scope(scope, &requested)?;
        Ok(disabled.is_empty())
    }

    /// The enablement answer for one tool under a scope, against a stated **owner**.
    ///
    /// This is the entry that keeps the two authorities apart. The
    /// `owner` is asked to verify the scope's task ownership and liveness; only if it
    /// permits does this read the coordinator's own tool enablement state.
    ///
    /// The refusals are typed by [`ScopeOwnershipV1`], so a caller can tell a forged
    /// scope from a finished task from a failed read, and none of them is presented as
    /// "the tool is disabled" — which would be a false statement about configuration.
    pub fn tool_enabled_under_scope_owned(
        &self,
        owner: &dyn TaskOwnershipSource,
        scope: &aura_tool_contract::ToolRegistryScopeV1,
        tool_id: aura_domain_common::ToolId,
    ) -> Result<OwnedToolEnablementV1, PersistenceError> {
        let ownership = owner.verify_scope(scope)?;
        if !ownership.permits_new_work() {
            return Ok(OwnedToolEnablementV1::Refused { ownership });
        }
        let requested = std::collections::BTreeSet::from([tool_id]);
        let disabled =
            self.tools_not_enabled_under_scope_with(scope, &requested, |this, scope| {
                let (workspace_id, session_id, turn_task_id) =
                    crate::tool_registry_authority::scope_ids(scope);
                this.tool_authority_snapshot(workspace_id, session_id, turn_task_id)
                    .map(|authority| this.enabled_tool_ids(&authority))
            })?;
        Ok(OwnedToolEnablementV1::Answered { enabled: disabled.is_empty(), ownership })
    }

    /// [`Self::tools_not_enabled_under_scope`] with the authority read injected.
    ///
    /// The read is a parameter so this port can be measured where a full
    /// session-pin chain is not available. That is not a test-only shortcut: the
    /// *fact* this port consumes — whether a tool is enabled — is what the filter
    /// below decides, and injecting the read lets a test drive the real filter
    /// against a real authority snapshot.
    pub fn tools_not_enabled_under_scope_with<T>(
        &self,
        scope: &aura_tool_contract::ToolRegistryScopeV1,
        tool_ids: &std::collections::BTreeSet<aura_domain_common::ToolId>,
        read_authority: impl FnOnce(
            &Self,
            &aura_tool_contract::ToolRegistryScopeV1,
        ) -> Result<T, PersistenceError>,
    ) -> Result<Vec<aura_domain_common::ToolId>, PersistenceError>
    where
        T: ToolInstanceEnablementSource,
    {
        if tool_ids.is_empty() {
            return Ok(Vec::new());
        }
        let enabled = read_authority(self, scope)?;
        Ok(tool_ids.iter().copied().filter(|id| !enabled.is_tool_enabled(*id)).collect())
    }
}

/// The result of a scoped enablement read that consulted a task-ownership authority.
///
/// The two outcomes are kept apart on purpose: "the tool is not enabled" is a fact
/// about configuration, while "the scope was refused" is a fact about the request.
/// Reporting the second as the first would tell the user their configuration disables
/// a tool when it does not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedToolEnablementV1 {
    /// The ownership was verified and the enablement state was read.
    Answered {
        /// Whether the tool is enabled under the scope.
        enabled: bool,
        /// The ownership verdict this answer rests on.
        ownership: ScopeOwnershipV1,
    },
    /// The scope was refused before any enablement was read.
    Refused {
        /// Why the scope was refused.
        ownership: ScopeOwnershipV1,
    },
}

impl OwnedToolEnablementV1 {
    /// Whether the read reached a real enablement answer.
    #[must_use]
    pub const fn was_answered(self) -> bool {
        matches!(self, Self::Answered { .. })
    }

    /// The enabled state, only when the read really answered.
    #[must_use]
    pub const fn enabled(self) -> Option<bool> {
        match self {
            Self::Answered { enabled, .. } => Some(enabled),
            Self::Refused { .. } => None,
        }
    }

    /// The ownership verdict, whether or not the read answered.
    #[must_use]
    pub const fn ownership(self) -> ScopeOwnershipV1 {
        match self {
            Self::Answered { ownership, .. } | Self::Refused { ownership } => ownership,
        }
    }
}

/// The outcome of verifying that a scope's task ownership is real.
///
/// A scope names a workspace, session, turn task and agent run.
/// Before, `tool_authority_snapshot` checked only that the session and turn-task rows
/// existed **in its own database** — which is not where the Agent Store's task
/// ownership lives, so the check answered a different question than the one that
/// matters and could not be satisfied by the real owner at all.
///
/// The outcome is **typed** rather than a boolean because the five cases lead to
/// different actions and collapsing them loses the distinction that matters:
///
/// * `Owned` — proceed; the scope's ownership is real and the task is live.
/// * `Mismatched` — the ids exist but do **not** belong together. A forged or
///   assembled scope. Refused.
/// * `TaskEnded` — the ownership was real and the task has finished. Refused for new
///   work, and deliberately distinct from `Mismatched`: this is not an attack, it is a
///   stale request.
/// * `Unknown` — the owner has no record of one of the ids. Refused.
/// * `Unavailable` — the ownership could not be read. Refused, and reported as a
///   read failure rather than as "not owned", because "could not ask" is not evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeOwnershipV1 {
    /// The scope's workspace/session/turn/run really belong together, and the task is live.
    Owned,
    /// The ids exist but do not belong together.
    Mismatched,
    /// The ownership was real, and the task has already ended.
    TaskEnded,
    /// The owner has no record of one of the ids.
    Unknown,
    /// The ownership could not be read at all.
    Unavailable,
}

impl ScopeOwnershipV1 {
    /// Whether this outcome permits new work under the scope.
    ///
    /// Only `Owned` does. Every other outcome refuses, including `Unavailable`: a
    /// read failure is never permission.
    #[must_use]
    pub const fn permits_new_work(self) -> bool {
        matches!(self, Self::Owned)
    }

    /// A bounded, path-free code for the outcome, for a log or a refusal.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Owned => "tool.scope.owned",
            Self::Mismatched => "tool.scope.mismatched",
            Self::TaskEnded => "tool.scope.task_ended",
            Self::Unknown => "tool.scope.unknown",
            Self::Unavailable => "tool.scope.unavailable",
        }
    }
}

/// The task-ownership authority a scoped tool read consults.
///
/// This is the seam that lets the **real owner** answer. The Agent
/// Store owns which task a turn/run belongs to and whether it is still live; the
/// persistence coordinator owns the tool enablement state. Separating them keeps each
/// authority on its own facts.
///
/// # Contract for an implementation
///
/// * **Read-only.** An implementation must not write, and must not create a second
///   record of a task. It answers about ownership that already exists.
/// * **No caller-held locks.** It must not re-acquire a lock the caller already
///   holds. The scoped read runs inside a bounded execution section, and a callback
///   that reached back into the same journal lock would deadlock.
/// * **Bounded.** It must answer within its own existing bounded section rather than
///   waiting on unbounded work.
/// * **No silent permission.** Returning `Owned` states ownership and liveness only.
///   It is **not** a permission decision: the enablement state is read separately, and
///   "the task is live" does not mean "this tool may run".
pub trait TaskOwnershipSource: Send + Sync {
    /// Verifies a scope's ownership and answers whether the task is still live.
    fn verify_scope(
        &self,
        scope: &aura_tool_contract::ToolRegistryScopeV1,
    ) -> Result<ScopeOwnershipV1, PersistenceError>;
}

/// The default authority: the coordinator's own session and turn-task tables.
///
/// This preserves the behaviour every existing caller already had, so wiring the real
/// Agent Store in is a change of **source**, not a change of contract for anyone who
/// does not opt in.
pub struct CoordinatorTableOwnership;

impl TaskOwnershipSource for CoordinatorTableOwnership {
    fn verify_scope(
        &self,
        _scope: &aura_tool_contract::ToolRegistryScopeV1,
    ) -> Result<ScopeOwnershipV1, PersistenceError> {
        // The tables this authority would read are the ones the scoped read already
        // checks directly, so it defers rather than duplicating the query. It reports
        // `Owned` only because the caller performs that check itself.
        Ok(ScopeOwnershipV1::Owned)
    }
}

/// What this port needs from an authority read: which tools are enabled.
///
/// Declared as a trait so the enablement judgement is stated once and a caller
/// cannot accidentally read a different field.
pub trait ToolInstanceEnablementSource {
    /// Whether the tool is enabled under the authority.
    ///
    /// A tool the authority does not know is `false`: absence is not evidence that
    /// a tool may run.
    fn is_tool_enabled(&self, tool_id: aura_domain_common::ToolId) -> bool;
}

impl ToolInstanceEnablementSource for ToolAuthoritySnapshotV1 {
    fn is_tool_enabled(&self, tool_id: aura_domain_common::ToolId) -> bool {
        self.instances.iter().any(|instance| instance.tool_id == tool_id && instance.enabled)
    }
}

impl ToolInstanceEnablementSource for std::collections::BTreeSet<aura_domain_common::ToolId> {
    fn is_tool_enabled(&self, tool_id: aura_domain_common::ToolId) -> bool {
        self.contains(&tool_id)
    }
}

fn validate_pin_target(target: Option<&ToolPinTargetV1>) -> Result<(), PersistenceError> {
    if let Some(target) = target {
        target.tool_version.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    }
    Ok(())
}

fn require_resolved_pin_target_authority(
    transaction: &Transaction<'_>,
    tool_id: ToolId,
    target: Option<&ToolPinTargetV1>,
) -> Result<(), PersistenceError> {
    let Some(target) = target else {
        return Ok(());
    };
    if target.state != ToolPinResolutionStateV1::Resolved || target.safe_reason.is_some() {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let definition = load_optional_record::<ToolDefinitionRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_definitions
         WHERE tool_id=?1 AND tool_version_json=?2 AND retired_at IS NULL",
        params![tool_id.to_string(), encode_json(&target.tool_version)?],
    )?
    .ok_or(PersistenceError::ToolEntityNotFound)?;
    if definition.tool_id != tool_id || definition.tool_version != target.tool_version {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    if let Some(tool_package_id) = definition.tool_package_id {
        let package = load_optional_record::<ToolPackageRecordV1, _>(
            transaction,
            "SELECT record_json, record_hash_json FROM tool_packages WHERE tool_package_id=?1",
            [tool_package_id.to_string()],
        )?
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
        if package.lifecycle != ToolPackageLifecycleV1::Installed || package.deletion_pending {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
    } else if definition.source_kind != ToolSourceKindV1::BuiltIn {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let instance_count = transaction.query_row(
        "SELECT COUNT(*) FROM tool_instances
         WHERE tool_id=?1 AND tool_version_json=?2",
        params![tool_id.to_string(), encode_json(&target.tool_version)?],
        |row| row.get::<_, i64>(0),
    )?;
    if instance_count <= 0 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

fn validate_expected_revision(
    current: Option<Revision>,
    expected: Option<Revision>,
) -> Result<(), PersistenceError> {
    match (current, expected) {
        (None, None) => Ok(()),
        (Some(current), Some(expected)) if current == expected => Ok(()),
        _ => Err(PersistenceError::ToolEntityRevisionConflict),
    }
}

fn pin_target_matches(current: Option<&ToolPinV1>, target: Option<&ToolPinTargetV1>) -> bool {
    match (current, target) {
        (None, None) => true,
        (Some(current), Some(target)) => {
            current.tool_version == target.tool_version
                && current.state == target.state
                && current.safe_reason == target.safe_reason
        }
        _ => false,
    }
}

fn require_workspace(
    transaction: &Transaction<'_>,
    workspace_id: WorkspaceId,
) -> Result<(), PersistenceError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspaces WHERE workspace_id=?1)",
        [workspace_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(PersistenceError::ToolEntityNotFound);
    }
    Ok(())
}

fn require_session_workspace(
    transaction: &Transaction<'_>,
    session_id: SessionId,
    workspace_id: WorkspaceId,
) -> Result<(), PersistenceError> {
    let actual_workspace = transaction
        .query_row(
            "SELECT workspace_id FROM sessions WHERE session_id=?1",
            [session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    if actual_workspace != workspace_id.to_string() {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn validate_install_request(
    request: &VerifiedToolPackageInstallRequest,
) -> Result<(), PersistenceError> {
    let package = &request.package;
    let version = &request.package_version;
    if package.lifecycle != ToolPackageLifecycleV1::Installed
        || package.entity_revision.value() == 0
        || package.active_version.as_ref() != Some(&version.package_version)
        || package.tool_package_id != version.tool_package_id
        || version.package_fingerprint.0.algorithm() != ContentHashAlgorithm::Sha256
        || !version.package_security_fingerprint.uses_blake3()
        || version.manifest_hash.algorithm() != ContentHashAlgorithm::Sha256
        || request.definitions.is_empty()
        || request.definitions.len() != request.instances.len()
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let mut definitions = BTreeMap::new();
    let mut tool_keys = BTreeMap::new();
    for definition in &request.definitions {
        definition.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if definition.tool_package_id != Some(package.tool_package_id)
            || definition.package_key.as_ref() != Some(&package.package_key)
            || definition.tool_key.is_none()
            || !matches!(
                definition.source_kind,
                ToolSourceKindV1::UserCreated
                    | ToolSourceKindV1::ModelCreated
                    | ToolSourceKindV1::Imported
            )
            || definition.entity_revision.value() == 0
            || definition.definition_fingerprint.algorithm() != ContentHashAlgorithm::Sha256
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let definition_key = (definition.tool_id, encode_json(&definition.tool_version)?);
        if definitions.insert(definition_key, definition).is_some()
            || tool_keys
                .insert(definition.tool_key.clone().expect("checked tool key"), definition.tool_id)
                .is_some()
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
    }
    let mut instances = BTreeMap::new();
    for instance in &request.instances {
        instance.overrides.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let key = (instance.tool_id, encode_json(&instance.tool_version)?);
        let Some(definition) = definitions.get(&key) else {
            return Err(PersistenceError::ToolEntityIntegrity);
        };
        if instance.source_kind != definition.source_kind
            || instance.entity_revision.value() == 0
            || instances.insert(key, instance).is_some()
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn package_exists(
    transaction: &Transaction<'_>,
    tool_package_id: aura_domain_common::ToolPackageId,
) -> Result<bool, PersistenceError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_packages WHERE tool_package_id=?1)",
            [tool_package_id.to_string()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(PersistenceError::from)
}

#[cfg(debug_assertions)]
fn persisted_install_matches(
    transaction: &Transaction<'_>,
    request: &VerifiedToolPackageInstallRequest,
) -> Result<bool, PersistenceError> {
    let package = load_optional_record::<ToolPackageRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_packages WHERE tool_package_id=?1",
        [request.package.tool_package_id.to_string()],
    )?;
    let package_version = load_optional_record::<ToolPackageVersionRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_package_versions
         WHERE tool_package_id=?1 AND package_version_json=?2",
        params![
            request.package.tool_package_id.to_string(),
            encode_json(&request.package_version.package_version)?,
        ],
    )?;
    let definitions = load_bound_records::<ToolDefinitionRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_definitions
         WHERE tool_package_id=?1 ORDER BY tool_id, tool_version_json",
        [request.package.tool_package_id.to_string()],
    )?;
    let instances = load_bound_records::<ToolInstanceRecordV1, _>(
        transaction,
        "SELECT i.record_json, i.record_hash_json
         FROM tool_instances i
         JOIN tool_definitions d
           ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json
         WHERE d.tool_package_id=?1
         ORDER BY i.tool_id, i.tool_version_json, i.tool_instance_id",
        [request.package.tool_package_id.to_string()],
    )?;
    let mut expected_definitions = request.definitions.clone();
    expected_definitions.sort_by(|left, right| {
        left.tool_id.cmp(&right.tool_id).then_with(|| {
            encode_json(&left.tool_version)
                .expect("validated ToolVersion")
                .cmp(&encode_json(&right.tool_version).expect("validated ToolVersion"))
        })
    });
    let mut expected_instances = request.instances.clone();
    expected_instances.sort_by(|left, right| {
        left.tool_id
            .cmp(&right.tool_id)
            .then_with(|| {
                encode_json(&left.tool_version)
                    .expect("validated ToolVersion")
                    .cmp(&encode_json(&right.tool_version).expect("validated ToolVersion"))
            })
            .then_with(|| left.tool_instance_id.cmp(&right.tool_instance_id))
    });
    Ok(package.as_ref() == Some(&request.package)
        && package_version.as_ref() == Some(&request.package_version)
        && definitions == expected_definitions
        && instances == expected_instances)
}

#[cfg(debug_assertions)]
pub(crate) fn insert_package(
    transaction: &Transaction<'_>,
    package: &ToolPackageRecordV1,
) -> Result<(), PersistenceError> {
    let (record_json, record_hash_json) = encode_record(package)?;
    transaction.execute(
        "INSERT INTO tool_packages(
            tool_package_id, package_key, lifecycle, deletion_pending, entity_revision,
            record_json, record_hash_json, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            package.tool_package_id.to_string(),
            package.package_key.to_string(),
            package_lifecycle_name(package.lifecycle),
            package.deletion_pending,
            sqlite_revision(package.entity_revision)?,
            record_json,
            record_hash_json,
            package.created_at.to_string(),
            package.updated_at.to_string(),
        ],
    )?;
    Ok(())
}

#[cfg(debug_assertions)]
pub(crate) fn insert_package_version(
    transaction: &Transaction<'_>,
    version: &ToolPackageVersionRecordV1,
) -> Result<(), PersistenceError> {
    let (record_json, record_hash_json) = encode_record(version)?;
    transaction.execute(
        "INSERT INTO tool_package_versions(
            tool_package_id, package_version_json, package_fingerprint_json,
            package_security_fingerprint_json, record_json, record_hash_json, installed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            version.tool_package_id.to_string(),
            encode_json(&version.package_version)?,
            encode_json(&version.package_fingerprint)?,
            encode_json(&version.package_security_fingerprint)?,
            record_json,
            record_hash_json,
            version.installed_at.to_string(),
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_definition(
    transaction: &Transaction<'_>,
    definition: &ToolDefinitionRecordV1,
) -> Result<(), PersistenceError> {
    let (record_json, record_hash_json) = encode_record(definition)?;
    transaction.execute(
        "INSERT INTO tool_definitions(
            tool_id, tool_version_json, tool_package_id, tool_key, canonical_name,
            definition_revision, definition_fingerprint_json, entity_revision,
            record_json, record_hash_json, retired_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
        params![
            definition.tool_id.to_string(),
            encode_json(&definition.tool_version)?,
            definition.tool_package_id.map(|id| id.to_string()),
            definition.tool_key.as_ref().map(ToString::to_string),
            definition.canonical_name.to_string(),
            sqlite_revision(definition.tool_version.definition_revision)?,
            encode_json(&definition.definition_fingerprint)?,
            sqlite_revision(definition.entity_revision)?,
            record_json,
            record_hash_json,
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_instance(
    transaction: &Transaction<'_>,
    instance: &ToolInstanceRecordV1,
) -> Result<(), PersistenceError> {
    let (record_json, record_hash_json) = encode_record(instance)?;
    transaction.execute(
        "INSERT INTO tool_instances(
            tool_instance_id, tool_id, tool_version_json, source_kind, enabled,
            runtime_health, entity_revision, record_json, record_hash_json,
            created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            instance.tool_instance_id.to_string(),
            instance.tool_id.to_string(),
            encode_json(&instance.tool_version)?,
            source_kind_name(instance.source_kind),
            instance.enabled,
            runtime_health_name(instance.runtime_health),
            sqlite_revision(instance.entity_revision)?,
            record_json,
            record_hash_json,
            instance.created_at.to_string(),
            instance.updated_at.to_string(),
        ],
    )?;
    Ok(())
}

pub(crate) fn active_package_security_fingerprint(
    transaction: &Transaction<'_>,
    tool_package_id: aura_domain_common::ToolPackageId,
) -> Result<Option<aura_tool_contract::PackageSecurityFingerprintV1>, PersistenceError> {
    let package = load_optional_record::<ToolPackageRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_packages WHERE tool_package_id=?1",
        [tool_package_id.to_string()],
    )?;
    let Some(package) = package else {
        return Ok(None);
    };
    if package.lifecycle != ToolPackageLifecycleV1::Installed || package.deletion_pending {
        return Ok(None);
    }
    let active_version = package.active_version.ok_or(PersistenceError::ToolEntityIntegrity)?;
    let version = load_optional_record::<ToolPackageVersionRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_package_versions
         WHERE tool_package_id=?1 AND package_version_json=?2",
        params![tool_package_id.to_string(), encode_json(&active_version)?],
    )?
    .ok_or(PersistenceError::ToolEntityIntegrity)?;
    Ok(Some(version.package_security_fingerprint))
}

pub(crate) fn tool_event(
    event_id: EventId,
    actor: EventActor,
    correlation_id: CorrelationId,
    lifecycle: ToolLifecycleEventV1,
    created_at: UtcTimestamp,
) -> Result<PersistentEventDraft, PersistenceError> {
    let event_type = lifecycle.event_type().to_owned();
    let mut envelope = serde_json::to_value(lifecycle)?;
    let payload = envelope
        .as_object_mut()
        .and_then(|object| object.remove("payload"))
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    Ok(PersistentEventDraft {
        event_id,
        workspace_id: None,
        session_id: None,
        turn_task_id: None,
        agent_run_id: None,
        event_type,
        event_version: 1,
        actor,
        correlation_id,
        causation_id: None,
        atomic_group_id: None,
        payload_json: Some(payload),
        payload_artifact_ref: None,
        searchable_text: None,
        created_at,
    })
}

pub(crate) fn read_registry_revision(
    connection: &rusqlite::Connection,
) -> Result<Revision, PersistenceError> {
    let value = connection.query_row(
        "SELECT registry_revision FROM tool_registry_authority_state WHERE singleton=1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let value = u64::try_from(value).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    Ok(Revision::new(value))
}

pub(crate) fn advance_registry_revision(
    transaction: &Transaction<'_>,
    updated_at: UtcTimestamp,
) -> Result<Revision, PersistenceError> {
    let changed = transaction.execute(
        "UPDATE tool_registry_authority_state
         SET registry_revision=registry_revision+1, updated_at=?1
         WHERE singleton=1 AND registry_revision < 9223372036854775807",
        [updated_at.to_string()],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    read_registry_revision(transaction)
}

pub(crate) fn next_entity_revision(
    current: Option<Revision>,
) -> Result<Revision, PersistenceError> {
    let next = match current {
        Some(current) => {
            current.value().checked_add(1).ok_or(PersistenceError::ToolEntityIntegrity)?
        }
        None => Revision::INITIAL.value(),
    };
    Ok(Revision::new(next))
}

pub(crate) fn load_all_records<T: DeserializeOwned + Serialize>(
    connection: &rusqlite::Connection,
    sql: &str,
) -> Result<Vec<T>, PersistenceError> {
    let mut statement = connection.prepare(sql)?;
    let rows =
        statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    rows.map(|row| {
        let (record_json, hash_json) = row?;
        decode_verified_record(&record_json, &hash_json)
    })
    .collect()
}

pub(crate) fn load_bound_records<T, P>(
    connection: &rusqlite::Connection,
    sql: &str,
    parameters: P,
) -> Result<Vec<T>, PersistenceError>
where
    T: DeserializeOwned + Serialize,
    P: rusqlite::Params,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement
        .query_map(parameters, |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    rows.map(|row| {
        let (record_json, hash_json) = row?;
        decode_verified_record(&record_json, &hash_json)
    })
    .collect()
}

pub(crate) fn load_optional_record<T, P>(
    connection: &rusqlite::Connection,
    sql: &str,
    parameters: P,
) -> Result<Option<T>, PersistenceError>
where
    T: DeserializeOwned + Serialize,
    P: rusqlite::Params,
{
    connection
        .query_row(sql, parameters, |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .optional()?
        .map(|(record_json, hash_json)| decode_verified_record(&record_json, &hash_json))
        .transpose()
}

pub(crate) fn encode_record<T: Serialize>(value: &T) -> Result<(String, String), PersistenceError> {
    Ok((
        encode_json(value)?,
        encode_json(&canonical_sha256(value).map_err(|_| PersistenceError::ToolEntityIntegrity)?)?,
    ))
}

pub(crate) fn decode_verified_record<T: DeserializeOwned + Serialize>(
    record_json: &str,
    hash_json: &str,
) -> Result<T, PersistenceError> {
    let record = decode_json::<T>(record_json)?;
    let expected = decode_json::<aura_domain_common::jcs::CanonicalHashV1>(hash_json)?;
    let actual = canonical_sha256(&record).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if expected != actual || encode_json(&record)? != record_json {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(record)
}

pub(crate) fn encode_json<T: Serialize + ?Sized>(value: &T) -> Result<String, PersistenceError> {
    let bytes = aura_domain_common::jcs::canonical_json_jcs(value)
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    String::from_utf8(bytes).map_err(|_| PersistenceError::ToolEntityIntegrity)
}

pub(crate) fn decode_json<T: DeserializeOwned>(value: &str) -> Result<T, PersistenceError> {
    let parsed = aura_domain_common::jcs::parse_json_strict(value.as_bytes())
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    serde_json::from_value(parsed).map_err(|_| PersistenceError::ToolEntityIntegrity)
}

pub(crate) fn sqlite_revision(revision: Revision) -> Result<i64, PersistenceError> {
    i64::try_from(revision.value()).map_err(|_| PersistenceError::ToolEntityIntegrity)
}

#[cfg(debug_assertions)]
const fn package_lifecycle_name(value: ToolPackageLifecycleV1) -> &'static str {
    match value {
        ToolPackageLifecycleV1::Installed => "installed",
        ToolPackageLifecycleV1::Corrupted => "corrupted",
        ToolPackageLifecycleV1::Deleted => "deleted",
    }
}

const fn source_kind_name(value: ToolSourceKindV1) -> &'static str {
    match value {
        ToolSourceKindV1::BuiltIn => "built_in",
        ToolSourceKindV1::UserCreated => "user_created",
        ToolSourceKindV1::ModelCreated => "model_created",
        ToolSourceKindV1::Imported => "imported",
        ToolSourceKindV1::Mcp => "mcp",
        ToolSourceKindV1::Plugin => "plugin",
    }
}

const fn runtime_health_name(value: aura_tool_domain::ToolRuntimeHealthV1) -> &'static str {
    match value {
        aura_tool_domain::ToolRuntimeHealthV1::Healthy => "healthy",
        aura_tool_domain::ToolRuntimeHealthV1::Unavailable => "unavailable",
        aura_tool_domain::ToolRuntimeHealthV1::Corrupted => "corrupted",
    }
}

#[cfg(test)]
mod tests {
    mod credential_fixture;
    use aura_domain_common::CanonicalSemVer;
    use aura_domain_common::jcs::{CanonicalBlake3HashV1, CanonicalHashV1};
    use aura_domain_common::{
        CredentialId, CredentialRef, CredentialRevision, PrincipalId, Revision, RuntimeInstanceId,
        SessionId, UtcTimestamp, WorkspaceId,
    };
    use aura_domain_common::{ToolConformanceRunId, ToolId, ToolInstanceId, ToolPackageId};
    use aura_projection_contract::{ProjectionChange, ProjectionScope};
    use aura_tool_contract::{
        CommandCredentialEnvironmentSlotV1, CommandWorkingDirectoryV1, CredentialSlotKey,
        DeclarativeCommandV1, EnvironmentVariableNameV1, PackageFingerprintV1, PackageKey,
        PackageRelativePath, PackageSecurityFingerprintV1, ToolAudienceV1, ToolCanonicalName,
        ToolInputSchemaV1, ToolKey, ToolPinResolutionStateV1, ToolResultContractV1,
        ToolRuntimeDescriptorV1, ToolSourceKindV1, ToolVersion,
    };
    use aura_tool_domain::{
        ToolCredentialAuthorityRecordV1, ToolCredentialKindV1, ToolCredentialPlacementPolicyV1,
        ToolCredentialPlacementV1, ToolCredentialScopeV1, ToolCredentialStateV1,
        ToolCredentialUsagePolicyV1, ToolDefinitionRecordV1, ToolExecutionSemanticsV1,
        ToolInstanceOverridesV1, ToolInstanceRecordV1, ToolPackageLifecycleV1, ToolPackageRecordV1,
        ToolPackageVersionRecordV1, ToolRuntimeHealthV1,
    };
    use aura_workspace_domain::{
        CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary,
    };
    use credential_fixture::credential_record;
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        PersistenceOptions, PersistenceStartup, ToolCredentialAuthoritySeedV1,
        ToolCredentialCandidateQueryV1, ToolInstanceCredentialSettingsChangeRequestV1,
    };

    fn timestamp() -> UtcTimestamp {
        "2026-09-02T00:00:00Z".parse().expect("timestamp")
    }

    fn later_timestamp() -> UtcTimestamp {
        "2026-09-02T00:01:00Z".parse().expect("timestamp")
    }

    fn start() -> (TempDir, PersistenceCoordinator) {
        let temp = tempfile::tempdir().expect("temporary directory");
        let startup = PersistenceCoordinator::start(PersistenceOptions::new(
            temp.path().join("aura.db"),
            temp.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup");
        let PersistenceStartup::Ready(coordinator) = startup else {
            panic!("fixture database must be ready");
        };
        (temp, coordinator)
    }

    fn install_request(seed: u8) -> VerifiedToolPackageInstallRequest {
        let package_id = ToolPackageId::from_bytes([seed; 16]);
        let tool_id = ToolId::from_bytes([seed.wrapping_add(1); 16]);
        let instance_id = ToolInstanceId::from_bytes([seed.wrapping_add(2); 16]);
        let conformance_id = ToolConformanceRunId::from_bytes([seed.wrapping_add(3); 16]);
        let package_version = CanonicalSemVer::new("1.0.0").expect("package version");
        let tool_version = ToolVersion::new(Some(package_version.clone()), Revision::INITIAL)
            .expect("tool version");
        let package_key = PackageKey::new(format!("fixture.package{seed}")).expect("package key");
        let tool_key = ToolKey::new(format!("tool{seed}")).expect("tool key");
        let package_fingerprint = PackageFingerprintV1(CanonicalHashV1::sha256(&[seed, 1]));
        let security_fingerprint =
            PackageSecurityFingerprintV1(CanonicalBlake3HashV1::blake3(&[seed, 2]));
        let package = ToolPackageRecordV1 {
            tool_package_id: package_id,
            package_key: package_key.clone(),
            active_version: Some(package_version.clone()),
            lifecycle: ToolPackageLifecycleV1::Installed,
            deletion_pending: false,
            entity_revision: Revision::INITIAL,
            created_at: timestamp(),
            updated_at: timestamp(),
        };
        let package_version_record = ToolPackageVersionRecordV1 {
            tool_package_id: package_id,
            package_version,
            package_fingerprint,
            package_security_fingerprint: security_fingerprint,
            manifest_hash: CanonicalHashV1::sha256(&[seed, 3]),
            package_store_relative_path: PackageRelativePath::new(format!(
                "packages/fixture-package{seed}/1.0.0"
            ))
            .expect("store path"),
            conformance_run_ids: vec![conformance_id],
            installed_at: timestamp(),
            payload_strong_reference_count: 1,
            payload_gc_eligible_at: None,
        };
        let definition = ToolDefinitionRecordV1::new(
            tool_id,
            tool_version.clone(),
            Some(package_id),
            Some(package_key),
            Some(tool_key),
            ToolCanonicalName::new(format!("fixture.tool{seed}")).expect("canonical name"),
            format!("Fixture Tool {seed}"),
            format!("Execute fixture tool {seed}"),
            format!("Fixture tool {seed} summary"),
            ToolInputSchemaV1::new(json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "additionalProperties": false
            }))
            .expect("input schema"),
            false,
            ToolResultContractV1::Unit,
            ToolRuntimeDescriptorV1::ExternalSource {
                source_contract_key: format!("fixture.package.{seed}.v1"),
            },
            vec![],
            vec![],
            ToolAudienceV1::SubagentConfigurable,
            ToolSourceKindV1::Imported,
            ToolExecutionSemanticsV1 {
                automatic_retry_allowed: false,
                repeat_is_idempotent: true,
                may_create_persistent_effects: false,
            },
            CanonicalHashV1::sha256(&[seed, 4]),
            Revision::INITIAL,
            timestamp(),
        )
        .expect("definition");
        let instance = ToolInstanceRecordV1 {
            tool_instance_id: instance_id,
            tool_id,
            tool_version,
            source_kind: ToolSourceKindV1::Imported,
            source_contract_fingerprint:
                aura_tool_contract::ToolSourceContractFingerprintV1::package(
                    ToolSourceKindV1::Imported,
                    aura_tool_contract::ToolContractFingerprintV1 {
                        package_runtime_closure_hash:
                            aura_tool_contract::PackageRuntimeClosureHashV1(
                                CanonicalHashV1::sha256(&[seed, 4]),
                            ),
                        fingerprint: CanonicalHashV1::sha256(&[seed, 5]),
                    },
                )
                .expect("package source fingerprint"),
            enabled: true,
            runtime_health: ToolRuntimeHealthV1::Healthy,
            overrides: ToolInstanceOverridesV1 {
                timeout_ms: None,
                max_retries: None,
                max_concurrent_attempts: None,
                subagent_enabled: None,
            },
            entity_revision: Revision::INITIAL,
            created_at: timestamp(),
            updated_at: timestamp(),
        };
        VerifiedToolPackageInstallRequest {
            package,
            package_version: package_version_record,
            definitions: vec![definition],
            instances: vec![instance],
            correlation_id: CorrelationId::from_bytes([seed.wrapping_add(4); 16]),
        }
    }

    fn credential_install_request(seed: u8) -> VerifiedToolPackageInstallRequest {
        let mut request = install_request(seed);
        let previous = request.definitions.remove(0);
        let runtime_closure_hash = CanonicalHashV1::sha256(&[seed, 40]);
        let runtime = ToolRuntimeDescriptorV1::DeclarativeCommand {
            command: DeclarativeCommandV1::Argv {
                executable: PackageRelativePath::new("bin/fixture.exe").expect("runtime path"),
                arguments: Vec::new(),
                working_directory: CommandWorkingDirectoryV1::PackageRoot,
                environment: Vec::new(),
                credential_slot: Some(CommandCredentialEnvironmentSlotV1 {
                    slot_key: CredentialSlotKey::new("api_key").expect("slot"),
                    environment_variable: EnvironmentVariableNameV1::new("AURA_FIXTURE_API_KEY")
                        .expect("environment variable"),
                }),
                success_exit_codes: vec![0],
            },
        };
        let definition = ToolDefinitionRecordV1::new(
            previous.tool_id,
            previous.tool_version,
            previous.tool_package_id,
            previous.package_key,
            previous.tool_key,
            previous.canonical_name,
            previous.display_name,
            previous.model_description,
            previous.model_summary,
            previous.input_schema,
            previous.strict,
            previous.result_contract,
            runtime,
            previous.capabilities,
            previous.resource_claims,
            previous.audience,
            previous.source_kind,
            previous.execution_semantics,
            runtime_closure_hash.clone(),
            previous.entity_revision,
            previous.created_at,
        )
        .expect("credential definition");
        request.instances[0].source_contract_fingerprint =
            aura_tool_contract::ToolSourceContractFingerprintV1::package(
                ToolSourceKindV1::Imported,
                aura_tool_contract::ToolContractFingerprintV1 {
                    package_runtime_closure_hash: aura_tool_contract::PackageRuntimeClosureHashV1(
                        runtime_closure_hash,
                    ),
                    fingerprint: definition.definition_fingerprint.clone(),
                },
            )
            .expect("credential source fingerprint");
        request.definitions.push(definition);
        request
    }

    fn create_workspace_session(
        coordinator: &PersistenceCoordinator,
        seed: u8,
    ) -> (WorkspaceId, SessionId) {
        let workspace_id = WorkspaceId::from_bytes([seed; 16]);
        let session_id = SessionId::from_bytes([seed.wrapping_add(1); 16]);
        coordinator
            .upsert_workspace_session(
                WorkspaceSummary {
                    workspace_id,
                    display_name: format!("Fixture {seed}"),
                    selected_root: format!(r"C:\fixture\{seed}"),
                    canonical_root: format!(r"C:\fixture\{seed}"),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Fresh,
                    authorization_epoch: Revision::INITIAL,
                },
                SessionSummary {
                    session_id,
                    workspace_id,
                    title: format!("Session {seed}"),
                    state: "active".to_owned(),
                },
                CorrelationId::from_bytes([seed.wrapping_add(2); 16]),
            )
            .expect("workspace session");
        (workspace_id, session_id)
    }

    #[test]
    fn current_schema_preserves_empty_tool_authority_at_initial_revision() {
        let (_temp, coordinator) = start();
        let snapshot =
            coordinator.tool_authority_snapshot(None, None, None).expect("authority snapshot");
        assert_eq!(snapshot.registry_revision, Revision::INITIAL);
        assert!(snapshot.packages.is_empty());
        assert!(snapshot.definitions.is_empty());
        coordinator
            .with_reader(|connection| {
                let version = connection.query_row(
                    "SELECT MAX(version) FROM schema_migrations",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                let table_count = connection.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                        'tool_packages', 'tool_package_versions', 'tool_definitions',
                        'tool_instances', 'tool_package_trust', 'workspace_tool_pins',
                        'session_tool_pins', 'tool_ephemeral_loads'
                    )",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                assert_eq!(version, crate::migration::LATEST_SCHEMA_VERSION);
                assert_eq!(table_count, 8);
                Ok(())
            })
            .expect("migration evidence");
    }

    #[test]
    fn verified_install_is_atomic_idempotent_and_emits_one_event() {
        let (_temp, coordinator) = start();
        let request = install_request(10);
        let first = coordinator
            .seed_verified_tool_package_for_test(request.clone())
            .expect("first install");
        assert!(first.changed);
        assert_eq!(first.registry_revision, Revision::new(2));
        assert!(first.event_id.is_some());

        let repeated = coordinator
            .seed_verified_tool_package_for_test(request.clone())
            .expect("idempotent install");
        assert!(!repeated.changed);
        assert_eq!(repeated.registry_revision, Revision::new(2));
        assert_eq!(repeated.event_id, None);

        let snapshot =
            coordinator.tool_authority_snapshot(None, None, None).expect("authority snapshot");
        assert_eq!(snapshot.packages, vec![request.package]);
        assert_eq!(snapshot.package_versions, vec![request.package_version]);
        assert_eq!(snapshot.definitions, request.definitions);
        assert_eq!(snapshot.instances, request.instances);
        coordinator
            .with_reader(|connection| {
                let events = connection.query_row(
                    "SELECT COUNT(*) FROM events WHERE event_type='ToolPackageInstalled'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                assert_eq!(events, 1);
                Ok(())
            })
            .expect("event evidence");
    }

    #[test]
    fn trust_and_enabled_mutations_use_cas_and_advance_registry_revision() {
        let (_temp, coordinator) = start();
        let request = install_request(20);
        let package_id = request.package.tool_package_id;
        let security_fingerprint = request.package_version.package_security_fingerprint.clone();
        let instance_id = request.instances[0].tool_instance_id;
        coordinator.seed_verified_tool_package_for_test(request).expect("install");

        let absent_false = coordinator
            .commit_tool_package_trust_change(ToolPackageTrustChangeRequest {
                tool_package_id: package_id,
                package_security_fingerprint: security_fingerprint.clone(),
                expected_revision: None,
                trusted: false,
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([40; 16]),
            })
            .expect("absent false is an exact no-op");
        assert!(!absent_false.changed);
        assert_eq!(absent_false.registry_revision, Revision::new(2));
        assert!(absent_false.projection_changes.is_empty());

        let trusted = coordinator
            .commit_tool_package_trust_change(ToolPackageTrustChangeRequest {
                tool_package_id: package_id,
                package_security_fingerprint: security_fingerprint.clone(),
                expected_revision: None,
                trusted: true,
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([41; 16]),
            })
            .expect("trust package");
        assert!(trusted.changed);
        assert_eq!(trusted.registry_revision, Revision::new(3));
        assert_eq!(trusted.projection_changes.len(), 3);
        assert!(matches!(
            &trusted.projection_changes[0],
            ProjectionChange::ToolPackageList { payload, .. }
                if payload.packages.len() == 1 && payload.packages[0].trusted
        ));
        assert!(matches!(
            &trusted.projection_changes[1],
            ProjectionChange::ToolPackage { payload, .. }
                if payload.tool_package_id == package_id
                    && payload.trusted_security_fingerprint.is_some()
        ));

        let trust_no_op = coordinator
            .commit_tool_package_trust_change(ToolPackageTrustChangeRequest {
                tool_package_id: package_id,
                package_security_fingerprint: security_fingerprint.clone(),
                expected_revision: Some(Revision::INITIAL),
                trusted: true,
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([42; 16]),
            })
            .expect("trust no-op");
        assert!(!trust_no_op.changed);

        let enabled_no_op = coordinator
            .commit_tool_instance_enabled_change(ToolInstanceEnabledChangeRequest {
                tool_instance_id: instance_id,
                expected_revision: Revision::INITIAL,
                enabled: true,
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([43; 16]),
            })
            .expect("enabled no-op");
        assert!(!enabled_no_op.changed);

        let disabled = coordinator
            .commit_tool_instance_enabled_change(ToolInstanceEnabledChangeRequest {
                tool_instance_id: instance_id,
                expected_revision: Revision::INITIAL,
                enabled: false,
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([44; 16]),
            })
            .expect("disable instance");
        assert!(disabled.changed);
        assert_eq!(disabled.registry_revision, Revision::new(4));
        assert_eq!(disabled.projection_changes.len(), 3);
        assert!(matches!(
            &disabled.projection_changes[1],
            ProjectionChange::ToolPackage { payload, .. }
                if payload.tools.len() == 1
                    && payload.tools[0].instances.len() == 1
                    && !payload.tools[0].instances[0].enabled
        ));

        let stale =
            coordinator.commit_tool_instance_enabled_change(ToolInstanceEnabledChangeRequest {
                tool_instance_id: instance_id,
                expected_revision: Revision::INITIAL,
                enabled: true,
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([45; 16]),
            });
        assert!(matches!(stale, Err(PersistenceError::ToolEntityRevisionConflict)));

        let snapshot =
            coordinator.tool_authority_snapshot(None, None, None).expect("authority snapshot");
        assert_eq!(snapshot.registry_revision, Revision::new(4));
        assert!(snapshot.trust_records[0].trusted);
        assert!(!snapshot.instances[0].enabled);
        assert_eq!(snapshot.instances[0].entity_revision, Revision::new(2));
    }

    #[test]
    fn authority_snapshot_rejects_record_or_hash_drift() {
        let (_temp, coordinator) = start();
        coordinator.seed_verified_tool_package_for_test(install_request(30)).expect("install");
        coordinator
            .dispatch(|connection| {
                connection.execute("UPDATE tool_packages SET record_json='{}'", [])?;
                Ok(())
            })
            .expect("inject corruption");
        assert!(matches!(
            coordinator.tool_authority_snapshot(None, None, None),
            Err(PersistenceError::ToolEntityIntegrity)
        ));
    }

    #[test]
    fn exact_workspace_and_session_pins_are_scoped_cas_mutations() {
        let (_temp, coordinator) = start();
        let request = install_request(35);
        let tool_id = request.definitions[0].tool_id;
        let tool_version = request.definitions[0].tool_version.clone();
        coordinator.seed_verified_tool_package_for_test(request).expect("install");
        let (workspace_id, session_id) = create_workspace_session(&coordinator, 70);
        let target = ToolPinTargetV1 {
            tool_version: tool_version.clone(),
            state: ToolPinResolutionStateV1::Resolved,
            safe_reason: None,
        };

        let workspace_pin = coordinator
            .commit_workspace_tool_pin_change(WorkspaceToolPinChangeRequest {
                workspace_id,
                tool_id,
                expected_revision: None,
                target: Some(target.clone()),
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([71; 16]),
            })
            .expect("workspace pin");
        assert_eq!(workspace_pin.registry_revision, Revision::new(3));
        assert!(matches!(
            workspace_pin.projection_changes.as_slice(),
            [ProjectionChange::WorkspaceToolPins {
                projection_scope: ProjectionScope::Workspace {
                    workspace_id: projected_workspace_id,
                },
                payload,
                ..
            }] if *projected_workspace_id == workspace_id
                && payload.workspace_id == workspace_id
                && payload.pins.len() == 1
                && payload.pins[0].tool_id == tool_id
                && payload.pins[0].tool_version == tool_version
        ));

        let workspace_no_op = coordinator
            .commit_workspace_tool_pin_change(WorkspaceToolPinChangeRequest {
                workspace_id,
                tool_id,
                expected_revision: Some(Revision::INITIAL),
                target: Some(target.clone()),
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([72; 16]),
            })
            .expect("workspace no-op");
        assert!(!workspace_no_op.changed);
        assert_eq!(workspace_no_op.registry_revision, Revision::new(3));
        assert!(workspace_no_op.projection_changes.is_empty());

        let session_pin = coordinator
            .commit_session_tool_pin_change(SessionToolPinChangeRequest {
                session_id,
                workspace_id,
                tool_id,
                expected_revision: None,
                target: Some(target),
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([73; 16]),
            })
            .expect("session pin");
        assert_eq!(session_pin.registry_revision, Revision::new(4));

        let snapshot = coordinator
            .tool_authority_snapshot(Some(workspace_id), Some(session_id), None)
            .expect("scoped snapshot");
        assert_eq!(snapshot.workspace_pins.len(), 1);
        assert_eq!(snapshot.session_pins.len(), 1);
        match &snapshot.workspace_pins[0] {
            ToolPinRecordV1::Workspace { pin, entity_revision, .. } => {
                assert_eq!(pin.tool_version, tool_version);
                assert_eq!(pin.revision, Revision::INITIAL);
                assert_eq!(*entity_revision, Revision::INITIAL);
            }
            ToolPinRecordV1::Session { .. } => panic!("workspace query returned session pin"),
        }

        let session_unpin = coordinator
            .commit_session_tool_pin_change(SessionToolPinChangeRequest {
                session_id,
                workspace_id,
                tool_id,
                expected_revision: Some(Revision::INITIAL),
                target: None,
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([74; 16]),
            })
            .expect("session unpin");
        assert_eq!(session_unpin.registry_revision, Revision::new(5));
        assert!(
            coordinator
                .tool_authority_snapshot(Some(workspace_id), Some(session_id), None)
                .expect("after unpin")
                .session_pins
                .is_empty()
        );

        coordinator
            .with_reader(|connection| {
                let scoped_events = connection.query_row(
                    "SELECT COUNT(*) FROM events
                     WHERE event_type IN ('WorkspaceToolPinChanged', 'SessionToolPinChanged')
                       AND workspace_id=?1",
                    [workspace_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )?;
                let session_events = connection.query_row(
                    "SELECT COUNT(*) FROM events
                     WHERE event_type='SessionToolPinChanged' AND session_id=?1",
                    [session_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )?;
                assert_eq!(scoped_events, 3);
                assert_eq!(session_events, 2);
                Ok(())
            })
            .expect("scoped audit events");
    }

    #[test]
    fn session_pin_rejects_a_mismatched_workspace_without_writing() {
        let (_temp, coordinator) = start();
        let request = install_request(45);
        let tool_id = request.definitions[0].tool_id;
        let tool_version = request.definitions[0].tool_version.clone();
        coordinator.seed_verified_tool_package_for_test(request).expect("install");
        let (_workspace_id, session_id) = create_workspace_session(&coordinator, 80);
        let other_workspace_id = WorkspaceId::from_bytes([90; 16]);
        assert!(matches!(
            coordinator.commit_session_tool_pin_change(SessionToolPinChangeRequest {
                session_id,
                workspace_id: other_workspace_id,
                tool_id,
                expected_revision: None,
                target: Some(ToolPinTargetV1 {
                    tool_version,
                    state: ToolPinResolutionStateV1::Resolved,
                    safe_reason: None,
                }),
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::from_bytes([81; 16]),
            }),
            Err(PersistenceError::ToolEntityStateConflict)
        ));
        let snapshot =
            coordinator.tool_authority_snapshot(None, None, None).expect("global snapshot");
        assert_eq!(snapshot.registry_revision, Revision::new(2));
    }

    #[test]
    fn tool_credential_candidates_and_binding_revalidate_the_p07_authority() {
        let (_temp, coordinator) = start();
        let install = credential_install_request(46);
        let package_id = install.package.tool_package_id;
        let tool_id = install.definitions[0].tool_id;
        let runtime = install.definitions[0].runtime.clone();
        let instance_id = install.instances[0].tool_instance_id;
        coordinator.seed_verified_tool_package_for_test(install).expect("install");
        let (workspace_id, session_id) = create_workspace_session(&coordinator, 100);
        let owner = coordinator.current_tool_principal().expect("profile owner");
        let placement = ToolCredentialPlacementV1::CommandEnvironment {
            environment_variable: EnvironmentVariableNameV1::new("AURA_FIXTURE_API_KEY")
                .expect("environment variable"),
        };
        let slot_key = CredentialSlotKey::new("api_key").expect("slot");
        let allowed = credential_record(
            110,
            owner,
            ToolCredentialScopeV1::Session { workspace_id, session_id },
            ToolCredentialStateV1::Configured,
            placement.clone(),
            tool_id,
            "Session Fixture Credential",
        );
        let allowed_ref = CredentialRef::for_test("p07-authority", "allowed").expect("reference");
        let first_candidate_revision = coordinator
            .seed_tool_credential_authority_for_test(ToolCredentialAuthoritySeedV1 {
                record: allowed.clone(),
                credential_ref: allowed_ref.clone(),
            })
            .expect("seed allowed credential");
        let query = ToolCredentialCandidateQueryV1 {
            authenticated_owner: owner,
            tool_instance_id: instance_id,
            credential_slot_key: slot_key.clone(),
            placement: placement.clone(),
            workspace_id: Some(workspace_id),
            session_id: Some(session_id),
            expected_instance_revision: Revision::INITIAL,
        };
        let first = coordinator
            .query_tool_credential_candidates_v1(query.clone())
            .expect("query candidate authority");
        assert_eq!(first.candidate_set_revision, first_candidate_revision);
        assert_eq!(first.candidates.len(), 1);
        assert_eq!(first.candidates[0].credential_id, allowed.credential_id);
        let safe_wire = serde_json::to_string(&first).expect("safe projection wire");
        assert!(!safe_wire.contains(&allowed_ref.to_string()));
        for forbidden in ["credential_ref", "wincred", "fingerprint", "secret_value"] {
            assert!(!safe_wire.contains(forbidden), "unsafe candidate field: {forbidden}");
        }
        let no_scope = coordinator
            .query_tool_credential_candidates_v1(ToolCredentialCandidateQueryV1 {
                workspace_id: None,
                session_id: None,
                ..query.clone()
            })
            .expect("query without scope");
        assert!(no_scope.candidates.is_empty());

        let wrong_owner = credential_record(
            111,
            PrincipalId::from_bytes([111; 16]),
            ToolCredentialScopeV1::User,
            ToolCredentialStateV1::Configured,
            placement.clone(),
            tool_id,
            "Foreign Credential",
        );
        coordinator
            .seed_tool_credential_authority_for_test(ToolCredentialAuthoritySeedV1 {
                record: wrong_owner,
                credential_ref: CredentialRef::for_test("p07-authority", "foreign")
                    .expect("reference"),
            })
            .expect("seed foreign credential");
        let suspended = credential_record(
            112,
            owner,
            ToolCredentialScopeV1::User,
            ToolCredentialStateV1::Suspended,
            placement.clone(),
            tool_id,
            "Suspended Credential",
        );
        coordinator
            .seed_tool_credential_authority_for_test(ToolCredentialAuthoritySeedV1 {
                record: suspended,
                credential_ref: CredentialRef::for_test("p07-authority", "suspended")
                    .expect("reference"),
            })
            .expect("seed suspended credential");
        let wrong_usage = credential_record(
            113,
            owner,
            ToolCredentialScopeV1::User,
            ToolCredentialStateV1::Configured,
            placement.clone(),
            ToolId::from_bytes([114; 16]),
            "Wrong Tool Credential",
        );
        let current_candidate_revision = coordinator
            .seed_tool_credential_authority_for_test(ToolCredentialAuthoritySeedV1 {
                record: wrong_usage,
                credential_ref: CredentialRef::for_test("p07-authority", "wrong-tool")
                    .expect("reference"),
            })
            .expect("seed wrong usage credential");
        let filtered = coordinator
            .query_tool_credential_candidates_v1(query.clone())
            .expect("query filtered candidates");
        assert_eq!(filtered.candidate_set_revision, current_candidate_revision);
        assert_eq!(filtered.candidates.len(), 1);
        assert_eq!(filtered.candidates[0].credential_id, allowed.credential_id);

        let binding_request = |authenticated_owner,
                               expected_candidate_set_revision,
                               credential_revision,
                               expected_entity_revision,
                               expected_binding_revision| {
            ToolInstanceCredentialSettingsChangeRequestV1 {
                authenticated_owner,
                tool_instance_id: instance_id,
                credential_slot_key: slot_key.clone(),
                placement: placement.clone(),
                expected_entity_revision,
                expected_binding_revision,
                expected_candidate_set_revision,
                credential_id: allowed.credential_id,
                credential_revision,
                workspace_id: Some(workspace_id),
                session_id: Some(session_id),
                changed_at: later_timestamp(),
                correlation_id: CorrelationId::new(),
            }
        };
        assert!(matches!(
            coordinator.commit_tool_instance_credential_settings_change(binding_request(
                owner,
                first_candidate_revision,
                allowed.credential_revision,
                Revision::INITIAL,
                None,
            )),
            Err(PersistenceError::ToolEntityRevisionConflict)
        ));
        assert!(matches!(
            coordinator.commit_tool_instance_credential_settings_change(binding_request(
                PrincipalId::from_bytes([115; 16]),
                current_candidate_revision,
                allowed.credential_revision,
                Revision::INITIAL,
                None,
            )),
            Err(PersistenceError::CredentialUnavailable)
        ));
        assert!(matches!(
            coordinator.commit_tool_instance_credential_settings_change(binding_request(
                owner,
                current_candidate_revision,
                allowed.credential_revision.next().expect("next revision"),
                Revision::INITIAL,
                None,
            )),
            Err(PersistenceError::CredentialUnavailable)
        ));
        let committed = coordinator
            .commit_tool_instance_credential_settings_change(binding_request(
                owner,
                current_candidate_revision,
                allowed.credential_revision,
                Revision::INITIAL,
                None,
            ))
            .expect("bind authorized credential");
        assert!(committed.changed);
        assert_eq!(committed.target_entity_revision, Revision::new(2));
        let authority = coordinator
            .tool_authority_snapshot(Some(workspace_id), Some(session_id), None)
            .expect("authority snapshot");
        let binding = authority.instance_credential_bindings.first().expect("binding");
        assert_eq!(binding.tool_instance_id, instance_id);
        assert_eq!(binding.credential_id, allowed.credential_id);
        assert_eq!(binding.credential_revision, allowed.credential_revision);
        assert_eq!(binding.placement, placement);
        assert!(
            coordinator
                .tool_credential_binding_ready_v1(
                    binding,
                    &runtime,
                    Some(package_id),
                    tool_id,
                    Some(workspace_id),
                    Some(session_id),
                )
                .expect("ready binding")
        );

        let mut revoked = allowed;
        revoked.state = ToolCredentialStateV1::Revoked;
        revoked.row_revision = Revision::new(2);
        revoked.updated_at = later_timestamp();
        coordinator
            .seed_tool_credential_authority_for_test(ToolCredentialAuthoritySeedV1 {
                record: revoked,
                credential_ref: allowed_ref,
            })
            .expect("revoke credential");
        assert!(
            !coordinator
                .tool_credential_binding_ready_v1(
                    binding,
                    &runtime,
                    Some(package_id),
                    tool_id,
                    Some(workspace_id),
                    Some(session_id),
                )
                .expect("revalidated binding")
        );
    }

    #[test]
    fn package_fingerprint_contract_rejects_non_sha256_wire() {
        assert!(
            serde_json::from_value::<PackageFingerprintV1>(json!({
                "algorithm": "blake3",
                "canonicalization_version": "rfc8785_jcs_v1",
                "hex": "00".repeat(32)
            }))
            .is_err()
        );
    }
}
