use super::*;

pub(super) fn load_tool_continuation_results(
    connection: &rusqlite::Connection,
    batch: &ToolBatchRequestV1,
    group: &HistoryAtomicGroupPreimageV1,
) -> Result<Vec<ToolContinuationResultReadV1>, PersistenceError> {
    if batch.calls.len() != group.calls.len() {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let mut rows = Vec::with_capacity(batch.calls.len());
    for (call, group_call) in batch.calls.iter().zip(&group.calls) {
        if call.ordinal != group_call.ordinal || call.tool_call_id != group_call.tool_call_id {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let result_row = connection
            .query_row(
                "SELECT result_json, result_hash_json, finalized_event_id,
                        model_visible_result_artifact_id
                 FROM tool_results
                 WHERE tool_batch_id=?1 AND tool_call_id=?2 AND ordinal=?3",
                params![
                    batch.tool_batch_id.to_string(),
                    call.tool_call_id.to_string(),
                    i64::from(call.ordinal),
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or(PersistenceError::ToolEntityStateConflict)?;
        let result = decode_verified_record::<ToolResultV1>(&result_row.0, &result_row.1)?;
        result.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if result.tool_call_id != call.tool_call_id
            || decode_json::<CanonicalSha256HashV1>(&result_row.1)?
                != group_call.tool_result_fact_hash
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let finalized_event_id = EventId::from_str(&result_row.2)?;
        validate_result_event(connection, batch, finalized_event_id, call.tool_call_id)?;
        let artifact_id = result_row
            .3
            .as_deref()
            .map(ArtifactId::from_str)
            .transpose()?
            .filter(|artifact_id| Some(*artifact_id) == result.model_visible_result_ref)
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        let artifact = connection
            .query_row(
                "SELECT media_type, byte_length FROM artifacts WHERE artifact_id=?1",
                [artifact_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        if artifact.0 != "application/vnd.aura.model-visible-tool-result.v1+json" {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let definition = connection
            .query_row(
                "SELECT record_json, record_hash_json FROM tool_definitions
                 WHERE tool_id=?1 AND tool_version_json=?2",
                params![
                    resolved_tool(call)?.tool_id.to_string(),
                    encode_json(&resolved_tool(call)?.tool_version)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .map(|(record, hash)| decode_verified_record::<ToolDefinitionRecordV1>(&record, &hash))
            .transpose()?
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        if definition.tool_id != resolved_tool(call)?.tool_id
            || definition.tool_version != resolved_tool(call)?.tool_version
        {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        validate_provider_call_binding(connection, batch, call, &definition)?;
        rows.push(ToolContinuationResultReadV1 {
            tool_call_id: call.tool_call_id,
            finalized_event_id,
            model_visible_result_artifact_id: artifact_id,
            model_visible_result_byte_length: u64::try_from(artifact.1)
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
            canonical_name: definition.canonical_name.to_string(),
            is_error: !matches!(result.outcome, ToolResultOutcomeV1::Succeeded),
        });
    }
    Ok(rows)
}

pub(super) fn build_logical_assistant_tool_calls(
    batch: &ToolBatchRequestV1,
    results: &[ToolContinuationResultReadV1],
) -> Result<Vec<LogicalHistoricalToolCall>, PersistenceError> {
    if batch.calls.len() != results.len() {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    batch
        .calls
        .iter()
        .zip(results)
        .map(|(call, result)| {
            let NormalizedToolCallInputStateV1::Ready { arguments, arguments_hash, .. } =
                &call.input
            else {
                return Err(PersistenceError::ToolEntityIntegrity);
            };
            let arguments_bytes =
                canonical_json_jcs(arguments).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            let logical_hash = ContentHash::sha256(&arguments_bytes);
            if logical_hash.hex != arguments_hash.hex() || result.tool_call_id != call.tool_call_id
            {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            Ok(LogicalHistoricalToolCall {
                tool_call_id: call.tool_call_id,
                tool_id: Some(resolved_tool(call)?.tool_id),
                canonical_name: result.canonical_name.clone(),
                arguments_json: String::from_utf8(arguments_bytes)
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                arguments_hash: logical_hash,
            })
        })
        .collect()
}
