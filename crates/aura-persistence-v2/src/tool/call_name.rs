//! Names come from the same frozen request and output that admitted the call.
use aura_model_domain::{NormalizedModelOutput, ProviderRequestEvidence};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_tool_contract::NormalizedToolCallV1;
use rusqlite::Connection;

pub(crate) fn from_frozen_output(
    call: &NormalizedToolCallV1,
    evidence: &ProviderRequestEvidence,
    output: &NormalizedModelOutput,
) -> StoreResult<String> {
    if let Some(reference) = &call.definition_ref {
        return evidence
            .tool_name_bindings
            .iter()
            .find(|binding| binding.tool_definition == *reference)
            .map(|binding| binding.canonical_name.clone())
            .ok_or(Error::Conflict);
    }
    if !evidence.allows_unknown_tool_names() {
        return Err(Error::Conflict);
    }
    let raw = output
        .tool_calls
        .iter()
        .find(|raw| raw.tool_call_id == call.tool_call_id)
        .ok_or(Error::Conflict)?;
    if evidence.tool_name_bindings.iter().any(|binding| binding.provider_wire_name == raw.tool_name)
    {
        return Err(Error::Conflict);
    }
    Ok(raw.tool_name.clone())
}

pub(crate) fn unbound(
    db: &Connection,
    batch: &super::execution::BatchExecution,
    call: &NormalizedToolCallV1,
) -> StoreResult<String> {
    if call.definition_ref.is_some() || !batch.batch.calls.contains(call) {
        return Err(Error::Conflict);
    }
    let step = batch.batch.invocation_identity.model_step_id;
    let snapshot = crate::model::step_snapshot(db, step)?;
    let evidence = crate::model::request_evidence(db, snapshot.id)?.ok_or(Error::Conflict)?;
    let output = crate::agent::history::output(db, step)?;
    let normalized = crate::hooks::admitted_calls(db, &snapshot, &evidence, &output)?;
    if normalized != batch.batch.calls {
        return Err(Error::Conflict);
    }
    from_frozen_output(call, &evidence, &output)
}
