//! A store as an older version left it: an outbox copy of every event, the whole attempt record
//! in each `model.transport` event, a context snapshot with its section texts for every step, and
//! the raw stream of every call.
use std::path::{Path, PathBuf};

use aura_domain_common::*;
use aura_model_domain::{TransportAttemptState, TransportOperationContext};
use aura_persistence_core::{StoreResult, artifact, event, record};
use aura_persistence_v2::{V2Store, workspace};
use rusqlite::{Connection, Transaction, params};

/// Snapshots each conversation's steps left.
pub const SNAPSHOTS: usize = 6;
/// Other events, each with its outbox copy.
const FACTS: usize = 300;
/// A snapshot's conversation, so the snapshots that go are well past what is worth a rebuild.
const CONVERSATION_BYTES: usize = 7 * 1024 * 1024;

pub struct Conversation {
    pub id: SessionId,
    /// Oldest first; the last is the newest.
    pub snapshots: Vec<String>,
    /// The newest snapshot's step and its section texts.
    pub newest_step: String,
    pub kept_sections: Vec<ArtifactId>,
    /// The section texts of the older snapshots.
    pub cleared_sections: Vec<ArtifactId>,
}

pub struct Call {
    pub id: TransportAttemptId,
    pub stream: ArtifactId,
    pub step: String,
    /// The states its `model.transport` events recorded, in order.
    pub states: Vec<TransportAttemptState>,
}

pub struct OldStore {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub conversations: Vec<Conversation>,
    pub completed: Call,
    pub failed: Call,
    pub events: i64,
}

pub fn reader(root: &Path) -> Connection {
    aura_persistence_core::open_reader(&root.join("aura-v2.db")).unwrap()
}

pub fn count(db: &Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn conversation(
    tx: &Transaction<'_>,
    at: UtcTimestamp,
    shared: &[u8],
    index: usize,
) -> StoreResult<Conversation> {
    let workspace_id = WorkspaceId::new();
    let id = SessionId::new();
    workspace::insert(
        tx,
        &aura_workspace_domain::WorkspaceSummary {
            workspace_id,
            display_name: "Old".into(),
            selected_root: format!("C:/old/{index}"),
            canonical_root: format!("C:/old/{index}"),
            root_identity: None,
            state: aura_workspace_domain::WorkspaceState::Offline,
            revision: Revision::INITIAL,
            catalog_freshness: aura_workspace_domain::CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        },
    )?;
    workspace::insert_session(
        tx,
        &workspace::SessionMetadata {
            id,
            workspace_id,
            title: "Old".into(),
            created_at: at,
            updated_at: at,
        },
    )?;
    let mut value = Conversation {
        id,
        snapshots: Vec::new(),
        newest_step: String::new(),
        kept_sections: Vec::new(),
        cleared_sections: Vec::new(),
    };
    for n in 0..SNAPSHOTS {
        let snapshot = ContextProjectionSnapshotId::new().to_string();
        let step = ModelStepId::new().to_string();
        // Every step sends the same instructions and texts of its own.
        let own = index * 100 + n * 2;
        let sections = [shared.to_vec(), unique(own, 64 * 1024), unique(own + 1, 64 * 1024)];
        let mut ids = Vec::new();
        for bytes in sections {
            let section = ArtifactId::new();
            artifact::put(tx, section, &step, "text/plain; charset=utf-8", &bytes, at)?;
            ids.push(section);
        }
        let payload = serde_json::json!({
            "id": snapshot,
            "session_id": id,
            "model_step_id": step,
            "sections": ids.iter().map(|id| serde_json::json!({ "content_artifact_id": id })).collect::<Vec<_>>(),
            "conversation_items": "x".repeat(CONVERSATION_BYTES),
        });
        tx.execute(
            "INSERT INTO context_snapshots VALUES(?1,?2,NULL,?3)",
            params![snapshot, id.to_string(), record::encode(&payload)?],
        )?;
        if n + 1 == SNAPSHOTS {
            value.newest_step = step;
            value.kept_sections = ids;
        } else {
            value.cleared_sections.extend(ids);
        }
        value.snapshots.push(snapshot);
    }
    Ok(value)
}

/// Bytes no other seed gives, so each text is its own object.
fn unique(seed: usize, length: usize) -> Vec<u8> {
    let mut bytes = format!("text {seed};").into_bytes();
    bytes.resize(length, b'.');
    bytes
}

/// One call, its row in its final state, its raw stream, and one `model.transport` event with the
/// whole record for each state it went through.
fn call(
    tx: &Transaction<'_>,
    session: SessionId,
    at: UtcTimestamp,
    states: Vec<TransportAttemptState>,
    seed: usize,
) -> StoreResult<Call> {
    let canonical = aura_contract_schema::canonical_fixture().unwrap();
    let mut attempt = canonical.models.transport_attempt.unwrap();
    let step = ModelStepId::new();
    let snapshot = ModelExecutionSnapshotId::new();
    let stream = ArtifactId::new();
    attempt.id = TransportAttemptId::new();
    attempt.request_evidence.operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: step,
        model_execution_snapshot_id: snapshot,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
    };
    tx.execute(
        "INSERT INTO model_execution_snapshots VALUES(?1,?2,'endpoint','profile','{}')",
        params![snapshot.to_string(), session.to_string()],
    )?;
    artifact::put(tx, stream, &step.to_string(), "text/event-stream", &unique(seed, 100_000), at)?;
    for state in &states {
        attempt.state = *state;
        attempt.raw_stream_artifact_id = state_is_final(*state).then_some(stream);
        event::append(
            tx,
            &event::EventDraft {
                id: EventId::new(),
                owner: &step.to_string(),
                kind: "model.transport",
                version: 1,
                payload: &serde_json::to_value(&attempt)?,
                at,
            },
            "model",
        )?;
    }
    tx.execute(
        "INSERT INTO transport_attempts VALUES(?1,?2,?3,1,?4)",
        params![
            attempt.id.to_string(),
            snapshot.to_string(),
            record::encode(&attempt.state)?,
            record::encode(&attempt)?
        ],
    )?;
    Ok(Call { id: attempt.id, stream, step: step.to_string(), states })
}

fn state_is_final(state: TransportAttemptState) -> bool {
    matches!(state, TransportAttemptState::Completed | TransportAttemptState::Failed)
}

pub fn build() -> OldStore {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let mut store = V2Store::create(&root).unwrap();
    let at: UtcTimestamp = "2026-10-20T00:00:00Z".parse().unwrap();
    let shared = unique(0, 32 * 1024);
    let (conversations, completed, failed) = store
        .database
        .unit_of_work(|tx| -> StoreResult<_> {
            let conversations =
                vec![conversation(tx, at, &shared, 1)?, conversation(tx, at, &shared, 2)?];
            for n in 0..FACTS {
                event::append(
                    tx,
                    &event::EventDraft {
                        id: EventId::new(),
                        owner: "run_old",
                        kind: "agent.fact",
                        version: 1,
                        payload: &serde_json::json!({ "n": n }),
                        at,
                    },
                    "agent",
                )?;
            }
            use TransportAttemptState::*;
            let session = conversations[0].id;
            let completed = call(
                tx,
                session,
                at,
                vec![QueuedForEndpoint, Connecting, Streaming, Completed],
                901,
            )?;
            let failed = call(tx, session, at, vec![QueuedForEndpoint, Connecting, Failed], 902)?;
            // The copy an older version wrote of every event.
            tx.execute(
                "INSERT INTO projection_outbox(event_id,topic,revision,payload) \
                 SELECT event_id,'old',sequence,payload FROM events",
                [],
            )?;
            Ok((conversations, completed, failed))
        })
        .unwrap();
    store.mark_clean_exit().unwrap();
    let events = count(store.database.connection(), "SELECT count(*) FROM events");
    drop(store);
    OldStore { _dir: dir, root, conversations, completed, failed, events }
}
