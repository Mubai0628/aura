//! Reuses the Registry search algorithm and builder with exact V2 Store facts.
//! Built-in loading changes later ModelSteps only; it never installs packages.
use super::*;
use aura_domain_common::jcs::canonical_sha256;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, Transaction};
use aura_persistence_v2::tool::{definitions, exposure};
use aura_tool_registry::*;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Mutex};

fn invalid(_: impl std::fmt::Display) -> Error {
    Error::Invalid("Agent registry Tool")
}
pub(super) fn execute(
    tx: &Transaction<'_>,
    call: &CallContext,
    name: &str,
    args: &Value,
    at: UtcTimestamp,
) -> StoreResult<Value> {
    match name {
        "tools.search" => {
            serde_json::to_value(search(tx, call, &serde_json::from_value(args.clone())?, at)?)
                .map_err(Into::into)
        }
        "tools.describe" => {
            let input: AgentToolDescribeInput = serde_json::from_value(args.clone())?;
            let definition_ref = definition_ref(call, input.tool_id)?;
            serde_json::to_value(describe(tx, call, &ToolDescribeRequestV1 { definition_ref })?)
                .map_err(Into::into)
        }
        "tools.list_loaded" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(args.clone())?;
            Ok(serde_json::to_value(ToolListLoadedResultV1 {
                registry_snapshot_ref: call.registry.snapshot_ref.clone(),
                tools: call
                    .registry
                    .catalog_entries
                    .iter()
                    .filter(|e| {
                        matches!(
                            e.load_state,
                            ToolLoadStateV1::Core
                                | ToolLoadStateV1::Pinned
                                | ToolLoadStateV1::Ephemeral
                        )
                    })
                    .map(|e| {
                        Ok(ToolLoadedEntryV1 {
                            definition_ref: ToolDefinitionSnapshotRef {
                                tool_id: e.tool_id,
                                tool_version: e.tool_version.clone(),
                                registry_snapshot_ref: call.registry.snapshot_ref.clone(),
                            },
                            tool_instance_id: e.tool_instance_id,
                            canonical_name: e.canonical_name.clone(),
                            display_name: e.display_name.clone(),
                            model_summary: e.summary.clone(),
                            source: e.source,
                            audience: definitions::get(tx, e.tool_id, &e.tool_version)?
                                .ok_or(Error::Conflict)?
                                .audience,
                            load_state: e.load_state,
                        })
                    })
                    .collect::<StoreResult<Vec<_>>>()?,
            })?)
        }
        "tools.load" | "tools.unload" => {
            let input = if name == "tools.load" {
                serde_json::from_value::<AgentToolLoadInput>(args.clone())?
            } else {
                AgentToolLoadInput {
                    tool_ids: vec![],
                    replace: serde_json::from_value::<AgentToolUnloadInput>(args.clone())?.tool_ids,
                }
            };
            Ok(serde_json::to_value(super::registry_change::change(tx, call, &input, at)?)?)
        }
        _ => Err(Error::Conflict),
    }
}
fn definition_ref(call: &CallContext, id: ToolId) -> StoreResult<ToolDefinitionSnapshotRef> {
    let entry =
        call.registry.catalog_entries.iter().find(|e| e.tool_id == id).ok_or(Error::Invalid(
            "tools.unknown_tool_id: use a tool_id returned by tools.search",
        ))?;
    Ok(ToolDefinitionSnapshotRef {
        tool_id: id,
        tool_version: entry.tool_version.clone(),
        registry_snapshot_ref: call.registry.snapshot_ref.clone(),
    })
}
fn describe(
    tx: &Transaction<'_>,
    call: &CallContext,
    input: &ToolDescribeRequestV1,
) -> StoreResult<ToolDescribeResultV1> {
    if input.definition_ref.registry_snapshot_ref != call.registry.snapshot_ref {
        return Err(Error::Conflict);
    }
    let entry = call
        .registry
        .catalog_entries
        .iter()
        .find(|e| {
            e.tool_id == input.definition_ref.tool_id
                && e.tool_version == input.definition_ref.tool_version
        })
        .ok_or(Error::Conflict)?;
    let definition =
        definitions::get(tx, entry.tool_id, &entry.tool_version)?.ok_or(Error::Conflict)?;
    if definition.definition_fingerprint != entry.definition_fingerprint {
        return Err(Error::Conflict);
    }
    let schema = ToolSchemaSnapshotV1::new(
        definition.tool_id,
        definition.tool_version.clone(),
        definition.canonical_name.clone(),
        definition.model_description.clone(),
        definition.input_schema.clone(),
        definition.strict,
    )
    .map_err(invalid)?;
    let capability_summary = definition
        .capabilities
        .iter()
        .map(|v| -> StoreResult<_> {
            Ok(ToolCapabilitySummaryV1 {
                capability: serde_json::to_value(v)?.as_str().ok_or(Error::Conflict)?.into(),
                summary: format!("{v:?}"),
            })
        })
        .collect::<StoreResult<Vec<_>>>()?;
    Ok(ToolDescribeResultV1 {
        definition_ref: input.definition_ref.clone(),
        canonical_name: definition.canonical_name,
        display_name: definition.display_name,
        model_summary: definition.model_summary,
        model_description: definition.model_description,
        schema,
        result_contract: definition.result_contract,
        source: definition.source_kind,
        audience: definition.audience,
        capability_summary,
        // The release has no Package resource claims. Real Core resources are
        // resolved per invocation, never inferred from a manifest path.
        resource_claim_summary: vec![],
        load_state: entry.load_state,
        currently_loadable: true,
        safe_unavailability_reason: None,
    })
}
struct SearchStore {
    corpus: ToolSearchCorpusV1,
    cursors: Mutex<BTreeMap<String, ToolSearchCursorBindingRecordV1>>,
}
impl ToolSearchCorpusStore for SearchStore {
    fn read_corpus(
        &self,
        reference: &ToolRegistrySnapshotRef,
    ) -> Result<ToolSearchCorpusV1, ToolSearchCorpusReadError> {
        if *reference != self.corpus.snapshot.snapshot_ref {
            return Err(ToolSearchCorpusReadError::NotFound);
        }
        Ok(self.corpus.clone())
    }
}
impl ToolSearchCursorStore for SearchStore {
    fn issue(
        &self,
        binding: ToolSearchCursorBindingRecordV1,
    ) -> Result<ToolSearchCursorV1, ToolSearchCursorStoreError> {
        binding.validate()?;
        let token = ToolSearchCursorV1::new(format!("tc1.{}", CorrelationId::new()))
            .map_err(|_| ToolSearchCursorStoreError::Corrupted)?;
        self.cursors
            .lock()
            .map_err(|_| ToolSearchCursorStoreError::Unavailable)?
            .insert(token.as_str().into(), binding);
        Ok(token)
    }
    fn resolve(
        &self,
        token: &ToolSearchCursorV1,
    ) -> Result<ToolSearchCursorBindingRecordV1, ToolSearchCursorStoreError> {
        self.cursors
            .lock()
            .map_err(|_| ToolSearchCursorStoreError::Unavailable)?
            .get(token.as_str())
            .cloned()
            .ok_or(ToolSearchCursorStoreError::Unavailable)
    }
}
fn search(
    tx: &Transaction<'_>,
    call: &CallContext,
    input: &ToolSearchRequestV1,
    at: UtcTimestamp,
) -> StoreResult<ToolSearchPageV1> {
    let mut cursors = BTreeMap::new();
    if let Some(token) = &input.cursor {
        let record = exposure::cursor(tx, &call.registry.scope, token)?.ok_or(Error::Conflict)?;
        cursors.insert(token.as_str().into(), serde_json::from_value(record)?);
    }
    let documents = call
        .registry
        .catalog_entries
        .iter()
        .map(|e| {
            let d = definitions::get(tx, e.tool_id, &e.tool_version)?.ok_or(Error::Conflict)?;
            if d.definition_fingerprint != e.definition_fingerprint {
                return Err(Error::Conflict);
            }
            Ok(ToolSearchDocumentV1 {
                tool_id: e.tool_id,
                tool_version: e.tool_version.clone(),
                definition_fingerprint: e.definition_fingerprint.clone(),
                model_description: d.model_description,
            })
        })
        .collect::<StoreResult<Vec<_>>>()?;
    let store = SearchStore {
        corpus: ToolSearchCorpusV1::new(call.registry.clone(), documents).map_err(invalid)?,
        cursors: Mutex::new(cursors),
    };
    let visibility = canonical_sha256(&(
        "agent-registry-visibility.v1",
        &call.registry.scope,
        &call.registry.snapshot_ref,
    ))
    .map_err(invalid)?;
    let page = ToolSearchServiceV1::new(&store, &store)
        .search(&call.registry.snapshot_ref, &visibility, input)
        .map_err(invalid)?;
    if let Some(token) = &page.next_cursor {
        let binding = store.resolve(token).map_err(invalid)?;
        exposure::save_cursor(
            tx,
            &call.registry.scope,
            token,
            &serde_json::to_value(binding)?,
            at,
        )?;
    }
    Ok(page)
}
