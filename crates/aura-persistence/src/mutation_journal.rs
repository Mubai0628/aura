use std::{
    collections::{BTreeMap, btree_map::Entry},
    str::FromStr,
    sync::Arc,
};

use aura_domain_common::{ErrorCode, MutationJournalId, RuntimeInstanceId, UtcTimestamp};
use aura_projection_contract::ProjectionChange;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    ModelProjectionDraft, PersistenceCoordinator, PersistenceError, PersistentEventDraft,
    coordinator::insert_event, model_projection::insert_model_projections,
};

#[derive(Clone, Debug, PartialEq)]
pub struct MutationJournalRecord {
    pub journal_id: MutationJournalId,
    pub journal_kind: String,
    pub idempotency_key: Option<String>,
    pub state: String,
    pub payload: serde_json::Value,
    pub owner_runtime_instance_id: RuntimeInstanceId,
    pub lease_epoch: u64,
    pub error_code: Option<ErrorCode>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrepareMutationJournal {
    pub journal_id: MutationJournalId,
    pub journal_kind: String,
    pub idempotency_key: Option<String>,
    pub initial_state: String,
    pub payload: serde_json::Value,
    pub resource_kind: String,
    pub resource_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MutationJournalMetadataCommit {
    pub journal: MutationJournalRecord,
    pub event_row_ids: Vec<i64>,
    pub projection_changes: Vec<ProjectionChange>,
    pub projection_outbox_ids: Vec<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MutationJournalMetadataTransition<'a> {
    pub journal_kind: &'a str,
    pub expected_state: &'a str,
    pub next_state: &'a str,
}

impl PrepareMutationJournal {
    fn validate(&self) -> Result<(), PersistenceError> {
        for (label, value) in [
            ("journal kind", self.journal_kind.as_str()),
            ("journal initial state", self.initial_state.as_str()),
            ("journal resource kind", self.resource_kind.as_str()),
            ("journal resource id", self.resource_id.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(PersistenceError::InvalidRequest(format!(
                    "mutation {label} must not be blank"
                )));
            }
        }
        if self.idempotency_key.as_ref().is_some_and(|value| value.trim().is_empty()) {
            return Err(PersistenceError::InvalidRequest(
                "mutation idempotency key must not be blank".to_owned(),
            ));
        }
        Ok(())
    }
}

pub trait MutationJournalRecoveryHandler: Send + Sync {
    fn journal_kind(&self) -> &'static str;

    fn recover(&self, journal: MutationJournalRecord) -> Result<(), PersistenceError>;
}

#[derive(Default)]
pub struct MutationJournalRecoveryRegistry {
    handlers: BTreeMap<&'static str, Arc<dyn MutationJournalRecoveryHandler>>,
}

impl MutationJournalRecoveryRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        handler: Arc<dyn MutationJournalRecoveryHandler>,
    ) -> Result<(), PersistenceError> {
        let kind = handler.journal_kind();
        if kind.trim().is_empty() {
            return Err(PersistenceError::InvalidRequest(
                "mutation recovery handler kind must not be blank".to_owned(),
            ));
        }
        match self.handlers.entry(kind) {
            Entry::Vacant(entry) => {
                entry.insert(handler);
            }
            Entry::Occupied(_) => {
                return Err(PersistenceError::MutationRecoveryHandlerConflict);
            }
        }
        Ok(())
    }

    pub fn recover_active(
        &self,
        persistence: &PersistenceCoordinator,
    ) -> Result<(), PersistenceError> {
        for journal in persistence.active_mutation_journals()? {
            let handler = self.handlers.get(journal.journal_kind.as_str()).ok_or_else(|| {
                PersistenceError::MutationRecoveryHandlerMissing {
                    journal_kind: journal.journal_kind.clone(),
                }
            })?;
            handler.recover(journal)?;
        }
        Ok(())
    }
}

impl PersistenceCoordinator {
    pub fn prepare_mutation_journal(
        &self,
        input: PrepareMutationJournal,
    ) -> Result<MutationJournalRecord, PersistenceError> {
        input.validate()?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let journal =
                insert_prepared_mutation_journal(&transaction, runtime_instance_id, &input)?;
            transaction.commit()?;
            Ok(journal)
        })
    }

    pub fn mutation_journal(
        &self,
        journal_id: MutationJournalId,
    ) -> Result<Option<MutationJournalRecord>, PersistenceError> {
        self.with_reader(|connection| load_mutation_journal(connection, journal_id))
    }

    pub fn mutation_journal_by_idempotency_key(
        &self,
        journal_kind: &str,
        idempotency_key: &str,
    ) -> Result<Option<MutationJournalRecord>, PersistenceError> {
        if journal_kind.trim().is_empty() || idempotency_key.trim().is_empty() {
            return Err(PersistenceError::InvalidRequest(
                "mutation journal kind and idempotency key must not be blank".to_owned(),
            ));
        }
        let journal_kind = journal_kind.to_owned();
        let idempotency_key = idempotency_key.to_owned();
        self.with_reader(|connection| {
            load_mutation_journal_by_idempotency_key(connection, &journal_kind, &idempotency_key)
        })
    }

    pub fn active_mutation_journals(&self) -> Result<Vec<MutationJournalRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT journal_id
                 FROM mutation_journals
                 WHERE state NOT IN ('completed', 'aborted', 'reconciliation_required')
                 ORDER BY created_at, journal_id",
            )?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|value| {
                    let journal_id = MutationJournalId::from_str(&value)?;
                    load_mutation_journal(connection, journal_id)?
                        .ok_or(PersistenceError::MutationJournalNotFound)
                })
                .collect()
        })
    }

    pub fn adopt_mutation_journal_lease(
        &self,
        journal_id: MutationJournalId,
        expected_journal_kind: &'static str,
        resource_kind: &'static str,
        resource_id: String,
    ) -> Result<MutationJournalRecord, PersistenceError> {
        if expected_journal_kind.trim().is_empty()
            || resource_kind.trim().is_empty()
            || resource_id.trim().is_empty()
        {
            return Err(PersistenceError::InvalidRequest(
                "mutation recovery lease identity must not be blank".to_owned(),
            ));
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let journal = load_mutation_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            if journal.journal_kind != expected_journal_kind {
                return Err(PersistenceError::MutationJournalKindConflict);
            }
            let existing = transaction
                .query_row(
                    "SELECT journal_id, runtime_instance_id, lease_epoch
                     FROM mutation_leases
                     WHERE resource_kind=?1 AND resource_id=?2",
                    params![resource_kind, resource_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
                .optional()?;
            if let Some((leased_journal, leased_runtime, leased_epoch)) = existing {
                if leased_journal == journal_id.to_string()
                    && leased_runtime == runtime_instance_id.to_string()
                    && from_sql_epoch(leased_epoch)? == journal.lease_epoch
                {
                    transaction.commit()?;
                    return Ok(journal);
                }
                return Err(PersistenceError::MutationLeaseConflict);
            }

            let lease_epoch = next_lease_epoch(&transaction)?;
            let now = UtcTimestamp::now();
            let changed = transaction.execute(
                "UPDATE mutation_journals
                 SET owner_runtime_instance_id=?1, lease_epoch=?2, updated_at=?3
                 WHERE journal_id=?4 AND journal_kind=?5 AND state NOT IN (
                    'completed', 'aborted', 'reconciliation_required'
                 )",
                params![
                    runtime_instance_id.to_string(),
                    to_sql_epoch(lease_epoch)?,
                    now.to_string(),
                    journal_id.to_string(),
                    expected_journal_kind,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::MutationJournalStateConflict);
            }
            transaction.execute(
                "INSERT INTO mutation_leases(
                    resource_kind, resource_id, journal_id, runtime_instance_id,
                    lease_epoch, acquired_at, heartbeat_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    resource_kind,
                    resource_id,
                    journal_id.to_string(),
                    runtime_instance_id.to_string(),
                    to_sql_epoch(lease_epoch)?,
                    now.to_string(),
                ],
            )?;
            let adopted = load_mutation_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            transaction.commit()?;
            Ok(adopted)
        })
    }

    pub fn transition_mutation_journal(
        &self,
        journal_id: MutationJournalId,
        expected_journal_kind: &str,
        expected_state: &str,
        next_state: &str,
        error_code: Option<ErrorCode>,
    ) -> Result<MutationJournalRecord, PersistenceError> {
        validate_transition_labels(expected_journal_kind, expected_state, next_state)?;
        let expected_journal_kind = expected_journal_kind.to_owned();
        let expected_state = expected_state.to_owned();
        let next_state = next_state.to_owned();
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let journal = load_mutation_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            assert_current_mutation_lease(&transaction, &journal, runtime_instance_id)?;
            let now = UtcTimestamp::now();
            let changed = transaction.execute(
                "UPDATE mutation_journals
                 SET state=?1, error_code=?2, updated_at=?3
                 WHERE journal_id=?4 AND journal_kind=?5 AND state=?6
                   AND owner_runtime_instance_id=?7 AND lease_epoch=?8",
                params![
                    next_state,
                    error_code.as_ref().map(ToString::to_string),
                    now.to_string(),
                    journal_id.to_string(),
                    expected_journal_kind,
                    expected_state,
                    runtime_instance_id.to_string(),
                    to_sql_epoch(journal.lease_epoch)?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::MutationJournalStateConflict);
            }
            if is_terminal_mutation_state(&next_state) {
                release_mutation_lease(&transaction, journal_id, runtime_instance_id)?;
            }
            let updated = load_mutation_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            transaction.commit()?;
            Ok(updated)
        })
    }

    pub fn commit_mutation_journal_metadata(
        &self,
        journal_id: MutationJournalId,
        expected_journal_kind: &str,
        expected_state: &str,
        next_state: &str,
        events: Vec<PersistentEventDraft>,
        projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<MutationJournalMetadataCommit, PersistenceError> {
        self.commit_mutation_journal_metadata_with(
            journal_id,
            MutationJournalMetadataTransition {
                journal_kind: expected_journal_kind,
                expected_state,
                next_state,
            },
            events,
            projection_drafts,
            |_| Ok((Vec::new(), Vec::new())),
        )
    }

    pub(crate) fn commit_mutation_journal_metadata_with<F>(
        &self,
        journal_id: MutationJournalId,
        transition: MutationJournalMetadataTransition<'_>,
        events: Vec<PersistentEventDraft>,
        projection_drafts: Vec<ModelProjectionDraft>,
        metadata_mutation: F,
    ) -> Result<MutationJournalMetadataCommit, PersistenceError>
    where
        F: FnOnce(
                &rusqlite::Transaction<'_>,
            ) -> Result<(Vec<ProjectionChange>, Vec<i64>), PersistenceError>
            + Send
            + 'static,
    {
        validate_transition_labels(
            transition.journal_kind,
            transition.expected_state,
            transition.next_state,
        )?;
        let expected_journal_kind = transition.journal_kind.to_owned();
        let expected_state = transition.expected_state.to_owned();
        let next_state = transition.next_state.to_owned();
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let journal = load_mutation_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            assert_current_mutation_lease(&transaction, &journal, runtime_instance_id)?;
            let changed = transaction.execute(
                "UPDATE mutation_journals
                 SET state=?1, error_code=NULL, updated_at=?2
                 WHERE journal_id=?3 AND journal_kind=?4 AND state=?5
                   AND owner_runtime_instance_id=?6 AND lease_epoch=?7",
                params![
                    next_state,
                    UtcTimestamp::now().to_string(),
                    journal_id.to_string(),
                    expected_journal_kind,
                    expected_state,
                    runtime_instance_id.to_string(),
                    to_sql_epoch(journal.lease_epoch)?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::MutationJournalStateConflict);
            }
            let (mut projection_changes, mut projection_outbox_ids) =
                metadata_mutation(&transaction)?;
            let mut event_row_ids = Vec::with_capacity(events.len());
            for event in events {
                event_row_ids.push(insert_event(&transaction, &event)?);
            }
            let (mut model_projection_changes, mut model_projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            projection_changes.append(&mut model_projection_changes);
            projection_outbox_ids.append(&mut model_projection_outbox_ids);
            if is_terminal_mutation_state(&next_state) {
                release_mutation_lease(&transaction, journal_id, runtime_instance_id)?;
            }
            let updated = load_mutation_journal(&transaction, journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)?;
            transaction.commit()?;
            Ok(MutationJournalMetadataCommit {
                journal: updated,
                event_row_ids,
                projection_changes,
                projection_outbox_ids,
            })
        })
    }
}

fn validate_transition_labels(
    journal_kind: &str,
    expected_state: &str,
    next_state: &str,
) -> Result<(), PersistenceError> {
    for (label, value) in [
        ("journal kind", journal_kind),
        ("expected journal state", expected_state),
        ("next journal state", next_state),
    ] {
        if value.trim().is_empty() {
            return Err(PersistenceError::InvalidRequest(format!(
                "mutation {label} must not be blank"
            )));
        }
    }
    Ok(())
}

pub(crate) fn assert_current_mutation_lease(
    connection: &Connection,
    journal: &MutationJournalRecord,
    runtime_instance_id: RuntimeInstanceId,
) -> Result<(), PersistenceError> {
    let lease = connection
        .query_row(
            "SELECT runtime_instance_id, lease_epoch
             FROM mutation_leases WHERE journal_id=?1",
            [journal.journal_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((leased_runtime, leased_epoch)) = lease else {
        return Err(PersistenceError::MutationLeaseConflict);
    };
    if journal.owner_runtime_instance_id != runtime_instance_id
        || leased_runtime != runtime_instance_id.to_string()
        || from_sql_epoch(leased_epoch)? != journal.lease_epoch
    {
        return Err(PersistenceError::MutationLeaseConflict);
    }
    Ok(())
}

pub(crate) fn load_mutation_journal_by_idempotency_key(
    connection: &Connection,
    journal_kind: &str,
    idempotency_key: &str,
) -> Result<Option<MutationJournalRecord>, PersistenceError> {
    let journal_id = connection
        .query_row(
            "SELECT journal_id
             FROM mutation_journals
             WHERE journal_kind=?1 AND idempotency_key=?2",
            params![journal_kind, idempotency_key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    journal_id
        .map(|value| {
            let journal_id = MutationJournalId::from_str(&value)?;
            load_mutation_journal(connection, journal_id)?
                .ok_or(PersistenceError::MutationJournalNotFound)
        })
        .transpose()
}

pub(crate) fn release_mutation_lease(
    connection: &Connection,
    journal_id: MutationJournalId,
    runtime_instance_id: RuntimeInstanceId,
) -> Result<(), PersistenceError> {
    let removed = connection.execute(
        "DELETE FROM mutation_leases
         WHERE journal_id=?1 AND runtime_instance_id=?2",
        params![journal_id.to_string(), runtime_instance_id.to_string()],
    )?;
    if removed != 1 {
        return Err(PersistenceError::MutationLeaseConflict);
    }
    Ok(())
}

fn is_terminal_mutation_state(state: &str) -> bool {
    matches!(state, "completed" | "aborted" | "reconciliation_required")
}

pub(crate) fn clear_stale_mutation_leases(
    connection: &mut Connection,
    runtime_instance_id: RuntimeInstanceId,
) -> Result<usize, PersistenceError> {
    let removed = connection.execute(
        "DELETE FROM mutation_leases WHERE runtime_instance_id<>?1",
        [runtime_instance_id.to_string()],
    )?;
    Ok(removed)
}

pub(crate) fn insert_prepared_mutation_journal(
    connection: &Connection,
    runtime_instance_id: RuntimeInstanceId,
    input: &PrepareMutationJournal,
) -> Result<MutationJournalRecord, PersistenceError> {
    input.validate()?;
    let lease_epoch = next_lease_epoch(connection)?;
    let now = UtcTimestamp::now();
    connection.execute(
        "INSERT INTO mutation_journals(
            journal_id, journal_kind, idempotency_key, state, payload_json,
            owner_runtime_instance_id, lease_epoch, error_code,
            created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?8)",
        params![
            input.journal_id.to_string(),
            input.journal_kind,
            input.idempotency_key,
            input.initial_state,
            serde_json::to_string(&input.payload)?,
            runtime_instance_id.to_string(),
            to_sql_epoch(lease_epoch)?,
            now.to_string(),
        ],
    )?;
    connection.execute(
        "INSERT INTO mutation_leases(
            resource_kind, resource_id, journal_id, runtime_instance_id,
            lease_epoch, acquired_at, heartbeat_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            input.resource_kind,
            input.resource_id,
            input.journal_id.to_string(),
            runtime_instance_id.to_string(),
            to_sql_epoch(lease_epoch)?,
            now.to_string(),
        ],
    )?;
    load_mutation_journal(connection, input.journal_id)?
        .ok_or(PersistenceError::MutationJournalNotFound)
}

pub(crate) fn load_mutation_journal(
    connection: &Connection,
    journal_id: MutationJournalId,
) -> Result<Option<MutationJournalRecord>, PersistenceError> {
    let raw = connection
        .query_row(
            "SELECT journal_id, journal_kind, idempotency_key, state, payload_json,
                    owner_runtime_instance_id, lease_epoch, error_code,
                    created_at, updated_at
             FROM mutation_journals WHERE journal_id=?1",
            [journal_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )
        .optional()?;
    raw.map(
        |(
            journal_id,
            journal_kind,
            idempotency_key,
            state,
            payload_json,
            owner_runtime_instance_id,
            lease_epoch,
            error_code,
            created_at,
            updated_at,
        )| {
            Ok(MutationJournalRecord {
                journal_id: MutationJournalId::from_str(&journal_id)?,
                journal_kind,
                idempotency_key,
                state,
                payload: serde_json::from_str(&payload_json)?,
                owner_runtime_instance_id: RuntimeInstanceId::from_str(&owner_runtime_instance_id)?,
                lease_epoch: from_sql_epoch(lease_epoch)?,
                error_code: error_code
                    .map(|value| {
                        ErrorCode::from_str(&value).map_err(|_| {
                            PersistenceError::InvalidRequest(
                                "persisted mutation journal error code is invalid".to_owned(),
                            )
                        })
                    })
                    .transpose()?,
                created_at: UtcTimestamp::from_str(&created_at).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "persisted mutation journal created_at is invalid".to_owned(),
                    )
                })?,
                updated_at: UtcTimestamp::from_str(&updated_at).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "persisted mutation journal updated_at is invalid".to_owned(),
                    )
                })?,
            })
        },
    )
    .transpose()
}

fn next_lease_epoch(connection: &Connection) -> Result<u64, PersistenceError> {
    let current = connection.query_row(
        "SELECT COALESCE(MAX(lease_epoch), 0) FROM mutation_journals",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let current = from_sql_epoch_allow_zero(current)?;
    current
        .checked_add(1)
        .ok_or_else(|| PersistenceError::InvalidRequest("mutation lease epoch overflow".to_owned()))
}

fn to_sql_epoch(value: u64) -> Result<i64, PersistenceError> {
    if value == 0 {
        return Err(PersistenceError::InvalidRequest(
            "mutation lease epoch must be positive".to_owned(),
        ));
    }
    i64::try_from(value).map_err(|_| {
        PersistenceError::InvalidRequest("mutation lease epoch exceeds SQLite range".to_owned())
    })
}

fn from_sql_epoch(value: i64) -> Result<u64, PersistenceError> {
    let value = from_sql_epoch_allow_zero(value)?;
    if value == 0 {
        return Err(PersistenceError::InvalidRequest(
            "persisted mutation lease epoch must be positive".to_owned(),
        ));
    }
    Ok(value)
}

fn from_sql_epoch_allow_zero(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| {
        PersistenceError::InvalidRequest("persisted mutation lease epoch is negative".to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PersistenceOptions, PersistenceStartup};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingRecoveryHandler {
        calls: Arc<AtomicUsize>,
    }

    impl MutationJournalRecoveryHandler for CountingRecoveryHandler {
        fn journal_kind(&self) -> &'static str {
            "test_recovery"
        }

        fn recover(&self, _journal: MutationJournalRecord) -> Result<(), PersistenceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn start(
        runtime_instance_id: RuntimeInstanceId,
    ) -> (tempfile::TempDir, PersistenceCoordinator) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            directory.path().join("data/aura.db"),
            directory.path().join("data/artifacts"),
            runtime_instance_id,
        ))
        .expect("start persistence")
        {
            PersistenceStartup::Ready(coordinator) => coordinator,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected recovery: {}", recovery.reason)
            }
        };
        (directory, persistence)
    }

    #[test]
    fn stale_runtime_lease_is_removed_and_journal_can_be_adopted() {
        let first_runtime = RuntimeInstanceId::new();
        let (directory, first) = start(first_runtime);
        let journal_id = MutationJournalId::new();
        first
            .prepare_mutation_journal(PrepareMutationJournal {
                journal_id,
                journal_kind: "test_recovery".to_owned(),
                idempotency_key: Some("test-command-1".to_owned()),
                initial_state: "prepared".to_owned(),
                payload: serde_json::json!({"public_value":"safe"}),
                resource_kind: "test_resource".to_owned(),
                resource_id: "resource-1".to_owned(),
            })
            .expect("prepare journal");
        drop(first);

        let second_runtime = RuntimeInstanceId::new();
        let second = match PersistenceCoordinator::start(PersistenceOptions::new(
            directory.path().join("data/aura.db"),
            directory.path().join("data/artifacts"),
            second_runtime,
        ))
        .expect("restart persistence")
        {
            PersistenceStartup::Ready(coordinator) => coordinator,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected recovery: {}", recovery.reason)
            }
        };
        let adopted = second
            .adopt_mutation_journal_lease(
                journal_id,
                "test_recovery",
                "test_resource",
                "resource-1".to_owned(),
            )
            .expect("adopt stale journal");
        assert_eq!(adopted.owner_runtime_instance_id, second_runtime);
        assert!(adopted.lease_epoch > 1);
    }

    #[test]
    fn duplicate_recovery_registration_is_rejected_without_replacing_handler() {
        let (_directory, persistence) = start(RuntimeInstanceId::new());
        persistence
            .prepare_mutation_journal(PrepareMutationJournal {
                journal_id: MutationJournalId::new(),
                journal_kind: "test_recovery".to_owned(),
                idempotency_key: None,
                initial_state: "prepared".to_owned(),
                payload: serde_json::json!({"public_value":"safe"}),
                resource_kind: "test_resource".to_owned(),
                resource_id: "resource-1".to_owned(),
            })
            .expect("prepare journal");

        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let mut registry = MutationJournalRecoveryRegistry::new();
        registry
            .register(Arc::new(CountingRecoveryHandler { calls: Arc::clone(&first_calls) }))
            .expect("register first handler");
        let duplicate = registry
            .register(Arc::new(CountingRecoveryHandler { calls: Arc::clone(&second_calls) }));
        assert!(matches!(duplicate, Err(PersistenceError::MutationRecoveryHandlerConflict)));

        registry.recover_active(&persistence).expect("recover active journals");
        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(Ordering::SeqCst), 0);
    }
}
