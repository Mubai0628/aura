//! A conversation's tokens are what its answers' providers reported, counted on from the last
//! count: an answer is counted once, and another conversation's answers never.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_domain::AgentTaskMode;
use aura_domain_common::{AgentRunId, EventId};
use aura_persistence_v2::{V2Store, agent::usage};

fn record(store: &V2Store, run: AgentRunId, kind: &str, tokens: u64) -> i64 {
    let db = store.database.connection();
    db.execute(
        "INSERT INTO events(event_id, owner, kind, version, payload, created_at) \
         VALUES(?1, ?2, ?3, 1, ?4, '2026-11-07T00:00:00Z')",
        rusqlite::params![
            EventId::new().to_string(),
            run.to_string(),
            kind,
            serde_json::json!({"usage": {"input_tokens": tokens / 2, "total_tokens": tokens}})
                .to_string(),
        ],
    )
    .unwrap();
    db.last_insert_rowid()
}

#[test]
fn a_conversations_answers_are_counted_once_from_where_the_last_count_ended() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let ours = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let theirs = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/other");
    let session = ours.turn.session_id;
    let run = ours.turn.agent_run_id;
    record(&store, run, "agent.assistant_response", 100);
    record(&store, run, "agent.tool_call", 7_000);
    record(&store, theirs.turn.agent_run_id, "agent.assistant_response", 9_000);
    let second = record(&store, run, "agent.assistant_response", 250);
    let db = store.database.connection();
    assert_eq!(usage::tokens_after(db, session, 0).unwrap(), (second, 350));

    let third = record(&store, run, "agent.assistant_response", 40);
    let db = store.database.connection();
    assert_eq!(usage::tokens_after(db, session, second).unwrap(), (third, 40));
    assert_eq!(usage::tokens_after(db, session, third).unwrap(), (third, 0));
}
