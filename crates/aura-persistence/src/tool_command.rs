use std::str::FromStr;

use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256, parse_json_strict};
use aura_domain_common::{CommandInvocationId, ContentHash, CorrelationId, UtcTimestamp};
use aura_domain_common::{ToolConformanceRunId, ToolDraftId};
use aura_runtime_contract::IpcErrorEnvelope;
use aura_tool_domain::{TOOL_MUTATION_METHODS_V1, ToolMutationResultV1};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{PersistenceCoordinator, PersistenceError};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolMutationCommandJournalStateV1 {
    Prepared,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolMutationCommandSeedV1 {
    pub original_correlation_id: CorrelationId,
    pub reserved_tool_draft_id: Option<ToolDraftId>,
    pub reserved_tool_conformance_run_id: Option<ToolConformanceRunId>,
    pub issued_at: UtcTimestamp,
}

impl ToolMutationCommandSeedV1 {
    /// No retained tool settings command reserves an id. The two fields stay because
    /// journals written before the Draft and Conformance commands were retired carry them.
    fn for_method(correlation_id: CorrelationId, issued_at: UtcTimestamp) -> Self {
        Self {
            original_correlation_id: correlation_id,
            reserved_tool_draft_id: None,
            reserved_tool_conformance_run_id: None,
            issued_at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "outcome_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolMutationCommandOutcomeV1 {
    Succeeded { result: ToolMutationResultV1 },
    Failed { error: IpcErrorEnvelope },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolMutationCommandJournalRecordV1 {
    pub command_invocation_id: CommandInvocationId,
    pub method_name: String,
    pub request: Value,
    pub request_hash: CanonicalSha256HashV1,
    pub authenticated_identity_hash: ContentHash,
    pub seed: ToolMutationCommandSeedV1,
    pub state: ToolMutationCommandJournalStateV1,
    pub outcome: Option<ToolMutationCommandOutcomeV1>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug)]
pub struct PrepareToolMutationCommandV1 {
    pub command_invocation_id: CommandInvocationId,
    pub method_name: String,
    pub request: Value,
    pub authenticated_identity_hash: ContentHash,
    pub correlation_id: CorrelationId,
    pub prepared_at: UtcTimestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PreparedToolMutationCommandV1 {
    Prepared(ToolMutationCommandJournalRecordV1),
    Existing(ToolMutationCommandJournalRecordV1),
}

impl PreparedToolMutationCommandV1 {
    #[must_use]
    pub const fn record(&self) -> &ToolMutationCommandJournalRecordV1 {
        match self {
            Self::Prepared(record) | Self::Existing(record) => record,
        }
    }
}

impl PersistenceCoordinator {
    pub fn prepare_tool_mutation_command_v1(
        &self,
        request: PrepareToolMutationCommandV1,
    ) -> Result<PreparedToolMutationCommandV1, PersistenceError> {
        if !TOOL_MUTATION_METHODS_V1.contains(&request.method_name.as_str())
            || !request.request.is_object()
            || request.authenticated_identity_hash.algorithm
                != aura_domain_common::ContentHashAlgorithm::Sha256
        {
            return Err(PersistenceError::InvalidRequest(
                "invalid authenticated Tool mutation command".to_owned(),
            ));
        }
        let request_hash = canonical_sha256(&request.request)
            .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?;
        let command_invocation_id = request.command_invocation_id;
        let method_name = request.method_name;
        let request_value = request.request;
        let identity_hash = request.authenticated_identity_hash;
        let correlation_id = request.correlation_id;
        let prepared_at = request.prepared_at;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            if let Some(existing) = load_tool_mutation_command(&transaction, command_invocation_id)?
            {
                if existing.method_name != method_name || existing.request_hash != request_hash {
                    return Err(PersistenceError::ToolMutationCommandIdempotencyConflict);
                }
                transaction.commit()?;
                return Ok(PreparedToolMutationCommandV1::Existing(existing));
            }
            let seed = ToolMutationCommandSeedV1::for_method(correlation_id, prepared_at);
            let request_json = canonical_json_string(&request_value)?;
            let seed_json = canonical_json_string(&seed)?;
            let seed_hash = canonical_sha256(&seed)
                .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?;
            transaction.execute(
                "INSERT INTO tool_mutation_command_journals(
                    command_invocation_id, method_name, request_json, request_hash_json,
                    authenticated_identity_hash_json, seed_json, seed_hash_json, state,
                    outcome_json, outcome_hash_json, created_at, updated_at,
                    original_correlation_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'prepared', NULL, NULL, ?8, ?8, ?9)",
                params![
                    command_invocation_id.to_string(),
                    method_name,
                    request_json,
                    serde_json::to_string(&request_hash)?,
                    serde_json::to_string(&identity_hash)?,
                    seed_json,
                    serde_json::to_string(&seed_hash)?,
                    prepared_at.to_string(),
                    correlation_id.to_string(),
                ],
            )?;
            let record = ToolMutationCommandJournalRecordV1 {
                command_invocation_id,
                method_name,
                request: request_value,
                request_hash,
                authenticated_identity_hash: identity_hash,
                seed,
                state: ToolMutationCommandJournalStateV1::Prepared,
                outcome: None,
                created_at: prepared_at,
                updated_at: prepared_at,
            };
            transaction.commit()?;
            Ok(PreparedToolMutationCommandV1::Prepared(record))
        })
    }

    pub fn complete_tool_mutation_command_v1(
        &self,
        command_invocation_id: CommandInvocationId,
        request_hash: CanonicalSha256HashV1,
        outcome: ToolMutationCommandOutcomeV1,
        completed_at: UtcTimestamp,
    ) -> Result<ToolMutationCommandJournalRecordV1, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_tool_mutation_command(&transaction, command_invocation_id)?
                .ok_or(PersistenceError::ToolEntityNotFound)?;
            if current.request_hash != request_hash {
                return Err(PersistenceError::ToolMutationCommandIdempotencyConflict);
            }
            if let Some(existing) = &current.outcome {
                if existing == &outcome {
                    transaction.commit()?;
                    return Ok(current);
                }
                return Err(PersistenceError::ToolMutationCommandJournalStateConflict);
            }
            if current.state != ToolMutationCommandJournalStateV1::Prepared {
                return Err(PersistenceError::ToolMutationCommandJournalStateConflict);
            }
            let state = match &outcome {
                ToolMutationCommandOutcomeV1::Succeeded { .. } => {
                    ToolMutationCommandJournalStateV1::Succeeded
                }
                ToolMutationCommandOutcomeV1::Failed { .. } => {
                    ToolMutationCommandJournalStateV1::Failed
                }
            };
            let outcome_json = canonical_json_string(&outcome)?;
            let outcome_hash = canonical_sha256(&outcome)
                .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?;
            let changed = transaction.execute(
                "UPDATE tool_mutation_command_journals
                 SET state=?1, outcome_json=?2, outcome_hash_json=?3, updated_at=?4
                 WHERE command_invocation_id=?5 AND state='prepared'",
                params![
                    journal_state_name(state),
                    outcome_json,
                    serde_json::to_string(&outcome_hash)?,
                    completed_at.to_string(),
                    command_invocation_id.to_string(),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ToolMutationCommandJournalStateConflict);
            }
            transaction.commit()?;
            Ok(ToolMutationCommandJournalRecordV1 {
                state,
                outcome: Some(outcome),
                updated_at: completed_at,
                ..current
            })
        })
    }

    pub fn tool_mutation_command_v1(
        &self,
        command_invocation_id: CommandInvocationId,
    ) -> Result<Option<ToolMutationCommandJournalRecordV1>, PersistenceError> {
        self.with_reader(move |connection| {
            load_tool_mutation_command(connection, command_invocation_id)
        })
    }
}

fn load_tool_mutation_command(
    connection: &rusqlite::Connection,
    command_invocation_id: CommandInvocationId,
) -> Result<Option<ToolMutationCommandJournalRecordV1>, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT method_name, request_json, request_hash_json,
                    authenticated_identity_hash_json, seed_json, seed_hash_json,
                    state, outcome_json, outcome_hash_json, created_at, updated_at,
                    original_correlation_id
             FROM tool_mutation_command_journals WHERE command_invocation_id=?1",
            [command_invocation_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            },
        )
        .optional()?;
    let Some((
        method_name,
        request_json,
        request_hash_json,
        identity_hash_json,
        seed_json,
        seed_hash_json,
        state,
        outcome_json,
        outcome_hash_json,
        created_at,
        updated_at,
        original_correlation_id,
    )) = row
    else {
        return Ok(None);
    };
    let request = parse_json_strict(request_json.as_bytes())
        .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?;
    let request_hash = serde_json::from_str::<CanonicalSha256HashV1>(&request_hash_json)?;
    if canonical_sha256(&request)
        .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?
        != request_hash
    {
        return Err(PersistenceError::ToolMutationCommandJournalIntegrity);
    }
    let authenticated_identity_hash = serde_json::from_str::<ContentHash>(&identity_hash_json)?;
    let seed_value = parse_json_strict(seed_json.as_bytes())
        .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?;
    let seed = serde_json::from_value::<ToolMutationCommandSeedV1>(seed_value.clone())?;
    let persisted_correlation_id = original_correlation_id
        .ok_or(PersistenceError::ToolMutationCommandJournalIntegrity)
        .and_then(|value| {
            CorrelationId::from_str(&value)
                .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)
        })?;
    if persisted_correlation_id != seed.original_correlation_id {
        return Err(PersistenceError::ToolMutationCommandJournalIntegrity);
    }
    let seed_hash = serde_json::from_str::<CanonicalSha256HashV1>(&seed_hash_json)?;
    if canonical_sha256(&seed_value)
        .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?
        != seed_hash
    {
        return Err(PersistenceError::ToolMutationCommandJournalIntegrity);
    }
    let state = parse_journal_state(&state)?;
    let outcome = match (outcome_json, outcome_hash_json) {
        (None, None) if state == ToolMutationCommandJournalStateV1::Prepared => None,
        (Some(json), Some(hash_json)) if state != ToolMutationCommandJournalStateV1::Prepared => {
            let value = parse_json_strict(json.as_bytes())
                .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?;
            let hash = serde_json::from_str::<CanonicalSha256HashV1>(&hash_json)?;
            if canonical_sha256(&value)
                .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?
                != hash
            {
                return Err(PersistenceError::ToolMutationCommandJournalIntegrity);
            }
            let outcome = serde_json::from_value::<ToolMutationCommandOutcomeV1>(value)?;
            if (state == ToolMutationCommandJournalStateV1::Succeeded
                && !matches!(outcome, ToolMutationCommandOutcomeV1::Succeeded { .. }))
                || (state == ToolMutationCommandJournalStateV1::Failed
                    && !matches!(outcome, ToolMutationCommandOutcomeV1::Failed { .. }))
            {
                return Err(PersistenceError::ToolMutationCommandJournalIntegrity);
            }
            Some(outcome)
        }
        _ => return Err(PersistenceError::ToolMutationCommandJournalIntegrity),
    };
    Ok(Some(ToolMutationCommandJournalRecordV1 {
        command_invocation_id,
        method_name,
        request,
        request_hash,
        authenticated_identity_hash,
        seed,
        state,
        outcome,
        created_at: UtcTimestamp::from_str(&created_at)
            .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?,
        updated_at: UtcTimestamp::from_str(&updated_at)
            .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?,
    }))
}

pub(crate) fn complete_tool_mutation_command_success_in_transaction(
    transaction: &Transaction<'_>,
    expected_method_name: &str,
    correlation_id: CorrelationId,
    result: ToolMutationResultV1,
    completed_at: UtcTimestamp,
) -> Result<bool, PersistenceError> {
    complete_tool_mutation_command_success_for_methods_in_transaction(
        transaction,
        &[expected_method_name],
        correlation_id,
        result,
        completed_at,
    )
}

pub(crate) fn complete_tool_mutation_command_success_for_methods_in_transaction(
    transaction: &Transaction<'_>,
    expected_method_names: &[&str],
    correlation_id: CorrelationId,
    result: ToolMutationResultV1,
    completed_at: UtcTimestamp,
) -> Result<bool, PersistenceError> {
    let command_invocation_id = transaction
        .query_row(
            "SELECT command_invocation_id
             FROM tool_mutation_command_journals
             WHERE original_correlation_id=?1",
            [correlation_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| {
            CommandInvocationId::from_str(&value)
                .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)
        })
        .transpose()?;
    let Some(command_invocation_id) = command_invocation_id else {
        return Ok(false);
    };
    let current = load_tool_mutation_command(transaction, command_invocation_id)?
        .ok_or(PersistenceError::ToolMutationCommandJournalIntegrity)?;
    if !expected_method_names.contains(&current.method_name.as_str())
        || current.seed.original_correlation_id != correlation_id
    {
        return Err(PersistenceError::ToolMutationCommandJournalIntegrity);
    }
    let outcome = ToolMutationCommandOutcomeV1::Succeeded { result };
    if let Some(existing) = current.outcome {
        if existing == outcome && current.state == ToolMutationCommandJournalStateV1::Succeeded {
            return Ok(false);
        }
        return Err(PersistenceError::ToolMutationCommandJournalStateConflict);
    }
    if current.state != ToolMutationCommandJournalStateV1::Prepared {
        return Err(PersistenceError::ToolMutationCommandJournalStateConflict);
    }
    let outcome_json = canonical_json_string(&outcome)?;
    let outcome_hash = canonical_sha256(&outcome)
        .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?;
    let changed = transaction.execute(
        "UPDATE tool_mutation_command_journals
         SET state='succeeded', outcome_json=?1, outcome_hash_json=?2, updated_at=?3
         WHERE command_invocation_id=?4 AND state='prepared'",
        params![
            outcome_json,
            serde_json::to_string(&outcome_hash)?,
            completed_at.to_string(),
            command_invocation_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolMutationCommandJournalStateConflict);
    }
    Ok(true)
}

fn canonical_json_string<T: Serialize>(value: &T) -> Result<String, PersistenceError> {
    let bytes = aura_domain_common::jcs::canonical_json_jcs(value)
        .map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)?;
    String::from_utf8(bytes).map_err(|_| PersistenceError::ToolMutationCommandJournalIntegrity)
}

const fn journal_state_name(state: ToolMutationCommandJournalStateV1) -> &'static str {
    match state {
        ToolMutationCommandJournalStateV1::Prepared => "prepared",
        ToolMutationCommandJournalStateV1::Succeeded => "succeeded",
        ToolMutationCommandJournalStateV1::Failed => "failed",
    }
}

fn parse_journal_state(value: &str) -> Result<ToolMutationCommandJournalStateV1, PersistenceError> {
    match value {
        "prepared" => Ok(ToolMutationCommandJournalStateV1::Prepared),
        "succeeded" => Ok(ToolMutationCommandJournalStateV1::Succeeded),
        "failed" => Ok(ToolMutationCommandJournalStateV1::Failed),
        _ => Err(PersistenceError::ToolMutationCommandJournalIntegrity),
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{ContentHash, CorrelationId, RuntimeInstanceId, ToolId, UtcTimestamp};
    use aura_tool_domain::{TOOL_SET_ENABLED_METHOD_V1, ToolMutationResultV1};
    use serde_json::json;

    use crate::{
        PersistenceOptions, PersistenceStartup, PrepareToolMutationCommandV1,
        PreparedToolMutationCommandV1, ToolMutationCommandOutcomeV1,
    };

    fn persistence() -> (tempfile::TempDir, crate::PersistenceCoordinator) {
        let root = tempfile::tempdir().expect("temp root");
        let PersistenceStartup::Ready(persistence) =
            crate::PersistenceCoordinator::start(PersistenceOptions::new(
                root.path().join("aura.db"),
                root.path().join("artifacts"),
                RuntimeInstanceId::new(),
            ))
            .expect("persistence startup")
        else {
            panic!("test database must start")
        };
        (root, persistence)
    }

    #[test]
    fn same_command_and_request_replays_exact_result_but_hash_change_conflicts() {
        let (_root, persistence) = persistence();
        let command_invocation_id = aura_domain_common::CommandInvocationId::new();
        let correlation_id = CorrelationId::new();
        let prepared_at = UtcTimestamp::now();
        let input = PrepareToolMutationCommandV1 {
            command_invocation_id,
            method_name: TOOL_SET_ENABLED_METHOD_V1.to_owned(),
            request: json!({}),
            authenticated_identity_hash: ContentHash::sha256(b"authenticated"),
            correlation_id,
            prepared_at,
        };
        let first =
            persistence.prepare_tool_mutation_command_v1(input.clone()).expect("prepare command");
        let PreparedToolMutationCommandV1::Prepared(first) = first else {
            panic!("first command must prepare")
        };
        // No retained tool settings command reserves a Draft or Conformance identity.
        assert!(first.seed.reserved_tool_draft_id.is_none());
        assert!(first.seed.reserved_tool_conformance_run_id.is_none());
        let result = ToolMutationResultV1::ToolCommitted {
            tool_id: ToolId::new(),
            entity_revision: aura_domain_common::Revision::INITIAL,
        };
        let completed = persistence
            .complete_tool_mutation_command_v1(
                command_invocation_id,
                first.request_hash.clone(),
                ToolMutationCommandOutcomeV1::Succeeded { result: result.clone() },
                UtcTimestamp::now(),
            )
            .expect("complete command");
        assert_eq!(
            completed.outcome,
            Some(ToolMutationCommandOutcomeV1::Succeeded { result: result.clone() })
        );
        let replay = persistence.prepare_tool_mutation_command_v1(input).expect("replay command");
        assert_eq!(replay.record().outcome, completed.outcome);

        let conflict = persistence.prepare_tool_mutation_command_v1(PrepareToolMutationCommandV1 {
            command_invocation_id,
            method_name: TOOL_SET_ENABLED_METHOD_V1.to_owned(),
            request: json!({ "different": true }),
            authenticated_identity_hash: ContentHash::sha256(b"authenticated"),
            correlation_id: CorrelationId::new(),
            prepared_at: UtcTimestamp::now(),
        });
        assert!(matches!(
            conflict,
            Err(crate::PersistenceError::ToolMutationCommandIdempotencyConflict)
        ));
    }
}
