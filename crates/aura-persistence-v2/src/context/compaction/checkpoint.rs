use super::*;

/// Walk committed records newest first. A damaged or out-of-scope checkpoint must not hide
/// an earlier usable one. Operational database errors still propagate to the caller.
pub fn usable_checkpoint(
    db: &Connection,
    session: SessionId,
    requesting_run: Option<AgentRunId>,
    visible: impl Fn(&[BackgroundSource]) -> bool,
) -> StoreResult<(Option<Compaction>, bool)> {
    let mut query = db.prepare("SELECT id,payload FROM context_compactions WHERE session_id=?1 AND state='completed' ORDER BY rowid DESC")?;
    let mut rows = query.query([session.to_string()])?;
    let mut skipped = false;
    let is_peer = |run| -> StoreResult<bool> {
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_collaboration_deliveries WHERE target_run_id=?1)",
            [AgentRunId::to_string(&run)],
            |r| r.get(0),
        )?)
    };
    let peer = requesting_run.map(is_peer).transpose()?.unwrap_or(false);
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let raw: String = row.get(1)?;
        if let Ok(record) = serde_json::from_str::<Compaction>(&raw)
            && record.id.to_string() == id
            && record.session_id == session
            && record.state == "completed"
            && visible(&record.background_sources)
            && let (Some(result), Some(artifact_id)) = (&record.result, record.result_artifact_id)
            && !(result.historical_snapshot.trim().is_empty()
                && result.active_turn_summary.trim().is_empty())
        {
            if requesting_run.is_some_and(|run| run != record.agent_run_id)
                && (peer || is_peer(record.agent_run_id)?)
            {
                continue;
            }
            match artifact::read(db, artifact_id, &id) {
                Ok(bytes)
                    if serde_json::from_slice::<ContextReadCompactionResult>(&bytes)
                        .ok()
                        .as_ref()
                        == Some(result)
                        && result.source_span == record.source_span
                        && result.anchor_manifest == record.anchors =>
                {
                    return Ok((Some(record), skipped));
                }
                Ok(_)
                | Err(
                    Error::Conflict
                    | Error::Invalid(_)
                    | Error::Sqlite(rusqlite::Error::QueryReturnedNoRows),
                ) => {}
                Err(error) => return Err(error),
            }
        }
        skipped = true;
    }
    Ok((None, skipped))
}
