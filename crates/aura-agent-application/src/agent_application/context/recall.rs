//! Sources relevant to the accepted user input, outside the reusable system prefix.
use super::*;
use aura_memory::{MemoryScope, MemorySearch, MemoryWorkspace, Vault, VaultRoots};
use aura_persistence_v2::{
    agent::rounds::{RecallHistoryHit, recall_user_sources},
    retrieval::{QueryTerms, related_names},
};
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};

const MAX_SOURCES: usize = 3;
const CACHE_SIZE: usize = 16;
pub(in crate::agent_application) const RELATED_SOURCES: &str = "agent.related_sources";

pub(in crate::agent_application) fn latest_input(
    input: &ContextPreparationInput,
) -> Option<(EventId, &str)> {
    // Initial submissions and answers enter the working set separately from history.
    if let Some(current) = input
        .section_inputs
        .iter()
        .find(|section| section.kind == ContextSectionKind::CurrentTurnWorkingSet)
        .and_then(|section| section.fragments.last())
        && let ContextSourceRef::Event { event_id } = current.source_ref
    {
        return Some((event_id, current.content.trim()));
    }
    input.recent_history_items.iter().rev().find_map(|item| match item {
        LogicalConversationItem::User { source_event_id, content, .. } => {
            Some((*source_event_id, content.trim()))
        }
        _ => None,
    })
}

#[derive(Clone, PartialEq, Eq)]
struct Key {
    session: SessionId,
    input: EventId,
    vaults: Vec<(String, u64)>,
}

#[derive(Clone, Default)]
struct Sources {
    historic: Vec<RecallHistoryHit>,
    memory: Vec<Value>,
}

#[derive(Default)]
pub(in crate::agent_application) struct RecallCache(Mutex<VecDeque<(Key, Sources)>>);

impl RecallCache {
    fn get(&self, key: &Key) -> Option<Sources> {
        self.0.lock().ok()?.iter().find(|(stored, _)| stored == key).map(|(_, value)| value.clone())
    }

    fn store(&self, key: Key, value: Sources) {
        if let Ok(mut rows) = self.0.lock() {
            rows.retain(|(other, _)| other.session != key.session);
            if rows.len() >= CACHE_SIZE {
                rows.pop_front();
            }
            rows.push_back((key, value));
        }
    }
}

impl AgentApplication {
    pub(in crate::agent_application::context) async fn refresh_recalled_sources(
        &self,
        turn: &TurnTask,
        input: &mut ContextPreparationInput,
        memory_available: bool,
    ) {
        if input
            .section_inputs
            .iter()
            .flat_map(|s| &s.fragments)
            .any(|source| source.source_kind == ModelEgressSourceKind::PeerTask)
        {
            return;
        }
        let recalled = self.recall_sources(turn, input, memory_available).await;
        let materials = self.prior_materials(turn, input).await;
        if let Some(state) =
            input.section_inputs.iter_mut().find(|s| s.kind == ContextSectionKind::CurrentTaskState)
        {
            state.fragments.retain(|fragment| {
                !matches!(&fragment.source_ref, ContextSourceRef::RuntimeState { state_kind, .. }
                    if state_kind == RELATED_SOURCES || state_kind == super::prior_materials::PRIOR_MATERIALS)
            });
            state.fragments.extend(recalled);
            state.fragments.extend(materials);
        }
    }

    async fn recall_sources(
        &self,
        turn: &TurnTask,
        input: &ContextPreparationInput,
        memory_available: bool,
    ) -> Option<ContextFragment> {
        let (event, text) = latest_input(input)?;
        if QueryTerms::new(text).is_empty() {
            return None;
        }
        let workspace = turn.workspace_id;
        let uses_global = event == turn.root_user_event_id && explicit_global_memory(text);
        let vaults = if memory_available {
            let execution = Arc::clone(&self.execution);
            tokio::task::spawn_blocking(move || open_vaults(&execution, workspace, uses_global))
                .await
                .ok()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let key = Key {
            session: turn.session_id,
            input: event,
            vaults: vaults
                .iter()
                .map(|vault| (vault.identity().to_owned(), vault.source_revision()))
                .collect(),
        };
        let mut sources = if let Some(sources) = self.recall_cache.get(&key) {
            sources
        } else {
            let query = text.chars().take(320).collect::<String>();
            let session = turn.session_id;
            let history_query = query.clone();
            let historic = self
                .store
                .read(StoreStep::run(turn.agent_run_id, "agent.recall.history"), move |db| {
                    recall_user_sources(db, session, &history_query)
                })
                .await
                .ok()
                .unwrap_or_default();
            let seeds = historic.iter().take(2).map(|hit| hit.quote.clone()).collect::<Vec<_>>();
            let memory =
                tokio::task::spawn_blocking(move || search_vaults(&vaults, &query, &seeds))
                    .await
                    .ok()
                    .unwrap_or_default();
            let value = Sources { historic, memory };
            self.recall_cache.store(key, value.clone());
            value
        };
        let visible = input
            .recent_history_items
            .iter()
            .filter_map(|item| match item {
                LogicalConversationItem::User { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .chain(
                input
                    .section_inputs
                    .iter()
                    .filter(|s| s.kind == ContextSectionKind::CurrentTurnWorkingSet)
                    .flat_map(|s| &s.fragments)
                    .map(|source| source.content.as_str()),
            )
            // Root history entries use the stored BLAKE3 revision. Detailed entries and the
            // sent conversation use SHA-256 for the same user text.
            .flat_map(|text| {
                [ContentHash::blake3(text.as_bytes()), ContentHash::sha256(text.as_bytes())]
                    .map(|hash| hash.to_string())
            })
            .collect::<BTreeSet<_>>();
        sources
            .historic
            .retain(|hit| !visible.contains(&hit.version.to_string()) && safe(&hit.quote));
        sources.historic.truncate(2);
        sources.historic.sort_by_key(|hit| hit.at);
        sources.memory.truncate(MAX_SOURCES - sources.historic.len());
        if sources.historic.is_empty() && sources.memory.is_empty() {
            return None;
        }
        Some(fragment(
            turn,
            RELATED_SOURCES,
            NetworkDataClass::WorkspaceSource,
            json!({
                "related_background": {
                    "original_user_messages": sources.historic,
                    "memory_notes": sources.memory
                }
            }),
        ))
    }
}

pub(in crate::agent_application) fn explicit_global_memory(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "使用全局记忆",
        "查一下全局记忆",
        "查全局记忆",
        "请使用全局记忆",
        "请查全局记忆",
        "use global memory",
        "search global memory",
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
}

pub(in crate::agent_application) fn open_vaults(
    execution: &aura_agent_execution::agent_execution::AgentExecution,
    workspace: WorkspaceId,
    global: bool,
) -> Vec<Arc<Vault>> {
    let Ok(resolver) = execution.workspace.resolver(workspace) else { return Vec::new() };
    if resolver.verify_root_identity().is_err() {
        return Vec::new();
    }
    let root = resolver.canonical_root().to_path_buf();
    let memory = execution.journal.memory();
    let project = VaultRoots::project(&root).is_dir();
    let workspace = MemoryWorkspace { workspace_id: workspace, root };
    [(project, MemoryScope::Project), (global, MemoryScope::Global)]
        .into_iter()
        .filter_map(|(enabled, scope)| {
            enabled.then(|| memory.vault(scope, &workspace).ok()).flatten()
        })
        .collect()
}

fn search_vaults(vaults: &[Arc<Vault>], input: &str, history: &[String]) -> Vec<Value> {
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    let mut visits = Vec::new();
    let request = |query: &str| MemorySearch {
        query: query.to_owned(),
        limit: Some(3),
        ..Default::default()
    };
    for vault in vaults {
        if let Ok(found) = vault.search_related(&request(input))
            && !found.incomplete
        {
            visits.extend(found.hits);
        }
    }
    let lexical = QueryTerms::new(input);
    let related = visits
        .iter()
        .take(2)
        .map(|hit| hit.excerpt.as_str())
        .chain(history.iter().map(String::as_str))
        .flat_map(|quote| related_names(quote, &lexical))
        .collect::<BTreeSet<_>>();
    let mut expansion = Vec::new();
    for alias in related.into_iter().take(2) {
        for vault in vaults {
            if let Ok(found) = vault.search(&request(&alias))
                && !found.incomplete
            {
                expansion.extend(found.hits);
            }
        }
    }
    let mut visits = visits.into_iter();
    for hit in visits.next().into_iter().chain(expansion).chain(visits) {
        if safe(&hit.excerpt)
            && safe(&hit.path)
            && safe(&hit.title)
            && seen.insert(hit.hash.clone())
        {
            sources.push(json!({
                "scope":hit.scope, "path":hit.path, "version":hit.hash,
                "title":hit.title, "quote":hit.excerpt
            }));
            if sources.len() == MAX_SOURCES {
                break;
            }
        }
    }
    sources
}

fn safe(text: &str) -> bool {
    aura_network_control::SecretScanner.scan_text("context.recall", text).is_ok()
}

fn fragment(
    turn: &TurnTask,
    name: &str,
    data_class: NetworkDataClass,
    value: Value,
) -> ContextFragment {
    ContextFragment {
        source_ref: ContextSourceRef::RuntimeState {
            state_kind: name.into(),
            state_revision: turn.revision,
        },
        source_kind: ModelEgressSourceKind::WorkspaceFile,
        data_class,
        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
        content: value.to_string(),
    }
}
