use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{ContentHash, SessionId, SessionSequence};
use aura_domain_common::{ToolCallId, ToolWireVersionV1};
use aura_tool_contract::{
    DEFAULT_TOOL_ACTIVITY_PAGE_SIZE_V1, NormalizedToolCallV1, ToolActivityCursorBindingV1,
    ToolActivityCursorV1, ToolRegistrySnapshotV1, ToolResultOutcomeV1, ToolResultV1,
};
use aura_tool_projection::{
    SessionToolActivityProjection, ToolActivityPageV1, ToolCallActivityHistoryViewV1,
    ToolCallActivitySummaryView, ToolResultOutcomeSummaryView,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use thiserror::Error;

use crate::{
    PersistenceCoordinator, PersistenceError,
    tool_entities::{decode_verified_record, encode_record},
};

const TOOL_ACTIVITY_CURSOR_RANDOM_BYTES: usize = 32;
const TOOL_ACTIVITY_CURSOR_PREFIX: &str = "tac1.";
const TOOL_ACTIVITY_ORDERING_ALGORITHM_VERSION_V1: ToolWireVersionV1 = ToolWireVersionV1;

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SessionActivityVisibilityPreimageV1 {
    kind: &'static str,
    policy_version: u32,
    session_id: SessionId,
}

#[derive(Debug, Error)]
pub enum ToolActivityPageReadErrorV1 {
    #[error("the Tool Activity cursor expired or its historical rows are no longer readable")]
    CursorExpired,
    #[error("the Tool Activity cursor is unavailable")]
    CursorUnavailable,
    #[error("the Tool Activity cursor does not match the authenticated Session visibility")]
    CursorMismatch,
    #[error("the Tool Activity cursor failed integrity verification")]
    CursorCorrupted,
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

pub(crate) fn attach_older_activity_cursor(
    transaction: &Transaction<'_>,
    mut projection: SessionToolActivityProjection,
) -> Result<SessionToolActivityProjection, PersistenceError> {
    projection.older_activity_cursor = None;
    let mut displayed_terminal_boundaries = Vec::new();
    for call in projection
        .recent_calls
        .iter()
        .filter(|call| call.status == aura_tool_contract::ToolCallStatusV1::Finalized)
    {
        let sequence = transaction
            .query_row(
                "SELECT event.session_sequence
                 FROM tool_results result
                 JOIN events event ON event.event_id=result.finalized_event_id
                 WHERE result.tool_call_id=?1",
                [call.tool_call_id.to_string()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten();
        let Some(sequence) = sequence else {
            continue;
        };
        let sequence = u64::try_from(sequence)
            .map(SessionSequence::new)
            .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?;
        displayed_terminal_boundaries.push((sequence, call.tool_call_id));
    }
    let Some((last_session_sequence, last_tool_call_id)) =
        displayed_terminal_boundaries.into_iter().min_by(|left, right| left.cmp(right))
    else {
        return Ok(projection);
    };
    let older_exists = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM tool_results result
            JOIN tool_calls call ON call.tool_call_id=result.tool_call_id
            JOIN tool_batches batch ON batch.tool_batch_id=result.tool_batch_id
            JOIN events event ON event.event_id=result.finalized_event_id
            WHERE batch.session_id=?1
              AND (event.session_sequence<?2
                   OR (event.session_sequence=?2 AND call.tool_call_id<?3))
        )",
        params![
            projection.session_id.to_string(),
            to_sql_sequence(last_session_sequence)?,
            last_tool_call_id.to_string(),
        ],
        |row| row.get::<_, bool>(0),
    )?;
    if !older_exists {
        return Ok(projection);
    }
    let cutoff_session_sequence = maximum_session_sequence(transaction, projection.session_id)?
        .ok_or(PersistenceError::ToolActivityCursorIntegrity)?;
    let visibility_context_hash = canonical_sha256(&SessionActivityVisibilityPreimageV1 {
        kind: "aura.p07.session_tool_activity_visibility",
        policy_version: 1,
        session_id: projection.session_id,
    })
    .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?;
    projection.older_activity_cursor = Some(insert_tool_activity_cursor(
        transaction,
        ToolActivityCursorBindingV1 {
            kind: Default::default(),
            cursor_version: ToolWireVersionV1,
            session_id: projection.session_id,
            visibility_context_hash,
            cutoff_session_sequence,
            page_size: DEFAULT_TOOL_ACTIVITY_PAGE_SIZE_V1,
            last_session_sequence,
            last_tool_call_id,
            ordering_algorithm_version: TOOL_ACTIVITY_ORDERING_ALGORITHM_VERSION_V1,
        },
    )?);
    Ok(projection)
}

#[must_use]
pub fn tool_activity_visibility_context_hash_v1(session_id: SessionId) -> CanonicalSha256HashV1 {
    canonical_sha256(&SessionActivityVisibilityPreimageV1 {
        kind: "aura.p07.session_tool_activity_visibility",
        policy_version: 1,
        session_id,
    })
    .expect("fixed Tool Activity visibility preimage is canonical")
}

#[derive(Clone)]
struct TerminalActivityRow {
    sequence: SessionSequence,
    call: NormalizedToolCallV1,
    result: ToolResultV1,
    batch_id: aura_domain_common::ToolBatchId,
    started_at: Option<aura_domain_common::UtcTimestamp>,
    snapshot: ToolRegistrySnapshotV1,
}

impl PersistenceCoordinator {
    /// Issues the opaque cursor stored in a Session activity projection. The
    /// caller supplies the last terminal row already present in that
    /// projection; the current visible Session event cutoff is frozen here.
    pub fn issue_tool_activity_cursor_v1(
        &self,
        session_id: SessionId,
        visibility_context_hash: CanonicalSha256HashV1,
        page_size: u16,
        last_session_sequence: SessionSequence,
        last_tool_call_id: ToolCallId,
    ) -> Result<ToolActivityCursorV1, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let cutoff = maximum_session_sequence(&transaction, session_id)?
                .ok_or(PersistenceError::ToolActivityCursorIntegrity)?;
            if last_session_sequence > cutoff {
                return Err(PersistenceError::ToolActivityCursorIntegrity);
            }
            let cursor = insert_tool_activity_cursor(
                &transaction,
                ToolActivityCursorBindingV1 {
                    kind: Default::default(),
                    cursor_version: ToolWireVersionV1,
                    session_id,
                    visibility_context_hash,
                    cutoff_session_sequence: cutoff,
                    page_size,
                    last_session_sequence,
                    last_tool_call_id,
                    ordering_algorithm_version: TOOL_ACTIVITY_ORDERING_ALGORITHM_VERSION_V1,
                },
            )?;
            transaction.commit()?;
            Ok(cursor)
        })
    }

    pub fn resolve_tool_activity_cursor_v1(
        &self,
        cursor: &ToolActivityCursorV1,
    ) -> Result<Option<ToolActivityCursorBindingV1>, PersistenceError> {
        let token_hash = ContentHash::sha256(cursor.as_str().as_bytes()).hex;
        self.with_reader(move |connection| resolve_tool_activity_cursor(connection, &token_hash))
    }

    /// Reads one stable page. Authentication remains a Core responsibility,
    /// but the authenticated Session and visibility fingerprint are mandatory
    /// inputs and are rechecked for every page.
    pub fn read_tool_activity_page_v1(
        &self,
        cursor: &ToolActivityCursorV1,
        authenticated_session_id: SessionId,
        current_visibility_context_hash: &CanonicalSha256HashV1,
    ) -> Result<ToolActivityPageV1, ToolActivityPageReadErrorV1> {
        let cursor = cursor.clone();
        let visibility = current_visibility_context_hash.clone();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let token_hash = ContentHash::sha256(cursor.as_str().as_bytes()).hex;
            let binding = resolve_tool_activity_cursor(&transaction, &token_hash)?
                .ok_or(PersistenceError::ToolActivityCursorUnavailable)?;
            if binding.session_id != authenticated_session_id
                || binding.visibility_context_hash != visibility
            {
                return Err(PersistenceError::ToolActivityCursorMismatch);
            }
            if !cursor_boundary_exists(&transaction, &binding)? {
                return Err(PersistenceError::ToolActivityCursorExpired);
            }
            let rows = load_terminal_activity_rows(&transaction, &binding)?;
            let requested = usize::from(binding.page_size);
            let has_next = rows.len() > requested;
            let rows = rows.into_iter().take(requested).collect::<Vec<_>>();
            if rows.is_empty() && terminal_boundary_was_removed(&transaction, &binding)? {
                return Err(PersistenceError::ToolActivityCursorExpired);
            }
            let mut items = Vec::with_capacity(rows.len());
            for row in &rows {
                items.push(activity_history_view(&transaction, row)?);
            }
            let next_cursor = if has_next {
                let last = rows.last().ok_or(PersistenceError::ToolActivityCursorIntegrity)?;
                let next = insert_tool_activity_cursor(
                    &transaction,
                    ToolActivityCursorBindingV1 {
                        kind: Default::default(),
                        cursor_version: ToolWireVersionV1,
                        session_id: binding.session_id,
                        visibility_context_hash: binding.visibility_context_hash.clone(),
                        cutoff_session_sequence: binding.cutoff_session_sequence,
                        page_size: binding.page_size,
                        last_session_sequence: last.sequence,
                        last_tool_call_id: last.call.tool_call_id,
                        ordering_algorithm_version: TOOL_ACTIVITY_ORDERING_ALGORITHM_VERSION_V1,
                    },
                )?;
                Some(next)
            } else {
                None
            };
            let page = ToolActivityPageV1 {
                session_id: binding.session_id,
                cutoff_session_sequence: binding.cutoff_session_sequence,
                items,
                next_cursor,
            };
            transaction.commit()?;
            Ok(page)
        })
        .map_err(map_cursor_persistence_error)
    }
}

fn insert_tool_activity_cursor(
    transaction: &Transaction<'_>,
    binding: ToolActivityCursorBindingV1,
) -> Result<ToolActivityCursorV1, PersistenceError> {
    binding.validate().map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?;
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1)",
        [binding.session_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(PersistenceError::ToolActivityCursorIntegrity);
    }
    let mut random = [0_u8; TOOL_ACTIVITY_CURSOR_RANDOM_BYTES];
    getrandom::fill(&mut random).map_err(|source| PersistenceError::Io {
        action: "generate Tool Activity Cursor",
        path: std::path::PathBuf::from("<database>"),
        source: std::io::Error::other(source),
    })?;
    let token = format!("{TOOL_ACTIVITY_CURSOR_PREFIX}{}", lower_hex(&random));
    let cursor = ToolActivityCursorV1::new(token.clone())
        .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?;
    let token_hash = ContentHash::sha256(token.as_bytes()).hex;
    let (binding_json, binding_hash_json) = encode_record(&binding)?;
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO tool_activity_cursor_bindings(
            cursor_token_hash, session_id, binding_json, binding_hash_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            token_hash,
            binding.session_id.to_string(),
            binding_json,
            binding_hash_json,
            aura_domain_common::UtcTimestamp::now().to_string(),
        ],
    )?;
    if inserted != 1 {
        return Err(PersistenceError::ToolActivityCursorIntegrity);
    }
    Ok(cursor)
}

fn resolve_tool_activity_cursor(
    connection: &Connection,
    token_hash: &str,
) -> Result<Option<ToolActivityCursorBindingV1>, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT session_id, binding_json, binding_hash_json
             FROM tool_activity_cursor_bindings WHERE cursor_token_hash=?1",
            [token_hash],
            |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            },
        )
        .optional()?;
    let Some((session_id, binding_json, binding_hash_json)) = row else {
        return Ok(None);
    };
    let binding =
        decode_verified_record::<ToolActivityCursorBindingV1>(&binding_json, &binding_hash_json)
            .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?;
    if binding.session_id.to_string() != session_id
        || binding.ordering_algorithm_version != TOOL_ACTIVITY_ORDERING_ALGORITHM_VERSION_V1
        || binding.validate().is_err()
    {
        return Err(PersistenceError::ToolActivityCursorIntegrity);
    }
    Ok(Some(binding))
}

fn maximum_session_sequence(
    connection: &Connection,
    session_id: SessionId,
) -> Result<Option<SessionSequence>, PersistenceError> {
    let value = connection.query_row(
        "SELECT MAX(session_sequence) FROM events WHERE session_id=?1",
        [session_id.to_string()],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    value
        .map(|value| {
            u64::try_from(value)
                .map(SessionSequence::new)
                .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)
        })
        .transpose()
}

fn load_terminal_activity_rows(
    connection: &Connection,
    binding: &ToolActivityCursorBindingV1,
) -> Result<Vec<TerminalActivityRow>, PersistenceError> {
    let limit = i64::from(binding.page_size) + 1;
    let rows = connection
        .prepare(
            "SELECT event.session_sequence,
                    call.normalized_call_json, call.normalized_call_hash_json,
                    result.result_json, result.result_hash_json,
                    batch.tool_batch_id, call.started_at, snapshot.snapshot_json
             FROM tool_results AS result
             JOIN tool_calls AS call ON call.tool_call_id=result.tool_call_id
             JOIN tool_batches AS batch ON batch.tool_batch_id=result.tool_batch_id
             JOIN events AS event ON event.event_id=result.finalized_event_id
             JOIN tool_registry_snapshots AS snapshot
               ON snapshot.tool_registry_snapshot_id=batch.registry_snapshot_id
             WHERE batch.session_id=?1
               AND event.session_sequence IS NOT NULL
               AND event.session_sequence<=?2
               AND (event.session_sequence<?3
                    OR (event.session_sequence=?3 AND call.tool_call_id<?4))
             ORDER BY event.session_sequence DESC, call.tool_call_id DESC
             LIMIT ?5",
        )?
        .query_map(
            params![
                binding.session_id.to_string(),
                to_sql_sequence(binding.cutoff_session_sequence)?,
                to_sql_sequence(binding.last_session_sequence)?,
                binding.last_tool_call_id.to_string(),
                limit,
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|row| {
            let sequence = u64::try_from(row.0)
                .map(SessionSequence::new)
                .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?;
            let call: NormalizedToolCallV1 = decode_verified_record(&row.1, &row.2)
                .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?;
            let result: ToolResultV1 = decode_verified_record(&row.3, &row.4)
                .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?;
            if call.tool_call_id != result.tool_call_id {
                return Err(PersistenceError::ToolActivityCursorIntegrity);
            }
            let snapshot: ToolRegistrySnapshotV1 =
                aura_domain_common::jcs::parse_json_strict(row.7.as_bytes())
                    .ok()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .ok_or(PersistenceError::ToolActivityCursorIntegrity)?;
            if crate::tool_reference::resolved(&call)?.registry_snapshot_ref
                != snapshot.snapshot_ref
            {
                return Err(PersistenceError::ToolActivityCursorIntegrity);
            }
            Ok(TerminalActivityRow {
                sequence,
                call,
                result,
                batch_id: row.5.parse().map_err(PersistenceError::from)?,
                started_at: row
                    .6
                    .as_deref()
                    .map(str::parse)
                    .transpose()
                    .map_err(|_| PersistenceError::ToolActivityCursorIntegrity)?,
                snapshot,
            })
        })
        .collect()
}

fn activity_history_view(
    connection: &Connection,
    row: &TerminalActivityRow,
) -> Result<ToolCallActivityHistoryViewV1, PersistenceError> {
    let binding = row
        .snapshot
        .effective_bindings
        .iter()
        .find(|binding| {
            row.call.definition_ref.as_ref().is_some_and(|r| {
                binding.tool_id == r.tool_id && binding.tool_version == r.tool_version
            })
        })
        .ok_or(PersistenceError::ToolActivityCursorIntegrity)?;
    let catalog = row
        .snapshot
        .catalog_entries
        .iter()
        .find(|entry| {
            entry.tool_id == binding.tool_id
                && entry.tool_version == binding.tool_version
                && entry.tool_instance_id == binding.tool_instance_id
        })
        .ok_or(PersistenceError::ToolActivityCursorIntegrity)?;
    let artifact_unavailable = if let Some(artifact_id) = row.result.model_visible_result_ref {
        !connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM artifacts WHERE artifact_id=?1)",
            [artifact_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?
    } else {
        false
    };
    Ok(ToolCallActivityHistoryViewV1 {
        session_sequence: row.sequence,
        call: ToolCallActivitySummaryView {
            tool_call_id: row.call.tool_call_id,
            tool_batch_id: row.batch_id,
            ordinal: row.call.ordinal,
            tool_id: Some(crate::tool_reference::resolved(&row.call)?.tool_id),
            tool_version: Some(crate::tool_reference::resolved(&row.call)?.tool_version.clone()),
            display_name: catalog.display_name.clone(),
            status: aura_tool_contract::ToolCallStatusV1::Finalized,
            outcome: Some(outcome_summary(&row.result.outcome)),
            error_class: crate::tool_execution::error_class(&row.result.outcome),
            side_effect_state: Some(row.result.side_effect_state),
            started_at: row.started_at,
            completed_at: Some(row.result.completed_at),
            can_cancel: false,
        },
        definition_ref: crate::tool_reference::resolved(&row.call)?.clone(),
        tool_instance_id: binding.tool_instance_id,
        source: binding.source_kind,
        artifact_unavailable,
    })
}

fn terminal_boundary_was_removed(
    connection: &Connection,
    binding: &ToolActivityCursorBindingV1,
) -> Result<bool, PersistenceError> {
    let cursor_session_exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1)",
        [binding.session_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    if !cursor_session_exists {
        return Ok(true);
    }
    let prior_terminal_exists = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM tool_results result
            JOIN tool_calls call ON call.tool_call_id=result.tool_call_id
            JOIN tool_batches batch ON batch.tool_batch_id=result.tool_batch_id
            JOIN events event ON event.event_id=result.finalized_event_id
            WHERE batch.session_id=?1 AND event.session_sequence<=?2
        )",
        params![binding.session_id.to_string(), to_sql_sequence(binding.cutoff_session_sequence)?,],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(!prior_terminal_exists)
}

fn cursor_boundary_exists(
    connection: &Connection,
    binding: &ToolActivityCursorBindingV1,
) -> Result<bool, PersistenceError> {
    connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM tool_results result
                JOIN tool_calls call ON call.tool_call_id=result.tool_call_id
                JOIN tool_batches batch ON batch.tool_batch_id=result.tool_batch_id
                JOIN events event ON event.event_id=result.finalized_event_id
                WHERE batch.session_id=?1
                  AND call.tool_call_id=?2
                  AND event.session_sequence=?3
                  AND event.session_sequence<=?4
            )",
            params![
                binding.session_id.to_string(),
                binding.last_tool_call_id.to_string(),
                to_sql_sequence(binding.last_session_sequence)?,
                to_sql_sequence(binding.cutoff_session_sequence)?,
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(PersistenceError::from)
}

fn outcome_summary(outcome: &ToolResultOutcomeV1) -> ToolResultOutcomeSummaryView {
    match outcome {
        ToolResultOutcomeV1::Succeeded => ToolResultOutcomeSummaryView::Succeeded,
        ToolResultOutcomeV1::OperationRejected { .. } => {
            ToolResultOutcomeSummaryView::OperationRejected
        }
        ToolResultOutcomeV1::Failed { .. } => ToolResultOutcomeSummaryView::Failed,
        ToolResultOutcomeV1::Cancelled { reason } => {
            ToolResultOutcomeSummaryView::Cancelled { reason: *reason }
        }
        ToolResultOutcomeV1::NotExecuted { reason, .. } => {
            ToolResultOutcomeSummaryView::NotExecuted { reason: *reason }
        }
        ToolResultOutcomeV1::Indeterminate { reason, .. } => {
            ToolResultOutcomeSummaryView::Indeterminate { reason: *reason }
        }
    }
}

fn map_cursor_persistence_error(error: PersistenceError) -> ToolActivityPageReadErrorV1 {
    match error {
        PersistenceError::ToolActivityCursorExpired => ToolActivityPageReadErrorV1::CursorExpired,
        PersistenceError::ToolActivityCursorUnavailable => {
            ToolActivityPageReadErrorV1::CursorUnavailable
        }
        PersistenceError::ToolActivityCursorMismatch => ToolActivityPageReadErrorV1::CursorMismatch,
        PersistenceError::ToolActivityCursorIntegrity | PersistenceError::ToolEntityIntegrity => {
            ToolActivityPageReadErrorV1::CursorCorrupted
        }
        other => ToolActivityPageReadErrorV1::Persistence(other),
    }
}

fn to_sql_sequence(value: SessionSequence) -> Result<i64, PersistenceError> {
    i64::try_from(value.value()).map_err(|_| PersistenceError::ToolActivityCursorIntegrity)
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
