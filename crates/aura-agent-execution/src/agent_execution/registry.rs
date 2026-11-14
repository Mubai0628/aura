//! Reuses the Registry search algorithm and builder with exact V2 Store facts.
//! Built-in loading changes later ModelSteps only; it never installs packages.
use super::*;
use aura_domain_common::jcs::canonical_sha256;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, Transaction};
use aura_persistence_v2::tool::{definitions, exposure};
use aura_tool_registry::*;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Mutex};

/// A cursor this conversation's searches did not give, or one whose pages have run out.
const CURSOR_UNKNOWN: &str = "tools.search.cursor: it is not a cursor a search gave in this \
                              conversation, or its pages have run out; search again without a \
                              cursor";

/// A failure inside Aura's own tool list, which no correction of the arguments can fix.
fn internal(_: impl std::fmt::Display) -> Error {
    Error::Invalid("tools.internal")
}

/// Why a search could not be answered, for the model to correct or wait out.
fn search_refusal(error: ToolSearchError) -> Error {
    Error::Invalid(match error {
        ToolSearchError::InvalidRequest => {
            "tools.search.query: give a query of at least one word and a limit of 1 to 50"
        }
        ToolSearchError::CursorMismatch => {
            "tools.search.cursor: it belongs to another search (another query, or another set of \
             tools this conversation can see); repeat that search, or search again without a \
             cursor"
        }
        ToolSearchError::CursorUnavailable => CURSOR_UNKNOWN,
        ToolSearchError::CursorExpired => {
            "tools.search.cursor: the tool list changed since this cursor was given (a tool was \
             loaded or unloaded); search again without a cursor"
        }
        ToolSearchError::SnapshotUnavailable => "tools.search_unavailable",
        ToolSearchError::CorpusCorrupted => "tools.internal",
    })
}

/// What a tool with this capability may do, as tools.describe tells the model.
fn capability_phrase(capability: aura_security_contract::ResolvedCapability) -> &'static str {
    use aura_security_contract::ResolvedCapability as C;
    match capability {
        C::WorkspaceFileRead => "reads files in the workspace",
        C::WorkspaceFileWrite => "changes files in the workspace",
        C::WorkspaceFileCreate => "creates files in the workspace",
        C::WorkspaceFileDelete => "deletes files in the workspace",
        C::WorkspaceFileMove => "moves or renames files in the workspace",
        C::ExternalFileRead => "reads files outside the workspace",
        C::ExternalFileWrite => "changes files outside the workspace",
        C::ExternalFileCreate => "creates files outside the workspace",
        C::ExternalFileDelete => "deletes files outside the workspace",
        C::ProcessExecute => "runs programs and commands",
        C::InteractiveTerminal => "runs an interactive terminal",
        C::BackgroundProcess => "starts programs that keep running in the background",
        C::GitRead => "reads the Git repository",
        C::GitWrite => "changes the Git repository (commits, branches)",
        C::GitRemoteRead => "reads from a Git remote",
        C::GitRemoteWrite => "pushes to a Git remote",
        C::GitDestructive => "rewrites or discards Git history or uncommitted changes",
        C::OrdinaryNetwork => "makes network requests",
        C::PublicNetworkRead => "reads public web pages",
        C::PrivateNetworkAccess => "reaches addresses on the local or private network",
        C::DataEgress => "sends data out of this computer",
        C::DownloadArtifact => "downloads files",
        C::DownloadWindowsExecutable => "downloads Windows programs",
        C::ExternalServiceRead => "reads from an outside service",
        C::ExternalServiceMutation => "changes data in an outside service",
        C::ExternalServiceDestructiveMutation => "deletes or overwrites data in an outside service",
        C::BrowserRead => "reads pages in Aura's browser",
        C::BrowserInteraction => "clicks, types and navigates in Aura's browser",
        C::BrowserSensitiveAction => "takes sensitive actions in Aura's browser",
        C::ChromeRead => "reads a tab the user shared from Chrome",
        C::ChromeInteraction => "clicks, types and navigates in a shared Chrome tab",
        C::ChromeSensitiveAction => "takes sensitive actions in a shared Chrome tab",
        C::ConfigurationRead => "reads configuration settings",
        C::ConfigurationWrite => "changes configuration settings",
        C::RegistryWrite => "changes the Windows registry",
        C::SystemEnvironmentWrite => "changes system environment variables",
        C::SystemFileWrite => "changes system files",
        C::ServiceManagement => "manages Windows services",
        C::ScheduledTaskManagement => "manages scheduled tasks",
        C::DriverManagement => "manages device drivers",
        C::WindowsSoftwareInstall => "installs Windows software",
        C::ElevatedOperation => "runs with administrator rights",
        C::ToolRegistryRead => "reads the list of tools",
        C::ToolEphemeralExposureWrite => "loads or unloads tools for this conversation",
        C::AgentTaskRead => "reads this task's state (to-do, goal, history)",
        C::AgentTaskWrite => "updates this task's state (to-do, goal, completion)",
        C::MemoryRead => "reads saved memory notes",
        C::MemoryWrite => "writes memory notes",
        C::MemoryDelete => "deletes memory notes",
        C::ExternalExtensionCall => "calls a tool of an outside extension, such as an MCP server",
        C::PluginDevelopmentInstall => "adds a workspace folder as a plugin in development",
        C::DesktopRead => "reads a program's windows on the desktop (elements and screenshots)",
        C::DesktopInput => "acts on a program on the desktop (pointer, keyboard, windows)",
    }
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
        _ => Err(Error::Invalid("not a task-state handler")),
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
    .map_err(internal)?;
    let capability_summary = definition
        .capabilities
        .iter()
        .map(|v| -> StoreResult<_> {
            Ok(ToolCapabilitySummaryV1 {
                capability: serde_json::to_value(v)?.as_str().ok_or(Error::Conflict)?.into(),
                summary: capability_phrase(*v).into(),
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
        let record = exposure::cursor(tx, &call.registry.scope, token)?
            .ok_or(Error::Invalid(CURSOR_UNKNOWN))?;
        // A saved binding that no longer decodes is Aura's own record, not the call's arguments.
        cursors.insert(token.as_str().into(), serde_json::from_value(record).map_err(internal)?);
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
        corpus: ToolSearchCorpusV1::new(call.registry.clone(), documents).map_err(internal)?,
        cursors: Mutex::new(cursors),
    };
    let visibility = canonical_sha256(&(
        "agent-registry-visibility.v1",
        &call.registry.scope,
        &call.registry.snapshot_ref,
    ))
    .map_err(internal)?;
    let page = ToolSearchServiceV1::new(&store, &store)
        .search(&call.registry.snapshot_ref, &visibility, input)
        .map_err(search_refusal)?;
    if let Some(token) = &page.next_cursor {
        let binding = store.resolve(token).map_err(internal)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A search the model can correct names the argument and what to pass instead; one it cannot
    /// correct is raised under a key the task-state refusals turn into words.
    #[test]
    fn a_refused_search_names_the_argument_or_its_cause() {
        for error in [
            ToolSearchError::InvalidRequest,
            ToolSearchError::CursorMismatch,
            ToolSearchError::CursorUnavailable,
            ToolSearchError::CursorExpired,
            ToolSearchError::SnapshotUnavailable,
            ToolSearchError::CorpusCorrupted,
        ] {
            let Error::Invalid(text) = search_refusal(error.clone()) else {
                panic!("{error:?} is refused as invalid");
            };
            if matches!(
                error,
                ToolSearchError::SnapshotUnavailable | ToolSearchError::CorpusCorrupted
            ) {
                assert!(!text.contains(' '), "{text} is a refusal key");
            } else {
                assert!(text.starts_with("tools.search."), "{text}");
                assert!(text.contains("search again") || text.contains("give "), "{text}");
            }
        }
        let Error::Invalid(text) = internal("a schema that does not compile") else {
            panic!("an internal failure is refused as invalid");
        };
        assert_eq!(text, "tools.internal");
    }

    /// tools.describe says what a capability lets a tool do in words, not the type's own name.
    #[test]
    fn a_capability_reads_as_words() {
        use aura_security_contract::ResolvedCapability as C;
        for capability in
            [C::WorkspaceFileRead, C::ProcessExecute, C::ExternalExtensionCall, C::DesktopInput]
        {
            let phrase = capability_phrase(capability);
            assert_ne!(phrase, format!("{capability:?}"));
            assert!(phrase.contains(' '), "{phrase}");
        }
        assert_eq!(capability_phrase(C::WorkspaceFileRead), "reads files in the workspace");
    }
}
