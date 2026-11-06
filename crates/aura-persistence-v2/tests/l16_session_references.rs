//! Another conversation a message refers to with "@". Its recap is its latest summary or its last
//! rounds; the history tools read it by its handle only when a message of this conversation
//! referred to it and it is still in this workspace, and read this conversation without the
//! argument. Deleting it leaves every record of the conversation that referred to it alone.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use std::collections::BTreeSet;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::PersistenceCoreError as Error;
use aura_persistence_v2::{
    V2Store,
    agent::{references, rounds},
    session_purge,
};
use serde_json::json;

fn referring(to: SessionId) -> Vec<ResolvedMessageReference> {
    vec![ResolvedMessageReference::Session {
        conversation: ConversationHandle::of(to),
        title: "Synthetic".into(),
        recap: "T1: Verify the requested work".into(),
    }]
}

/// Every event and operation in the store.
fn records(db: &rusqlite::Connection) -> BTreeSet<String> {
    let mut statement = db
        .prepare("SELECT event_id FROM events UNION ALL SELECT id FROM operations")
        .expect("query");
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).expect("rows");
    rows.map(|row| row.expect("row")).collect()
}

fn refused(result: &Result<serde_json::Value, Error>, code: &str) -> bool {
    matches!(result, Err(Error::Invalid(why)) if why.starts_with(code))
}

#[test]
fn the_history_tools_read_a_conversation_only_a_message_of_this_one_referred_to() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let earlier = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/b");
    let other = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/c");
    let before = records(store.database.connection());
    let here = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let (session, target, elsewhere, stray) =
        (here.turn.session_id, earlier.turn.session_id, other.turn.session_id, SessionId::new());
    store
        .database
        .connection()
        .execute(
            "UPDATE sessions SET workspace_id=(SELECT workspace_id FROM sessions WHERE id=?1) \
             WHERE id=?2",
            [session.to_string(), target.to_string()],
        )
        .expect("the same workspace");

    // Its recap: no summary yet, so its rounds' first messages.
    let lines = rounds::recap_lines(store.database.connection(), target).expect("recap");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("T1 (") && lines[0].ends_with("): Verify the requested work"));

    // A message of this conversation refers to it and to one of another workspace; a queued one
    // that never became part of the conversation refers to a third, which does not count.
    let message = here.turn.root_user_event_id;
    store
        .database
        .unit_of_work(|tx| {
            let mut both = referring(target);
            both.extend(referring(elsewhere));
            references::set(tx, message, session, &both, here.at)?;
            references::set(tx, EventId::new(), session, &referring(stray), here.at)
        })
        .expect("referred");
    let db = store.database.connection();
    let handle = |id: SessionId| ConversationHandle::of(id).to_string();
    assert!(references::refers_to(db, session, &ConversationHandle::of(target)).expect("read"));
    assert!(!references::refers_to(db, session, &ConversationHandle::of(stray)).expect("read"));

    let listed = rounds::dispatch(
        db,
        session,
        "history.list_turns",
        &json!({"conversation": handle(target)}),
    )
    .expect("the referred conversation");
    assert_eq!(listed["turns"][0]["first_message"], "Verify the requested work");
    let own = rounds::dispatch(db, session, "history.list_turns", &json!({})).expect("this one");
    assert_eq!(own["total"], 1);
    let itself = json!({"conversation": handle(session)});
    assert_eq!(rounds::dispatch(db, session, "history.list_turns", &itself).expect("this"), own);
    let unreferred = rounds::dispatch(
        db,
        session,
        "history.search",
        &json!({"query": "x", "conversation": handle(stray)}),
    );
    assert!(refused(&unreferred, "history.conversation_not_referred"), "{unreferred:?}");
    let away = json!({"conversation": handle(elsewhere)});
    let other_workspace = rounds::dispatch(db, session, "history.list_turns", &away);
    assert!(refused(&other_workspace, "history.conversation_unavailable"), "{other_workspace:?}");
    let by_id = rounds::dispatch(
        db,
        session,
        "history.list_turns",
        &json!({"conversation": target.to_string()}),
    );
    assert!(by_id.is_err(), "a conversation is named by its handle, never its id");
    let mine = records(db).difference(&before).cloned().collect::<BTreeSet<_>>();
    assert!(!mine.is_empty());

    // Deleted since, it cannot be read, and nothing this conversation keeps went with it.
    session_purge::purge_now(&mut store.database, |db| session_purge::plan_session(db, target))
        .expect("purged");
    let db = store.database.connection();
    let after = records(db);
    let lost: Vec<_> = mine.iter().filter(|id| !after.contains(*id)).collect();
    assert!(lost.is_empty(), "the referring conversation lost {lost:?}");
    assert_eq!(references::of_message(db, message).expect("kept").len(), 2);
    let gone = rounds::dispatch(
        db,
        session,
        "history.read",
        &json!({"entries": ["T1.1"], "conversation": handle(target)}),
    );
    assert!(refused(&gone, "history.conversation_unavailable"), "{gone:?}");
    let own = rounds::dispatch(db, session, "history.list_turns", &json!({})).expect("this one");
    assert_eq!(own["total"], 1);
}
