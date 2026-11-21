//! Bind mutable evidence in a summary to the version and scope visible to its author.
use super::*;
use crate::agent_application::context::preparation::{prior_materials, recall};
use aura_domain_common::WorkspacePath;
use aura_memory::MemoryScope;
use serde_json::Value;
use store::BackgroundSource;

#[derive(Clone)]
pub(in crate::agent_application) struct Access {
    workspace: WorkspaceId,
    memory: bool,
    global: bool,
    team: Option<ContentHash>,
    private_activity: Vec<BackgroundSource>,
}

impl Access {
    pub(in crate::agent_application) fn for_turn(
        turn: &TurnTask,
        input: &ContextPreparationInput,
        memory: bool,
    ) -> Self {
        let global = recall::latest_input(input).is_some_and(|(event, text)| {
            event == turn.root_user_event_id && recall::explicit_global_memory(text)
        });
        let team = input.section_inputs.iter().flat_map(|section| &section.fragments)
            .find(|f| matches!(&f.source_ref, ContextSourceRef::RuntimeState { state_kind, .. } if state_kind == "agent.team_download_receipts"))
            .map(|f| ContentHash::sha256(f.content.as_bytes()));
        let private_activity = input
            .section_inputs
            .iter()
            .flat_map(|s| &s.fragments)
            .filter_map(scoped_activity)
            .collect();
        Self { workspace: turn.workspace_id, memory, global, team, private_activity }
    }
}

pub(super) fn collect(
    input: &ContextPreparationInput,
    prior: Option<&store::Compaction>,
) -> anyhow::Result<Vec<BackgroundSource>> {
    bind(
        input.section_inputs.iter().flat_map(|s| &s.fragments),
        prior.map_or(&[], |r| r.background_sources.as_slice()),
    )
}

fn bind<'a>(
    fragments: impl IntoIterator<Item = &'a ContextFragment>,
    inherited: &[BackgroundSource],
) -> anyhow::Result<Vec<BackgroundSource>> {
    let mut refs = inherited.to_vec();
    let mut add = |source| {
        if !refs.contains(&source) {
            refs.push(source);
        }
    };
    for f in fragments {
        if let Some(binding) = scoped_activity(f) {
            add(binding);
        }
        let ContextSourceRef::RuntimeState { state_kind, .. } = &f.source_ref else { continue };
        match state_kind.as_str() {
            recall::RELATED_SOURCES => {
                let v: Value = serde_json::from_str(&f.content)?;
                for note in v["related_background"]["memory_notes"].as_array().into_iter().flatten()
                {
                    let scope: MemoryScope = serde_json::from_value(note["scope"].clone())?;
                    add(BackgroundSource::Memory {
                        global: scope == MemoryScope::Global,
                        path: serde_json::from_value(note["path"].clone())?,
                        version: serde_json::from_value(note["version"].clone())?,
                    });
                }
            }
            prior_materials::PRIOR_MATERIALS => {
                let v: Value = serde_json::from_str(&f.content)?;
                for m in v["prior_material_evidence"].as_array().into_iter().flatten() {
                    let (path, hash) = match m["tool"].as_str() {
                        Some("fs.read") => (&m["path"], &m["disk_hash"]),
                        Some("http.download") => (&m["receipt"]["path"], &m["receipt"]["hash"]),
                        _ => continue,
                    };
                    add(BackgroundSource::File {
                        path: serde_json::from_value::<WorkspacePath>(path.clone())?,
                        version: serde_json::from_value(hash.clone())?,
                    });
                }
            }
            "agent.team_download_receipts" => add(BackgroundSource::TeamReceipts {
                hash: ContentHash::sha256(f.content.as_bytes()),
            }),
            _ => {}
        }
    }
    Ok(refs)
}

pub(super) async fn current(
    app: &Arc<AgentApplication>,
    sources: &[BackgroundSource],
    access: &Access,
) -> bool {
    if sources.is_empty() {
        return true;
    }
    let sources = sources.to_owned();
    let access = access.clone();
    let execution = app.execution.clone();
    tokio::task::spawn_blocking(move || {
        let vaults = if access.memory
            && sources.iter().any(|s| matches!(s, BackgroundSource::Memory { .. }))
        {
            recall::open_vaults(&execution, access.workspace, access.global)
        } else {
            Vec::new()
        };
        sources.iter().all(|source| match source {
            BackgroundSource::Memory { global, path, version } => vaults.iter().any(|vault| {
                (vault.scope().scope() == MemoryScope::Global) == *global
                    && vault.read_path(path).is_ok_and(|r| r.hash == *version)
            }),
            BackgroundSource::File { path, version } => {
                execution.recorded_workspace_file_matches(access.workspace, path, version)
            }
            BackgroundSource::PrivateActivity { .. } => access.private_activity.contains(source),
            BackgroundSource::TeamReceipts { hash } => access.team.as_ref() == Some(hash),
        })
    })
    .await
    .unwrap_or(false)
}

pub(in crate::agent_application) fn scoped_activity(
    f: &ContextFragment,
) -> Option<BackgroundSource> {
    use aura_security_contract::NetworkDataClass;
    let class = match &f.source_ref {
        ContextSourceRef::RuntimeState { state_kind, .. }
            if state_kind == "agent.activity_index.personal.v1" =>
        {
            NetworkDataClass::PersonalData
        }
        ContextSourceRef::RuntimeState { state_kind, .. }
            if state_kind == "agent.activity_index.workspace.v1" =>
        {
            NetworkDataClass::WorkspaceMetadata
        }
        _ if f.source_kind == ModelEgressSourceKind::ActivityIndex
            && f.data_class != NetworkDataClass::PublicContent =>
        {
            f.data_class
        }
        _ => return None,
    };
    Some(BackgroundSource::PrivateActivity {
        data_class: class,
        hash: ContentHash::sha256(f.content.as_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compaction_carries_versioned_recall_and_file_references_without_quoting_them() {
        let make = |name: &str, value: Value| ContextFragment {
            source_ref: ContextSourceRef::RuntimeState {
                state_kind: name.into(),
                state_revision: Revision::INITIAL,
            },
            source_kind: ModelEgressSourceKind::AuraCore,
            data_class: aura_security_contract::NetworkDataClass::WorkspaceSource,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: value.to_string(),
        };
        let note = make(
            recall::RELATED_SOURCES,
            serde_json::json!({"related_background":{"memory_notes":[{
                "scope":MemoryScope::Global, "path":"research.md", "version":"rev2", "quote":"private note text"
            }]}}),
        );
        let file = make(
            prior_materials::PRIOR_MATERIALS,
            serde_json::json!({"prior_material_evidence":[{
                "tool":"fs.read","path":"research.md","disk_hash":ContentHash::sha256(b"file"),"excerpt":"private file text"
            }]}),
        );
        let refs = bind([&note, &file], &[]).expect("valid compaction fixture and receipt");
        assert_eq!(refs.len(), 2);
        assert_eq!(
            bind([&file, &note], &refs).expect("valid compaction fixture and receipt"),
            refs
        );
        assert!(
            matches!(&refs[0], BackgroundSource::Memory { global: true,version,.. } if version=="rev2")
        );
        assert!(
            !serde_json::to_string(&refs)
                .expect("valid compaction fixture and receipt")
                .contains("private")
        );
    }
}
