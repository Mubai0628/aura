use super::*;

/// Reconcile a durable response after a crash between transport settlement and summary commit.
pub fn recover_received(
    tx: &Transaction<'_>,
    old: &Compaction,
    at: UtcTimestamp,
) -> StoreResult<bool> {
    if old.state != "running" {
        return Ok(false);
    }
    let Some(attempt) =
        old.attempts.last().filter(|a| a.state == CompactionModelAttemptState::Running)
    else {
        return Ok(false);
    };
    let receipt: Option<(String, String)> = tx
        .query_row(
            "SELECT t.id,o.payload FROM events e
         JOIN transport_attempts t ON t.id=json_extract(e.payload,'$.transport_attempt_id')
         JOIN model_outputs o ON o.snapshot_id=t.snapshot_id
         WHERE e.owner=?1 AND e.kind='context.compaction_model_output' AND e.version=1
           AND t.snapshot_id=?2 AND t.state=?3 AND o.outcome='completed'",
            params![
                attempt.model_step_id.to_string(),
                attempt.model_execution_snapshot_id.to_string(),
                record::encode(&TransportAttemptState::Completed)?
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((transport, raw)) = receipt else { return Ok(false) };
    let output: NormalizedModelOutput = record::decode(&raw)?;
    let Ok(historical_snapshot) = summary_answer(attempt.model_step_id, &output) else {
        return Ok(false);
    };
    let result = ContextReadCompactionResult {
        source_context_snapshot_id: old.id,
        sequence: old.sequence,
        historical_snapshot,
        active_turn_summary: String::new(),
        structured_compaction: String::new(),
        source_span: old.source_span.clone(),
        anchor_manifest: old.anchors.clone(),
    };
    complete(
        tx,
        old.id,
        attempt.attempt_ordinal,
        transport.parse().map_err(|_| Error::Conflict)?,
        result,
        at,
    )?;
    Ok(true)
}
