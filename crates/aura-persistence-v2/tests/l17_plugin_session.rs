//! What a plugin reads of the conversation it was given and how its messages are marked. A
//! message a plugin sent names the plugin, in the queue's record and in the conversation, and
//! goes with the session. The events since a cursor are the turns starting and ending, read from
//! the conversation's own state records in the journal's order, never another conversation's,
//! and one read looks only so far past its cursor.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::PersistenceCoreError as Error;
use aura_persistence_v2::{
    V2Store,
    agent::{
        attachments, desktop_messages, inbox, input_consumption, lifecycle, plugin_session,
        recovery, sender,
    },
    session_purge,
};

fn panel() -> MessageSender {
    MessageSender { plugin: "usage-panel".into(), from: None }
}

fn ended(events: &PluginSessionEvents) -> Vec<(TurnTaskId, Option<AgentTurnOutcome>)> {
    events
        .events
        .iter()
        .filter_map(|event| match &event.event {
            PluginSessionEventKind::TurnEnded { turn_task_id, outcome, .. } => {
                Some((*turn_task_id, *outcome))
            }
            _ => None,
        })
        .collect()
}

fn started(events: &PluginSessionEvents) -> Vec<TurnTaskId> {
    events
        .events
        .iter()
        .filter_map(|event| match event.event {
            PluginSessionEventKind::TurnStarted { turn_task_id } => Some(turn_task_id),
            _ => None,
        })
        .collect()
}

#[test]
fn a_message_a_plugin_sent_names_the_plugin_in_the_conversation_and_goes_with_the_session() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = f.turn.session_id;
    let from = panel();
    let command = CommandInvocationId::new();
    let send = |store: &mut V2Store, command| {
        store.database.unit_of_work(|tx| {
            inbox::submit_prepared(
                tx,
                command,
                &AgentSubmitRequest::new(session, "本会话用了多少", AgentInputLane::NextTurn),
                &inbox::PreparedMessage { sender: Some(&from), ..Default::default() },
                f.at,
            )
        })
    };
    send(&mut store, command).expect("queued");
    let queued = send(&mut store, command).expect("the same command is the same message");
    assert_eq!(queued.messages.len(), 1);
    let kept: i64 = store
        .database
        .connection()
        .query_row("SELECT count(*) FROM events WHERE kind='agent.message_sender'", [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(kept, 1, "sent again, the sender is kept once");
    let message = queued.messages[0].clone();
    let db = store.database.connection();
    assert_eq!(sender::of_message(db, message.message_id).expect("kept"), Some(panel()));
    assert_eq!(sender::of_message(db, EventId::new()).expect("none"), None);

    // The turn it starts shows it in the conversation as the plugin's.
    store
        .database
        .unit_of_work(|tx| recovery::interrupt(tx, f.turn.agent_run_id, "stopped", true, f.at))
        .expect("stop");
    let paused = inbox::load(store.database.connection(), session).expect("inbox");
    let pending = store
        .database
        .unit_of_work(|tx| {
            inbox::mutate(
                tx,
                CommandInvocationId::new(),
                &AgentInboxMutation {
                    session_id: session,
                    expected_revision: paused.revision,
                    action: AgentInboxAction::Resume,
                },
                f.at,
            )
        })
        .expect("resume");
    let first = &pending.messages[0];
    let db = store.database.connection();
    let user = attachments::freeze(db, first, session, f.at).expect("frozen");
    let prior = aura_persistence_v2::agent::turns::load_run(db, f.turn.agent_run_id)
        .expect("run")
        .prompt_bundle_snapshot_id;
    let mut bundle =
        aura_persistence_v2::agent::model_context::bundle(db, prior, session).expect("bundle");
    bundle.id = PromptBundleSnapshotId::new();
    bundle.turn_task_id = TurnTaskId::new();
    let turn = store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::context::insert_bundle(tx, session, &bundle)?;
            let turn = lifecycle::start(tx, &user, AgentTaskMode::Normal, None, bundle.id)?;
            input_consumption::start_batch(tx, &pending, &turn, f.at)?;
            Ok::<_, Error>(turn)
        })
        .expect("turn");
    let db = store.database.connection();
    let page = desktop_messages::page(db, &turn, None).expect("page");
    let shown = page.users.iter().find(|u| u.event_id == user.event_id).expect("shown");
    assert_eq!(shown.sender, Some(panel()));
    assert!(page.users.iter().all(|u| u.event_id == user.event_id || u.sender.is_none()));

    session_purge::purge_now(&mut store.database, |db| session_purge::plan_session(db, session))
        .expect("purged");
    let db = store.database.connection();
    assert_eq!(sender::of_message(db, message.message_id).expect("read"), None);
}

#[test]
fn the_events_since_a_cursor_are_this_conversations_turns_starting_and_ending() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let other = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/other");
    let session = f.turn.session_id;
    let db = store.database.connection();
    let all = plugin_session::events(db, session, 0, plugin_session::EVENTS_WINDOW).expect("read");
    assert_eq!(started(&all), vec![f.turn.turn_task_id], "only this conversation's turn");
    assert!(ended(&all).is_empty());
    assert_eq!(all.cursor, plugin_session::head(db).expect("head"));
    assert!(!all.more);

    // From the cursor on, nothing until the turn ends; then how it ended.
    let cursor = all.cursor;
    let none =
        plugin_session::events(db, session, cursor, plugin_session::EVENTS_WINDOW).expect("read");
    assert!(none.events.is_empty() && none.cursor == cursor);
    for fixture in [&f, &other] {
        store
            .database
            .unit_of_work(|tx| {
                recovery::interrupt(tx, fixture.turn.agent_run_id, "stopped", true, fixture.at)
            })
            .expect("stop");
    }
    let db = store.database.connection();
    let after =
        plugin_session::events(db, session, cursor, plugin_session::EVENTS_WINDOW).expect("read");
    assert_eq!(ended(&after), vec![(f.turn.turn_task_id, Some(AgentTurnOutcome::Cancelled))]);
    assert!(started(&after).is_empty());
    assert!(after.events.iter().all(|event| event.sequence > cursor));

    // One read looks only so far: the rest comes on the next.
    let near = plugin_session::events(db, session, cursor, 1).expect("read");
    assert_eq!(near.cursor, cursor + 1);
    assert!(near.more);
    let mut seen = near.events.clone();
    let mut at = near.cursor;
    loop {
        let next = plugin_session::events(db, session, at, 1).expect("read");
        seen.extend(next.events.clone());
        at = next.cursor;
        if !next.more {
            break;
        }
    }
    assert_eq!(seen, after.events, "read a little at a time, the same events in order");
    // A cursor past the journal's end reads from the end.
    let past = plugin_session::events(db, session, u64::MAX / 4, 10).expect("read");
    assert!(past.events.is_empty() && past.cursor == plugin_session::head(db).expect("head"));
}

#[test]
fn a_turn_taken_off_for_recovery_ends_with_why() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = f.turn.session_id;
    let cursor = plugin_session::head(store.database.connection()).expect("head");
    let why = "model.stream_lost: 连接断了".repeat(40);
    store
        .database
        .unit_of_work(|tx| recovery::interrupt(tx, f.turn.agent_run_id, &why, false, f.at))
        .expect("interrupted");
    let db = store.database.connection();
    let read =
        plugin_session::events(db, session, cursor, plugin_session::EVENTS_WINDOW).expect("read");
    let shown: Vec<_> = read.events.iter().map(|event| event.event.clone()).collect();
    // As Aura records it, at most as long as a plugin is told.
    let told: String = why.chars().take(plugin_session::REASON_MAX_CHARS).collect();
    assert_eq!(
        shown,
        vec![PluginSessionEventKind::TurnEnded {
            turn_task_id: f.turn.turn_task_id,
            outcome: Some(AgentTurnOutcome::RecoveryRequired),
            final_reply: None,
            reason: Some(told),
        }]
    );
}

#[test]
fn the_newest_turns_to_do_list_and_a_conversation_without_requests_reads_as_unused() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = f.turn.session_id;
    let db = store.database.connection();
    assert_eq!(plugin_session::newest_turn(db, session).expect("read"), Some(f.turn.turn_task_id));
    assert!(plugin_session::todo(db, session).expect("read").is_empty());
    assert_eq!(plugin_session::usage(db, session).expect("read"), PluginSessionUsage::default());
    assert_eq!(plugin_session::context_use(db, session).expect("read"), None);
    let absent = SessionId::new();
    assert_eq!(plugin_session::newest_turn(db, absent).expect("read"), None);
    assert!(plugin_session::todo(db, absent).expect("read").is_empty());
}
