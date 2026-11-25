//! Recover bounded source evidence whose original tool results a compaction replaced.
use super::*;
use aura_domain_common::WorkspacePath;
use aura_persistence_core::{Connection, StoreResult, artifact};
use aura_persistence_v2::retrieval::QueryTerms;
use aura_tool_contract::{
    ModelVisibleToolResultEnvelopeV1, ModelVisibleToolResultOutcomeV1,
    ModelVisibleToolSuccessPayloadV1,
};
use serde_json::{Value, json};
use std::{cmp::Reverse, collections::BTreeSet};

pub(in crate::agent_application) const PRIOR_MATERIALS: &str = "agent.prior_materials";
const EXCERPT_CHARACTERS: usize = 420;

struct Material {
    key: String,
    score: u32,
    local: Option<(WorkspacePath, ContentHash)>,
    evidence: Value,
}

impl AgentApplication {
    pub(super) async fn prior_materials(
        &self,
        turn: &TurnTask,
        input: &ContextPreparationInput,
    ) -> Option<ContextFragment> {
        let recovering = super::super::recovery::has_gap(input);
        if !recovering
            && !input.section_inputs.iter().any(|section| {
                section.kind == ContextSectionKind::HistoricalSnapshotSummary
                    && !section.fragments.is_empty()
            })
        {
            return None;
        }
        let (_, text) = super::recall::latest_input(input)?;
        if text.starts_with("/compact") {
            return None;
        }
        let session = turn.session_id;
        let workspace = turn.workspace_id;
        let query = QueryTerms::new(&text.chars().take(320).collect::<String>()).with_synonyms();
        if query.is_empty() {
            return None;
        }
        let candidates = self
            .store
            .read(StoreStep::run(turn.agent_run_id, "agent.context.prior_materials"), move |db| {
                sources(db, session, workspace, &query, recovering)
            })
            .await
            .ok()?;
        if candidates.is_empty() {
            return None;
        }
        let execution = self.execution.clone();
        let evidence = tokio::task::spawn_blocking(move || {
            candidates
                .into_iter()
                .take(3)
                .map(|material| {
                    material.checked_evidence(|path, hash| {
                        execution.recorded_workspace_file_matches(workspace, path, hash)
                    })
                })
                .collect::<Vec<_>>()
        })
        .await
        .ok()?;
        if evidence.is_empty() {
            return None;
        }
        Some(ContextFragment {
            source_ref: ContextSourceRef::RuntimeState {
                state_kind: PRIOR_MATERIALS.into(),
                state_revision: turn.revision,
            },
            source_kind: ModelEgressSourceKind::ToolResult,
            data_class: NetworkDataClass::WorkspaceSource,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: json!({"prior_material_evidence": evidence}).to_string(),
        })
    }
}

impl Material {
    fn checked_evidence(
        mut self,
        check: impl FnOnce(&WorkspacePath, &ContentHash) -> bool,
    ) -> Value {
        if let Some((path, hash)) = &self.local {
            if check(path, hash) {
                self.evidence["disk_version_matches"] = true.into();
                self.evidence["current_file_state"] = if self.evidence["excerpt_complete"] == true
                    && self.evidence["range"]["range_kind"] == "full"
                {
                    "complete_file_verified_this_step"
                } else {
                    "verified_this_step"
                }
                .into();
            } else {
                self.evidence["disk_version_matches"] = Value::Null;
                self.evidence["current_file_state"] =
                    json!("unverified; locate or check the current file if needed");
                if let Some(fields) = self.evidence.as_object_mut() {
                    fields.remove("excerpt");
                    fields.remove("excerpt_complete");
                }
            }
        }
        self.evidence
    }
}

fn sources(
    db: &Connection,
    session: SessionId,
    workspace: WorkspaceId,
    query: &QueryTerms,
    recovering: bool,
) -> StoreResult<Vec<Material>> {
    let mut statement = db.prepare(
        "WITH recent AS (
             SELECT c.payload FROM context_compactions c WHERE c.session_id=?1 AND c.state='completed'
             AND NOT EXISTS (SELECT 1 FROM agent_collaboration_deliveries d
                 WHERE d.target_run_id=json_extract(c.payload,'$.agent_run_id'))
             ORDER BY c.rowid DESC LIMIT 8
         ), covered AS (
             SELECT DISTINCT x.value AS id FROM recent r,json_each(CASE WHEN json_valid(r.payload) THEN r.payload END,'$.excluded_event_ids') x
         ), supported AS (
             SELECT DISTINCT tool_id,json_extract(payload,'$.canonical_name') AS name
             FROM tool_definitions WHERE name IN ('fs.read','web.fetch','http.download')
         )
         SELECT c.id,d.name,json_extract(e.payload,'$.model_visible_result_ref'),e.created_at
         FROM events e
         JOIN tool_calls c ON c.id=json_extract(e.payload,'$.tool_call_id')
         JOIN tool_batches b ON b.id=c.batch_id AND b.id=e.owner
         JOIN agent_runs source ON source.id=json_extract(b.payload,'$.batch.invocation_identity.agent_run_id')
         JOIN turn_tasks t ON t.id=source.turn_task_id AND t.session_id=b.session_id
         JOIN sessions s ON s.id=b.session_id
         JOIN supported d ON d.tool_id=json_extract(c.payload,'$.call.definition_ref.tool_id')
         WHERE e.kind='tool.call_completed' AND b.session_id=?1 AND s.workspace_id=?2
         AND NOT EXISTS (SELECT 1 FROM agent_collaboration_deliveries d WHERE d.target_run_id=source.id)
         AND (?3 OR e.event_id IN (SELECT id FROM covered))
         ORDER BY e.sequence DESC LIMIT 24",
    )?;
    let mut rows = statement.query((session.to_string(), workspace.to_string(), recovering))?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let at: String = row.get(3)?;
        let Some(reference) = row.get::<_, Option<String>>(2)?.and_then(|value| value.parse().ok())
        else {
            continue;
        };
        let Ok(bytes) = artifact::read(db, reference, &id) else { continue };
        let Ok(envelope) = serde_json::from_slice::<ModelVisibleToolResultEnvelopeV1>(&bytes)
        else {
            continue;
        };
        let ModelVisibleToolResultOutcomeV1::Succeeded {
            payload: ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
        } = envelope.outcome
        else {
            continue;
        };
        if let Some(material) = project(&name, &value, query, &id, &at)
            && seen.insert(material.key.clone())
        {
            result.push(material);
        }
    }
    result.sort_by_key(|material| Reverse(material.score));
    Ok(result)
}

fn project(name: &str, value: &Value, query: &QueryTerms, id: &str, at: &str) -> Option<Material> {
    let (key, score, local, mut evidence) = match name {
        "fs.read" if value["external"].is_null() && value["source"].as_str() == Some("disk") => {
            let path = WorkspacePath::parse(value["path"].as_str()?).ok()?;
            let disk_hash: ContentHash = serde_json::from_value(value["disk_hash"].clone()).ok()?;
            let text = value["text"].as_str()?;
            let hit = query.passage(path.as_str(), text, EXCERPT_CHARACTERS)?;
            let excerpt = read_excerpt(text, &hit.excerpt);
            (
                path.as_str().to_lowercase(),
                hit.score,
                Some((path.clone(), disk_hash.clone())),
                json!({
                    "path": path, "disk_hash": disk_hash, "range": value["returned_range"],
                    "excerpt": excerpt, "excerpt_complete": excerpt == text,
                }),
            )
        }
        "web.fetch" => {
            let url = public_source(value["source_url"].as_str()?)?;
            let title = value["title"].as_str()?;
            let text = value["excerpt"].as_str()?;
            let hit = query.passage(&format!("{title} {url}"), text, EXCERPT_CHARACTERS)?;
            let excerpt =
                if text.chars().count() <= EXCERPT_CHARACTERS { text } else { &hit.excerpt };
            (
                url.into(),
                hit.score,
                None,
                json!({
                    "source_url": url, "title": title.chars().take(160).collect::<String>(),
                    "excerpt": excerpt, "document_artifact_id": value["document_artifact_id"],
                }),
            )
        }
        "http.download" => {
            let receipt = super::team_downloads::verified_receipt(value)?;
            let path = WorkspacePath::parse(receipt["path"].as_str()?).ok()?;
            let hash: ContentHash = serde_json::from_value(receipt["hash"].clone()).ok()?;
            let hit = query.passage(path.as_str(), receipt["source"].as_str()?, 0)?;
            (
                path.as_str().to_lowercase(),
                hit.score,
                Some((path, hash)),
                json!({"receipt": receipt}),
            )
        }
        _ => return None,
    };
    evidence["tool"] = name.into();
    evidence["tool_call"] = id.into();
    evidence["observed_at"] = at.into();
    aura_network_control::SecretScanner
        .scan_text("context.prior_materials", &evidence.to_string())
        .ok()?;
    Some(Material { key, score, local, evidence })
}

fn read_excerpt(text: &str, matched: &str) -> String {
    if text.chars().nth(EXCERPT_CHARACTERS).is_none() {
        return text.to_owned();
    }
    // A matching passage can miss a heading or identifier near the start of a long read.
    let start: String = text.trim_start().chars().take(EXCERPT_CHARACTERS / 3).collect();
    let start = start.trim_end();
    let matched = matched.trim();
    if matched.contains(start) {
        return matched.to_owned();
    }
    if start.contains(matched) {
        return start.to_owned();
    }
    let separator = "\n…\n";
    let remaining = EXCERPT_CHARACTERS - start.chars().count() - separator.chars().count();
    let matched: String = matched.chars().take(remaining).collect();
    format!("{start}{separator}{matched}")
}

fn public_source(url: &str) -> Option<&str> {
    let authority =
        url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?.split('/').next()?;
    (!authority.is_empty()
        && !authority.contains('@')
        && !url.contains(['?', '#'])
        && url.len() <= 512)
        .then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_tail_preserves_the_start_of_the_previously_read_range() {
        let query = QueryTerms::new("Synthetic exercise catalog-a.txt");
        let read = json!({
            "path": "catalog-a.txt",
            "text": format!("Reference code: RECEIPT-43.\n{}\nEnd of synthetic file.", " ".repeat(80_000)),
            "source": "disk", "external": null, "disk_hash": ContentHash::blake3(b"source"),
            "returned_range": {"range_kind": "full"},
        });
        let material = project("fs.read", &read, &query, "call-1", "date").expect("relevant read");
        let checked = material.checked_evidence(|_, _| true);
        let excerpt = checked["excerpt"].as_str().expect("checked sample");
        assert!(excerpt.contains("Reference code: RECEIPT-43."));
        assert!(excerpt.contains("End of synthetic file."));
        assert!(excerpt.chars().count() <= EXCERPT_CHARACTERS);
        assert_eq!(checked["range"]["range_kind"], "full");
        assert_eq!(checked["disk_version_matches"], true);
        assert_eq!(checked["excerpt_complete"], false);
    }

    #[test]
    fn projects_only_relevant_workspace_reads_and_safe_prior_web_sources() {
        let query = QueryTerms::new("GP-742 资料登记");
        let mut read = json!({
            "path": "manual.txt", "text": "GP-742 资料登记，标记 MANUAL-71",
            "source": "disk", "external": null, "disk_hash": ContentHash::blake3(b"source"),
            "returned_range": {"range_kind": "full"},
        });
        let material = project("fs.read", &read, &query, "call-1", "date").expect("relevant proof");
        assert_eq!(material.evidence["excerpt"], read["text"]);
        assert!(material.local.is_some(), "workspace facts await a disk hash check");
        let checked = material.checked_evidence(|_, hash| *hash == ContentHash::blake3(b"source"));
        assert_eq!(checked["current_file_state"], "complete_file_verified_this_step");
        assert_eq!(checked["disk_version_matches"], true);
        read["returned_range"] = json!({"range_kind": "lines", "start": 2, "end": 3});
        let range = project("fs.read", &read, &query, "call-1", "date").expect("relevant range");
        assert_eq!(range.checked_evidence(|_, _| true)["current_file_state"], "verified_this_step");
        let changed = project("fs.read", &read, &query, "call-1", "date").expect("old observation");
        let changed = changed.checked_evidence(|_, _| false);
        assert!(changed.get("excerpt").is_none() && changed["disk_version_matches"].is_null());
        read["source"] = "buffer".into();
        assert!(project("fs.read", &read, &query, "call-2", "date").is_none());

        let mut page = json!({
            "source_url": "https://example.test/catalog", "title": "GP-742 资料登记",
            "excerpt": "Reference B is listed.", "document_artifact_id": "link",
        });
        assert!(project("web.fetch", &page, &query, "call-3", "date").is_some());
        page["source_url"] = "https://example.test/catalog?token=private".into();
        assert!(project("web.fetch", &page, &query, "call-4", "date").is_none());
    }
}
