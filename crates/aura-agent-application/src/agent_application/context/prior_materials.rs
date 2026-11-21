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
        if !input.section_inputs.iter().any(|section| {
            section.kind == ContextSectionKind::HistoricalSnapshotSummary
                && !section.fragments.is_empty()
        }) {
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
                sources(db, session, workspace, &query)
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
                .filter(|material| {
                    material.local.as_ref().is_none_or(|(path, hash)| {
                        execution.recorded_workspace_file_matches(workspace, path, hash)
                    })
                })
                .map(|material| material.evidence)
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

fn sources(
    db: &Connection,
    session: SessionId,
    workspace: WorkspaceId,
    query: &QueryTerms,
) -> StoreResult<Vec<Material>> {
    let mut statement = db.prepare(
        "WITH recent AS (
             SELECT payload FROM context_compactions WHERE session_id=?1 AND state='completed'
             ORDER BY rowid DESC LIMIT 8
         ), covered AS (
             SELECT DISTINCT x.value AS id FROM recent r,json_each(r.payload,'$.excluded_event_ids') x
         ), supported AS (
             SELECT DISTINCT tool_id,json_extract(payload,'$.canonical_name') AS name
             FROM tool_definitions WHERE name IN ('fs.read','web.fetch','http.download')
         )
         SELECT c.id,d.name,json_extract(e.payload,'$.model_visible_result_ref'),e.created_at
         FROM covered JOIN events e ON e.event_id=covered.id
         JOIN tool_calls c ON c.id=json_extract(e.payload,'$.tool_call_id')
         JOIN tool_batches b ON b.id=c.batch_id AND b.id=e.owner
         JOIN sessions s ON s.id=b.session_id
         JOIN supported d ON d.tool_id=json_extract(c.payload,'$.call.definition_ref.tool_id')
         WHERE e.kind='tool.call_completed' AND b.session_id=?1 AND s.workspace_id=?2
         ORDER BY e.sequence DESC LIMIT 24",
    )?;
    let mut rows = statement.query([session.to_string(), workspace.to_string()])?;
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
            let excerpt =
                if text.chars().count() <= EXCERPT_CHARACTERS { text } else { &hit.excerpt };
            (
                path.as_str().to_lowercase(),
                hit.score,
                Some((path.clone(), disk_hash.clone())),
                json!({
                    "path": path, "disk_hash": disk_hash, "disk_version_matches": true, "range": value["returned_range"],
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
                json!({"receipt": receipt, "disk_version_matches": true}),
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
