use std::str::FromStr;

use aura_domain_common::{
    CredentialId, CredentialRef, CredentialRevision, PrincipalId, Revision, SessionId,
    UtcTimestamp, WorkspaceId,
};
use aura_domain_common::{ToolId, ToolInstanceId, ToolPackageId};
use aura_tool_contract::{
    CredentialSlotKey, DeclarativeCommandV1, HttpCredentialSlotV1, ToolRuntimeDescriptorV1,
};
use aura_tool_domain::{
    ToolCredentialAuthorityRecordV1, ToolCredentialPlacementV1, ToolCredentialScopeV1,
    ToolCredentialStateV1,
};
use aura_tool_projection::{
    ToolCredentialCandidateCompatibilityV1, ToolCredentialCandidateViewV1,
    ToolCredentialCandidatesProjectionV1, ToolCredentialScopeSummaryV1,
};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    PersistenceCoordinator, PersistenceError,
    tool_entities::{encode_record, load_all_records, load_optional_record, sqlite_revision},
};

const CURRENT_PROFILE_KEY: &str = "current";

#[derive(Clone, Debug)]
pub struct ToolCredentialCandidateQueryV1 {
    pub authenticated_owner: PrincipalId,
    pub tool_instance_id: ToolInstanceId,
    pub credential_slot_key: CredentialSlotKey,
    pub placement: ToolCredentialPlacementV1,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
    pub expected_instance_revision: Revision,
}

#[derive(Clone, Debug)]
pub struct ToolCredentialBindingAuthorizationV1 {
    pub authenticated_owner: PrincipalId,
    pub tool_instance_id: ToolInstanceId,
    pub credential_slot_key: CredentialSlotKey,
    pub placement: ToolCredentialPlacementV1,
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
    pub expected_candidate_set_revision: Revision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedToolCredentialV1 {
    pub record: ToolCredentialAuthorityRecordV1,
    pub credential_ref: CredentialRef,
}

#[derive(Clone, Debug)]
pub struct ToolCredentialAuthoritySeedV1 {
    pub record: ToolCredentialAuthorityRecordV1,
    pub credential_ref: CredentialRef,
}

impl PersistenceCoordinator {
    /// Resolves the local-profile principal after the Main/Core pipe has been
    /// authenticated. The ID is created once per data root and is never
    /// renderer-authored or derived from a per-command hash.
    pub fn current_tool_principal(&self) -> Result<PrincipalId, PersistenceError> {
        self.dispatch(|connection| {
            let transaction = connection.transaction()?;
            let existing = transaction
                .query_row(
                    "SELECT principal_id FROM tool_profile_principals WHERE profile_key=?1",
                    [CURRENT_PROFILE_KEY],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let principal_id = match existing {
                Some(value) => PrincipalId::from_str(&value)?,
                None => {
                    let principal_id = PrincipalId::new();
                    transaction.execute(
                        "INSERT INTO tool_profile_principals(profile_key, principal_id, created_at)
                         VALUES (?1, ?2, ?3)",
                        params![
                            CURRENT_PROFILE_KEY,
                            principal_id.to_string(),
                            UtcTimestamp::now().to_string()
                        ],
                    )?;
                    principal_id
                }
            };
            transaction.commit()?;
            Ok(principal_id)
        })
    }

    pub fn query_tool_credential_candidates_v1(
        &self,
        query: ToolCredentialCandidateQueryV1,
    ) -> Result<ToolCredentialCandidatesProjectionV1, PersistenceError> {
        self.with_reader(|connection| {
            validate_session_scope(connection, query.workspace_id, query.session_id)?;
            let (instance_revision, tool_id, tool_package_id, runtime) = connection
                .query_row(
                    "SELECT i.entity_revision, i.tool_id, d.tool_package_id, d.record_json
                     FROM tool_instances i
                     JOIN tool_definitions d
                       ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json
                     WHERE i.tool_instance_id=?1",
                    [query.tool_instance_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()?
                .ok_or(PersistenceError::ToolEntityNotFound)?;
            let instance_revision = revision_from_i64(instance_revision)?;
            if instance_revision != query.expected_instance_revision {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            let definition: aura_tool_domain::ToolDefinitionRecordV1 =
                serde_json::from_str(&runtime)
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            require_exact_slot(&definition.runtime, &query.credential_slot_key, &query.placement)?;
            let tool_id = tool_id.parse()?;
            let tool_package_id = tool_package_id.map(|value| value.parse()).transpose()?;
            let candidate_set_revision = read_candidate_set_revision(connection)?;
            let mut candidates = load_all_records::<ToolCredentialAuthorityRecordV1>(
                connection,
                "SELECT record_json, record_hash_json FROM tool_credentials
                 ORDER BY credential_id",
            )?
            .into_iter()
            .filter(|record| {
                record.validate().is_ok()
                    && record.authenticated_owner == query.authenticated_owner
                    && record.state == ToolCredentialStateV1::Configured
                    && record.scope_allows(query.workspace_id, query.session_id)
                    && record.placement_policy.allows(&query.placement)
                    && record.usage_policy.allows(tool_package_id, tool_id)
            })
            .map(|record| ToolCredentialCandidateViewV1 {
                credential_id: record.credential_id,
                credential_revision: record.credential_revision,
                display_name: record.display_name,
                credential_kind: record.credential_kind,
                state: record.state,
                compatibility: ToolCredentialCandidateCompatibilityV1::Compatible,
                scope_summary: match record.scope {
                    ToolCredentialScopeV1::User => ToolCredentialScopeSummaryV1::User,
                    ToolCredentialScopeV1::Workspace { .. } => {
                        ToolCredentialScopeSummaryV1::Workspace
                    }
                    ToolCredentialScopeV1::Session { .. } => ToolCredentialScopeSummaryV1::Session,
                },
            })
            .collect::<Vec<_>>();
            candidates.sort_by(|left, right| {
                left.display_name
                    .cmp(&right.display_name)
                    .then(left.credential_id.cmp(&right.credential_id))
            });
            Ok(ToolCredentialCandidatesProjectionV1 {
                tool_instance_id: query.tool_instance_id,
                instance_revision,
                credential_slot_key: query.credential_slot_key,
                placement: query.placement,
                workspace_id: query.workspace_id,
                session_id: query.session_id,
                candidate_set_revision,
                candidates,
            })
        })
    }

    pub(crate) fn authorize_tool_credential_binding_in_transaction(
        transaction: &Transaction<'_>,
        request: &ToolCredentialBindingAuthorizationV1,
    ) -> Result<AuthorizedToolCredentialV1, PersistenceError> {
        validate_session_scope(transaction, request.workspace_id, request.session_id)?;
        if read_candidate_set_revision(transaction)? != request.expected_candidate_set_revision {
            return Err(PersistenceError::ToolEntityRevisionConflict);
        }
        let (tool_id, tool_package_id, definition_json) = transaction
            .query_row(
                "SELECT i.tool_id, d.tool_package_id, d.record_json
                 FROM tool_instances i
                 JOIN tool_definitions d
                   ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json
                 WHERE i.tool_instance_id=?1",
                [request.tool_instance_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
        let definition: aura_tool_domain::ToolDefinitionRecordV1 =
            serde_json::from_str(&definition_json)
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        require_exact_slot(&definition.runtime, &request.credential_slot_key, &request.placement)?;
        let record = transaction
            .query_row(
                "SELECT record_json FROM tool_credentials WHERE credential_id=?1",
                [request.credential_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(PersistenceError::CredentialRecordNotFound)
            .and_then(|encoded| {
                serde_json::from_str::<ToolCredentialAuthorityRecordV1>(&encoded)
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)
            })?;
        record.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let tool_id = tool_id.parse()?;
        let tool_package_id = tool_package_id.map(|value| value.parse()).transpose()?;
        if record.credential_revision != request.credential_revision
            || record.authenticated_owner != request.authenticated_owner
            || record.state != ToolCredentialStateV1::Configured
            || !record.scope_allows(request.workspace_id, request.session_id)
            || !record.placement_policy.allows(&request.placement)
            || !record.usage_policy.allows(tool_package_id, tool_id)
        {
            return Err(PersistenceError::CredentialUnavailable);
        }
        let credential_ref = transaction
            .query_row(
                "SELECT credential_ref FROM tool_credential_secret_bindings
                 WHERE credential_id=?1 AND credential_revision=?2",
                params![
                    request.credential_id.to_string(),
                    i64::try_from(request.credential_revision.value())
                        .map_err(|_| PersistenceError::ToolEntityIntegrity)?
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(PersistenceError::CredentialUnavailable)?
            .parse()
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        Ok(AuthorizedToolCredentialV1 { record, credential_ref })
    }

    pub(crate) fn tool_credential_binding_ready_v1(
        &self,
        binding: &aura_tool_domain::ToolInstanceCredentialBindingRecordV1,
        runtime: &ToolRuntimeDescriptorV1,
        tool_package_id: Option<ToolPackageId>,
        tool_id: ToolId,
        workspace_id: Option<WorkspaceId>,
        session_id: Option<SessionId>,
    ) -> Result<bool, PersistenceError> {
        if binding.validate().is_err() {
            return Ok(false);
        }
        let Some((slot_key, placement)) = exact_runtime_slot(runtime) else {
            return Ok(false);
        };
        if binding.credential_slot_key != slot_key || binding.placement != placement {
            return Ok(false);
        }
        let authenticated_owner = self.current_tool_principal()?;
        let credential_id = binding.credential_id;
        let credential_revision = binding.credential_revision;
        self.with_reader(move |connection| {
            validate_session_scope(connection, workspace_id, session_id)?;
            let record = load_optional_record::<ToolCredentialAuthorityRecordV1, _>(
                connection,
                "SELECT record_json, record_hash_json FROM tool_credentials
                 WHERE credential_id=?1",
                [credential_id.to_string()],
            )?;
            let Some(record) = record else { return Ok(false) };
            if record.validate().is_err()
                || record.credential_revision != credential_revision
                || record.authenticated_owner != authenticated_owner
                || record.state != ToolCredentialStateV1::Configured
                || !record.scope_allows(workspace_id, session_id)
                || !record.placement_policy.allows(&placement)
                || !record.usage_policy.allows(tool_package_id, tool_id)
            {
                return Ok(false);
            }
            let credential_ref = connection
                .query_row(
                    "SELECT credential_ref FROM tool_credential_secret_bindings
                     WHERE credential_id=?1 AND credential_revision=?2",
                    params![
                        credential_id.to_string(),
                        i64::try_from(credential_revision.value())
                            .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            match credential_ref {
                None => Ok(false),
                Some(reference) => {
                    let _: CredentialRef =
                        reference.parse().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
                    Ok(true)
                }
            }
        })
    }

    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn seed_tool_credential_authority_for_test(
        &self,
        seed: ToolCredentialAuthoritySeedV1,
    ) -> Result<Revision, PersistenceError> {
        seed.record.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (record_json, record_hash_json) = encode_record(&seed.record)?;
            transaction.execute(
                "INSERT INTO tool_credentials(
                    credential_id, credential_revision, authenticated_owner, state,
                    row_revision, record_json, record_hash_json, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(credential_id) DO UPDATE SET
                    credential_revision=excluded.credential_revision,
                    authenticated_owner=excluded.authenticated_owner,
                    state=excluded.state,
                    row_revision=excluded.row_revision,
                    record_json=excluded.record_json,
                    record_hash_json=excluded.record_hash_json,
                    updated_at=excluded.updated_at",
                params![
                    seed.record.credential_id.to_string(),
                    i64::try_from(seed.record.credential_revision.value())
                        .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                    seed.record.authenticated_owner.to_string(),
                    credential_state(&seed.record.state),
                    sqlite_revision(seed.record.row_revision)?,
                    record_json,
                    record_hash_json,
                    seed.record.updated_at.to_string(),
                ],
            )?;
            transaction.execute(
                "INSERT INTO tool_credential_secret_bindings(
                    credential_id, credential_revision, credential_ref, updated_at
                 ) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(credential_id) DO UPDATE SET
                    credential_revision=excluded.credential_revision,
                    credential_ref=excluded.credential_ref,
                    updated_at=excluded.updated_at",
                params![
                    seed.record.credential_id.to_string(),
                    i64::try_from(seed.record.credential_revision.value())
                        .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                    seed.credential_ref.to_string(),
                    seed.record.updated_at.to_string(),
                ],
            )?;
            let current = read_candidate_set_revision(&transaction)?;
            let next = current
                .value()
                .checked_add(1)
                .map(Revision::new)
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            transaction.execute(
                "UPDATE tool_credential_authority_state
                 SET candidate_set_revision=?1, updated_at=?2 WHERE singleton=1",
                params![sqlite_revision(next)?, seed.record.updated_at.to_string()],
            )?;
            transaction.commit()?;
            Ok(next)
        })
    }
}

pub(crate) fn validate_session_scope(
    connection: &rusqlite::Connection,
    workspace_id: Option<WorkspaceId>,
    session_id: Option<SessionId>,
) -> Result<(), PersistenceError> {
    if session_id.is_some() && workspace_id.is_none() {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    if let Some(session_id) = session_id {
        let actual = connection
            .query_row(
                "SELECT workspace_id FROM sessions WHERE session_id=?1",
                [session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
        if workspace_id.is_none_or(|expected| expected.to_string() != actual) {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
    }
    Ok(())
}

fn read_candidate_set_revision(
    connection: &rusqlite::Connection,
) -> Result<Revision, PersistenceError> {
    let value = connection.query_row(
        "SELECT candidate_set_revision FROM tool_credential_authority_state WHERE singleton=1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    revision_from_i64(value)
}

fn revision_from_i64(value: i64) -> Result<Revision, PersistenceError> {
    let value = u64::try_from(value).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let revision = Revision::new(value);
    (revision.value() > 0).then_some(revision).ok_or(PersistenceError::ToolEntityIntegrity)
}

pub(crate) fn require_exact_slot(
    runtime: &ToolRuntimeDescriptorV1,
    slot_key: &CredentialSlotKey,
    placement: &ToolCredentialPlacementV1,
) -> Result<(), PersistenceError> {
    let expected = exact_runtime_slot(runtime).ok_or(PersistenceError::ToolEntityStateConflict)?;
    if expected.0 != *slot_key || expected.1 != *placement {
        return Err(PersistenceError::CredentialUnavailable);
    }
    Ok(())
}

pub(crate) fn exact_runtime_slot(
    runtime: &ToolRuntimeDescriptorV1,
) -> Option<(CredentialSlotKey, ToolCredentialPlacementV1)> {
    let command_slot = |slot: &aura_tool_contract::CommandCredentialEnvironmentSlotV1| {
        (
            slot.slot_key.clone(),
            ToolCredentialPlacementV1::CommandEnvironment {
                environment_variable: slot.environment_variable.clone(),
            },
        )
    };
    match runtime {
        ToolRuntimeDescriptorV1::DeclarativeCommand { command } => match command {
            DeclarativeCommandV1::Argv { credential_slot, .. }
            | DeclarativeCommandV1::PowershellScript { credential_slot, .. } => {
                credential_slot.as_ref().map(command_slot)
            }
        },
        ToolRuntimeDescriptorV1::AuraStdio { credential_slot, .. } => {
            credential_slot.as_ref().map(command_slot)
        }
        ToolRuntimeDescriptorV1::DeclarativeHttp { request } => {
            request.credential_slot.as_ref().map(|slot| match slot {
                HttpCredentialSlotV1::BearerAuthorization { slot_key } => {
                    (slot_key.clone(), ToolCredentialPlacementV1::HttpBearerAuthorization)
                }
                HttpCredentialSlotV1::FixedHeader { slot_key, header_name } => (
                    slot_key.clone(),
                    ToolCredentialPlacementV1::HttpFixedHeader { header_name: header_name.clone() },
                ),
            })
        }
        ToolRuntimeDescriptorV1::ExternalSource { source_contract_key }
            if source_contract_key == "aura.builtin.http.request.v1" =>
        {
            Some((
                CredentialSlotKey::new("api_key").expect("built-in credential slot"),
                ToolCredentialPlacementV1::HttpBearerAuthorization,
            ))
        }
        ToolRuntimeDescriptorV1::ExternalSource { .. } => None,
    }
}

pub(crate) fn credential_state(state: &ToolCredentialStateV1) -> &'static str {
    match state {
        ToolCredentialStateV1::Configured => "configured",
        ToolCredentialStateV1::Suspended => "suspended",
        ToolCredentialStateV1::Revoked => "revoked",
        ToolCredentialStateV1::RecoveryRequired => "recovery_required",
    }
}
