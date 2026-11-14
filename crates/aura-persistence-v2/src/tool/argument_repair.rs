//! Calls whose arguments hold an object or an array as a string of JSON, read as the tool's frozen
//! schema takes them ([`aura_tool_contract::repair_string_encoded_arguments`]). Admission, Hooks,
//! history and recovery read a step's calls through [`super::entry::expand`], which runs this,
//! so each sees the same arguments and the same hash. A step an earlier Core admitted keeps such
//! arguments as the model wrote them: [`as_admitted`] compares it that way.
use aura_agent_domain::ModelStepState;
use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{ModelStepId, ToolCallId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_tool_contract::{NormalizedToolCallInputStateV1 as Input, NormalizedToolCallV1};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::collections::BTreeMap;

/// Whether a top-level field holds a string that may be an object or an array of JSON: only then
/// is the step's registry read.
fn may_hold_json(arguments: &Value) -> bool {
    arguments.as_object().is_some_and(|fields| {
        fields.values().any(|value| {
            value.as_str().is_some_and(|text| {
                matches!(text.trim_start().as_bytes().first(), Some(b'{' | b'['))
            })
        })
    })
}

/// Reads such strings as their tool's schema takes them, and returns what each changed call's
/// result tells the model.
pub(crate) fn repair(
    db: &Connection,
    calls: &mut [NormalizedToolCallV1],
) -> StoreResult<BTreeMap<ToolCallId, String>> {
    let mut notes = BTreeMap::new();
    let Some(registry_id) = calls.iter().find_map(|call| {
        let Input::Ready { arguments, .. } = &call.input else { return None };
        let definition = call.definition_ref.as_ref()?;
        may_hold_json(arguments)
            .then_some(definition.registry_snapshot_ref.tool_registry_snapshot_id)
    }) else {
        return Ok(notes);
    };
    let registry = super::registry(db, registry_id)?.ok_or(Error::Conflict)?;
    for call in calls.iter_mut() {
        let Some(definition) = &call.definition_ref else { continue };
        let Input::Ready { purpose, arguments, .. } = &call.input else { continue };
        if definition.registry_snapshot_ref.tool_registry_snapshot_id != registry_id
            || !may_hold_json(arguments)
        {
            continue;
        }
        let Some(schema) = registry.schema_set.tools.iter().find(|tool| {
            tool.tool_id == definition.tool_id && tool.tool_version == definition.tool_version
        }) else {
            continue;
        };
        let Some(repaired) =
            aura_tool_contract::repair_string_encoded_arguments(&schema.input_schema, arguments)
        else {
            continue;
        };
        let Ok(arguments_hash) = canonical_sha256(&repaired.arguments) else { continue };
        notes.insert(call.tool_call_id, repaired.note());
        call.input = Input::Ready {
            purpose: purpose.clone(),
            arguments: repaired.arguments,
            arguments_hash,
        };
    }
    Ok(notes)
}

/// Which of a step's calls stand as admitted: `read` with its `notes`, unless the step's stored
/// batch holds `written`, the arguments as the model wrote them, as an earlier Core admitted it.
/// Such a step's results said nothing of reading them, so it has no notes.
pub(crate) fn as_admitted(
    stored: Option<&[NormalizedToolCallV1]>,
    written: Vec<NormalizedToolCallV1>,
    read: Vec<NormalizedToolCallV1>,
    notes: BTreeMap<ToolCallId, String>,
) -> (Vec<NormalizedToolCallV1>, BTreeMap<ToolCallId, String>) {
    match stored {
        Some(stored) if !notes.is_empty() && stored == written.as_slice() => {
            (written, BTreeMap::new())
        }
        _ => (read, notes),
    }
}

/// The calls step `step`'s batch was admitted with, once the batch is stored.
pub(crate) fn stored_calls(
    db: &Connection,
    step: ModelStepId,
) -> StoreResult<Option<Vec<NormalizedToolCallV1>>> {
    let ModelStepState::Completed { tool_batch_id: Some(batch), .. } =
        crate::agent::turns::step(db, step)?.state
    else {
        return Ok(None);
    };
    let stored: Option<String> = db
        .query_row("SELECT payload FROM tool_batches WHERE id=?1", [batch.to_string()], |row| {
            row.get(0)
        })
        .optional()?;
    stored
        .map(|text| {
            record::decode::<super::execution::BatchExecution>(&text)
                .map(|stored| stored.batch.calls)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{Revision, ToolId, ToolRegistrySnapshotId};
    use aura_tool_contract::{ToolDefinitionSnapshotRef, ToolRegistrySnapshotRef, ToolVersion};
    use serde_json::json;

    fn call(id: ToolCallId, arguments: Value) -> NormalizedToolCallV1 {
        NormalizedToolCallV1 {
            tool_call_id: id,
            ordinal: 1,
            definition_ref: Some(ToolDefinitionSnapshotRef {
                tool_id: ToolId::new(),
                tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
                registry_snapshot_ref: ToolRegistrySnapshotRef {
                    tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                    registry_revision: Revision::INITIAL,
                    content_hash: CanonicalHashV1::sha256(b"registry"),
                },
            }),
            input: Input::Ready {
                purpose: "Start".parse().expect("purpose"),
                arguments_hash: canonical_sha256(&arguments).expect("hash"),
                arguments,
            },
            provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"envelope"),
        }
    }

    /// A step stored before arguments were read from strings keeps them as written, without a
    /// note; a step stored after, or not stored yet, is read with its note.
    #[test]
    fn a_step_an_earlier_core_admitted_stands_as_it_was_written() {
        let id = ToolCallId::new();
        let written = vec![call(id, json!({"task": "{\"goal\":\"读\"}"}))];
        let mut read = written.clone();
        read[0].input = call(id, json!({"task": {"goal": "读"}})).input;
        let notes = BTreeMap::from([(id, "Note".to_owned())]);
        let pick = |stored: Option<&[NormalizedToolCallV1]>| {
            as_admitted(stored, written.clone(), read.clone(), notes.clone())
        };
        assert_eq!(pick(Some(&written)), (written.clone(), BTreeMap::new()));
        assert_eq!(pick(Some(&read)), (read.clone(), notes.clone()));
        assert_eq!(pick(None), (read.clone(), notes.clone()));
        // Nothing was read from a string: the calls stand whatever is stored.
        assert_eq!(
            as_admitted(Some(&read), written.clone(), written.clone(), BTreeMap::new()),
            (written.clone(), BTreeMap::new())
        );
    }
}
