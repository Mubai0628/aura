//! Share bounded, recorded download receipts with the active task's direct collaborators.
use super::*;
use aura_agent_team::{Member, MemberState};
use aura_domain_common::WorkspacePath;
use aura_persistence_core::{Connection, StoreResult, artifact};
use aura_persistence_v2::retrieval::QueryTerms;
use aura_tool_contract::{
    ModelVisibleToolResultEnvelopeV1, ModelVisibleToolResultOutcomeV1,
    ModelVisibleToolSuccessPayloadV1,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

impl AgentApplication {
    pub(super) async fn team_downloads(
        &self,
        turn: &TurnTask,
        child: Option<&Member>,
        latest_input: Option<&str>,
    ) -> Option<ContextFragment> {
        let input = if let Some(child) = child { &child.details.sheet.goal } else { latest_input? };
        let query = input.chars().take(320).collect::<String>();
        let session_id = turn.session_id;
        let parent = child.map_or(session_id, |member| member.parent_session_id);
        let workspace = turn.workspace_id;
        let receipts = self
            .store
            .read(StoreStep::run(turn.agent_run_id, "agent.context.team_downloads"), move |db| {
                receipts(db, parent, session_id, workspace, &query)
            })
            .await
            .ok()?;
        if receipts.is_empty() {
            return None;
        }
        Some(ContextFragment {
            source_ref: ContextSourceRef::RuntimeState {
                state_kind: "agent.team_download_receipts".into(),
                state_revision: turn.revision,
            },
            source_kind: ModelEgressSourceKind::ToolResult,
            data_class: NetworkDataClass::WorkspaceMetadata,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: json!({"team_download_receipts": receipts}).to_string(),
        })
    }
}

fn receipts(
    db: &Connection,
    parent: SessionId,
    current: SessionId,
    workspace: WorkspaceId,
    input: &str,
) -> StoreResult<Vec<Value>> {
    let Some(active_turn) = session::load(db, parent)?.active_turn else { return Ok(Vec::new()) };
    let task = turns::load(db, active_turn)?;
    if task.workspace_id != workspace {
        return Ok(Vec::new());
    }
    let members = aura_agent_team::store::members_of(db, parent)?;
    let collaborators = members
        .into_iter()
        .filter(|member| member.created_at >= task.created_at)
        .collect::<Vec<_>>();
    if collaborators.is_empty()
        || (current != parent
            && !collaborators.iter().any(|member| {
                member.child_session_id == current && member.state != MemberState::Stopped
            }))
    {
        return Ok(Vec::new());
    }
    let query = QueryTerms::new(input);
    let threshold = task.created_at.to_string();
    let peers = std::iter::once((parent, "main".to_owned()))
        .chain(collaborators.into_iter().map(|member| (member.child_session_id, member.handle)))
        .map(|(owner, name)| (owner.to_string(), (owner != current).then_some(name)))
        .collect::<BTreeMap<_, _>>();
    let mut statement = db.prepare(
        "SELECT c.id,peer.value FROM json_each(?1) peer JOIN tool_batches b ON b.session_id=peer.key
         JOIN tool_calls c ON c.batch_id=b.id
         JOIN sessions s ON s.id=b.session_id
         WHERE s.workspace_id=?3
         AND json_extract(c.payload,'$.result.completed_at') >= ?2
         AND json_extract(c.payload,'$.call.definition_ref.tool_id') IN (
             SELECT d.tool_id FROM tool_definitions d
             WHERE json_extract(d.payload,'$.canonical_name')='http.download'
         )
         ORDER BY julianday(json_extract(c.payload,'$.result.completed_at')) DESC,c.rowid DESC LIMIT 96",
    )?;
    let mut candidates = Vec::new();
    let mut rows =
        statement.query([serde_json::to_string(&peers)?, threshold, workspace.to_string()])?;
    while let Some(row) = rows.next()? {
        let Ok(id) = row.get::<_, String>(0)?.parse::<ToolCallId>() else { continue };
        if let Some(record) = receipt(db, id, row.get(1)?)? {
            candidates.push(record);
        }
    }
    Ok(latest_distinct(candidates, &query))
}

struct DownloadRecord {
    at: UtcTimestamp,
    id: ToolCallId,
    agent: Option<String>,
    path: WorkspacePath,
    evidence: Option<Value>,
}

fn latest_distinct(mut candidates: Vec<DownloadRecord>, query: &QueryTerms) -> Vec<Value> {
    candidates.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
    let mut paths = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    candidates
        .into_iter()
        .filter_map(|row| {
            // Even a login page, or the reader's own new copy, replaces the previous file.
            if !paths.insert(row.path.as_str().to_lowercase()) {
                return None;
            }
            let agent = row.agent?;
            let value = row.evidence?;
            query.passage(row.path.as_str(), value["source"].as_str()?, 0)?;
            hashes
                .insert(value["hash"].to_string())
                .then(|| json!({"agent":agent, "tool_call":row.id, "receipt":value}))
        })
        .take(4)
        .collect()
}

fn receipt(
    db: &Connection,
    id: ToolCallId,
    agent: Option<String>,
) -> StoreResult<Option<DownloadRecord>> {
    let call = tool::execution::call(db, id)?;
    let Some(result) = call.result else { return Ok(None) };
    let Some(artifact_id) = result.model_visible_result_ref else {
        return Ok(None);
    };
    let bytes = artifact::read(db, artifact_id, &id.to_string())?;
    let envelope: ModelVisibleToolResultEnvelopeV1 = serde_json::from_slice(&bytes)?;
    let ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
    } = envelope.outcome
    else {
        return Ok(None);
    };
    if value["published"].as_bool() != Some(true) {
        return Ok(None);
    }
    let Some(path) = value["path"].as_str().and_then(|path| WorkspacePath::parse(path).ok()) else {
        return Ok(None);
    };
    Ok(Some(DownloadRecord {
        at: result.completed_at,
        id,
        agent,
        path,
        evidence: verified_receipt(&value),
    }))
}

pub(super) fn verified_receipt(value: &Value) -> Option<Value> {
    let status = value["status"].as_u64()?;
    if value["published"].as_bool() != Some(true)
        || value["transfer_complete"].as_bool() != Some(true)
        || !(200..300).contains(&status)
        || matches!(status, 206 | 226)
        || value["bytes_written"].as_u64()? == 0
    {
        return None;
    }
    let mime = value["detected_media_type"].as_str()?;
    // An HTML response may be a login page, error or public full text. It cannot prove a file goal.
    if !matches!(
        mime,
        "application/pdf"
            | "application/zip"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    ) {
        return None;
    }
    if let Some(header) = value["content_type"].as_str() {
        let declared = header.split(';').next()?.trim();
        if !declared.eq_ignore_ascii_case(mime)
            && !declared.eq_ignore_ascii_case("application/octet-stream")
            && !(declared.eq_ignore_ascii_case("application/zip")
                && mime
                    == "application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        {
            return None;
        }
    }
    let path = aura_domain_common::WorkspacePath::parse(value["path"].as_str()?).ok()?;
    let hash: ContentHash = serde_json::from_value(value["hash"].clone()).ok()?;
    let source = value["final_url"].as_str()?;
    let authority = source.split_once("://")?.1.split('/').next()?;
    if source.contains(['?', '#']) || authority.contains('@') || source.len() > 512 {
        return None;
    }
    let result = json!({"path":path,"hash":hash,"content_type":mime,"source":source,
        "bytes_written":value["bytes_written"],"status":status,"transfer_complete":true});
    aura_network_control::SecretScanner
        .scan_text("context.download_receipt", &result.to_string())
        .ok()?;
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_receipts_keep_distinct_versions_and_collapse_duplicate_downloads() {
        let first = UtcTimestamp::now();
        let second = UtcTimestamp::from_value(first.value() + std::time::Duration::from_secs(1));
        let row = |at, agent: &str, path: &str, hash: &str| DownloadRecord {
            at,
            id: ToolCallId::new(),
            agent: Some(agent.into()),
            path: WorkspacePath::parse(path).expect("receipt path"),
            evidence: Some(json!({"path":path,"hash":hash,"source":"https://example.test/paper"})),
        };
        let mut overwritten = row(second, "b", "stale.pdf", "");
        overwritten.evidence = None;
        let receipts = latest_distinct(
            vec![
                row(first, "a", "paper.pdf", "v1"),
                row(second, "b", "copy.pdf", "v1"),
                row(second, "b", "updated.pdf", "v2"),
                row(first, "a", "stale.pdf", "v3"),
                overwritten,
            ],
            &QueryTerms::new("paper"),
        );
        assert_eq!(receipts.len(), 2);
        assert!(receipts.iter().all(|r| r["agent"] == "b"));
        assert!(receipts.iter().any(|r| r["receipt"]["hash"] == "v2"));
    }

    #[test]
    fn html_and_credential_bearing_addresses_are_not_shared_as_available_files() {
        let mut result = json!({
            "status":200,"published":true,"path":"refs/paper.pdf","bytes_written":50,"content_type":"application/pdf",
            "hash": ContentHash::sha256(b"recorded file"),"final_url":"https://example.test/paper.pdf"
        });
        assert!(
            verified_receipt(&result).is_none(),
            "an old publication has no complete-transfer proof"
        );
        result["transfer_complete"] = json!(true);
        result["detected_media_type"] = json!("text/html");
        assert!(
            verified_receipt(&result).is_none(),
            "a login page served with a PDF media type cannot satisfy the file goal"
        );
        result["detected_media_type"] = json!("application/pdf");
        assert!(verified_receipt(&result).is_some());
        result["status"] = json!(206);
        assert!(verified_receipt(&result).is_none());
        result["status"] = json!(200);
        result["content_type"] = json!("text/html");
        assert!(verified_receipt(&result).is_none());
        result["content_type"] = json!("application/pdf");
        result["final_url"] = json!("https://example.test/file?auth=secret");
        assert!(verified_receipt(&result).is_none());
    }
}
