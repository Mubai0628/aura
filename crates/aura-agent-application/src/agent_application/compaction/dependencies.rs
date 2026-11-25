//! Bind mutable evidence in a summary to the version and scope visible to its author.
use super::*;
use crate::agent_application::context::preparation::{prior_materials, recall};
use aura_domain_common::WorkspacePath;
use aura_memory::MemoryScope;
use serde_json::Value;
use store::BackgroundSource;

#[derive(Clone)]
pub(in crate::agent_application) struct Access {
    memory: bool,
    global: bool,
    team: Option<ContentHash>,
    private_activity: Vec<BackgroundSource>,
}

impl Access {
    pub(in crate::agent_application) fn for_step<'a>(
        turn: &TurnTask,
        latest_input: Option<(EventId, &str)>,
        memory: bool,
        scoped_fragments: impl IntoIterator<Item = &'a ContextFragment>,
    ) -> Self {
        let global = latest_input.is_some_and(|(event, text)| {
            event == turn.root_user_event_id && recall::explicit_global_memory(text)
        });
        let mut team = None;
        let mut private_activity = Vec::new();
        for fragment in scoped_fragments {
            if matches!(&fragment.source_ref, ContextSourceRef::RuntimeState { state_kind, .. } if state_kind == "agent.team_download_receipts")
            {
                team = Some(ContentHash::sha256(fragment.content.as_bytes()));
            }
            private_activity.extend(scoped_activity(fragment));
        }
        Self { memory, global, team, private_activity }
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
    let mut refs = Vec::new();
    let mut add = |source| {
        // Keep one observation per local source. Earlier revisions remain on their own
        // checkpoints; carrying every historical file digest makes a long session grow forever.
        let same = |r: &BackgroundSource| match (r, &source) {
            (BackgroundSource::File { path: a, .. }, BackgroundSource::File { path: b, .. }) => {
                a == b
            }
            (
                BackgroundSource::Memory { global: g, path: a, .. },
                BackgroundSource::Memory { global: h, path: b, .. },
            ) => g == h && a == b,
            _ => r == &source,
        };
        if let Some(index) = refs.iter().position(same) {
            refs[index] = source;
        } else {
            refs.push(source);
        }
    };
    for source in inherited {
        add(source.clone());
    }
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

pub(super) fn visible(sources: &[BackgroundSource], access: &Access) -> bool {
    sources.iter().all(|source| match source {
        // The historical observation belongs to this session. Current quotes and file use
        // are revalidated independently by recall, prior_materials and the file tools.
        BackgroundSource::File { .. } => true,
        BackgroundSource::Memory { global, .. } => access.memory && (!global || access.global),
        BackgroundSource::PrivateActivity { .. } => access.private_activity.contains(source),
        BackgroundSource::TeamReceipts { hash } => access.team.as_ref() == Some(hash),
    })
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
    fn context_continuity_rechecks_source_scope_independently_of_project_file_revisions() {
        let path = WorkspacePath::parse("events/recent.md").expect("fixture note path");
        let note = |global| BackgroundSource::Memory {
            global,
            path: path.clone(),
            version: "old-file-revision".into(),
        };
        let mut access =
            Access { memory: true, global: false, team: None, private_activity: vec![] };
        assert!(visible(&[note(false)], &access));
        assert!(!visible(&[note(true)], &access));
        access.memory = false;
        assert!(!visible(&[note(false)], &access));
        assert!(!visible(
            &[BackgroundSource::PrivateActivity {
                data_class: aura_security_contract::NetworkDataClass::PersonalData,
                hash: ContentHash::sha256(b"old private scope")
            }],
            &access
        ));
    }
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
