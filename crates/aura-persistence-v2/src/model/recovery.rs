//! Recovery closes persisted transport bookkeeping without claiming the remote
//! Provider stopped processing, fabricating output, or scheduling a retry.
use super::*;
use aura_domain_common::*;
use aura_model_domain::*;

pub(crate) fn interrupt_run(
    tx: &Transaction<'_>,
    run: AgentRunId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let mut query = tx.prepare(
        "SELECT t.id,t.snapshot_id FROM transport_attempts t JOIN model_execution_snapshots s ON s.id=t.snapshot_id
         WHERE EXISTS(SELECT 1 FROM model_steps m WHERE m.snapshot_id=s.id AND m.agent_run_id=?1)
         OR EXISTS(SELECT 1 FROM context_compactions c,json_each(c.payload,'$.attempts') a
                   WHERE json_extract(c.payload,'$.agent_run_id')=?1
                   AND json_extract(a.value,'$.model_execution_snapshot_id')=s.id)
         ORDER BY t.id"
    )?;
    let rows = query
        .query_map([run.to_string()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, snapshot_id) in rows {
        let id = id.parse().map_err(|_| Error::Conflict)?;
        let snapshot_id = snapshot_id.parse().map_err(|_| Error::Conflict)?;
        let old = agent_transport::attempt(tx, id)?;
        if matches!(
            old.state,
            TransportAttemptState::Completed
                | TransportAttemptState::Failed
                | TransportAttemptState::Cancelled
        ) {
            continue;
        }
        let frozen = snapshot(tx, snapshot_id)?.ok_or(Error::Conflict)?;
        if old.request_evidence.operation_context
            != (TransportOperationContext::AgentModelAttempt {
                model_step_id: frozen.model_step_id,
                model_execution_snapshot_id: frozen.id,
                context_snapshot_id: frozen.context_snapshot_id,
                prompt_bundle_snapshot_id: frozen.prompt_bundle_snapshot_id,
            })
            || old.endpoint_id != frozen.endpoint_id
            || old.endpoint_fingerprint != frozen.endpoint_fingerprint
        {
            return Err(Error::Conflict);
        }
        let failure = NormalizedModelFailure {
            category: ModelFailureCategory::StreamInterrupted,
            code: "agent.transport_interrupted".into(),
            message: "Core transport ended without a persisted terminal; remote delivery is unknown and is not retried".into(),
            retry_class: ModelRetryClass::NotRetryable,
            provider_status: None,
            provider_request_id: old.provider_request_id.clone(),
            provider_error_ref: None,
            partial_output_ref: old.raw_stream_artifact_id,
            transport_attempt_id: Some(old.id),
        };
        let failure = PersistedModelFailure {
            id: ModelFailureId::new(),
            fingerprint: FailureFingerprint(ContentHash::blake3(&serde_json::to_vec(&(
                failure.category,
                &failure.code,
                &failure.message,
                failure.provider_status,
            ))?)),
            failure,
            created_at: at,
        };
        let mut next = old.clone();
        next.state = TransportAttemptState::Failed;
        next.finished_at = Some(at.max(old.started_at));
        next.failure_id = Some(failure.id);
        // This is a Recovery authority transition, deliberately separate from
        // the ordinary transport state machine and its active-Step precondition.
        record::changed(tx.execute("UPDATE transport_attempts SET state=?2,revision=revision+1,payload=?3 WHERE id=?1 AND snapshot_id=?4 AND payload=?5",
            params![id.to_string(), record::encode(&next.state)?, record::encode(&next)?, frozen.id.to_string(), record::encode(&old)?])?)?;
        for (kind, payload) in [
            ("model.failure", serde_json::to_value(&failure)?),
            ("model.transport", serde_json::to_value(&next)?),
        ] {
            event::append(
                tx,
                &event::EventDraft {
                    id: EventId::new(),
                    owner: &frozen.model_step_id.to_string(),
                    kind,
                    version: 1,
                    payload: &payload,
                    at,
                },
                "model",
            )?;
        }
    }
    Ok(())
}
