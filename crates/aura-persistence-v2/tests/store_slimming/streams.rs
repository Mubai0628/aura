//! A completed reply's streamed deltas, which an older version kept, go; a stopped reply keeps the
//! deltas it showed. The slimming runs before the launcher copies the store, so it changes no
//! structure: a store 0.1.9 left stays one 0.1.9 opens, and Core adds the indexes after the copy. A
//! store that needs new tables first is slimmed at the start after Core has migrated it.
use std::{
    ops::ControlFlow,
    path::{Path, PathBuf},
};

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_core::{DatabaseConnection, schema};
use aura_persistence_v2::{
    MODULES, V2Store,
    agent::{desktop, history, stream, turns},
    migration_pending, model,
    slimming::{self, Owed, Stage},
};

use super::old_store::{count, reader};
use crate::{release_0_1_9, support};

fn room(_: &Path) -> std::io::Result<u64> {
    Ok(u64::MAX / 4)
}

struct Reply {
    session: SessionId,
    step: ModelStepId,
    text: String,
}

struct Replies {
    _dir: tempfile::TempDir,
    root: PathBuf,
    completed: Reply,
    stopped: Reply,
}

/// An older version's outbox copy of every event not copied yet.
fn outbox(store: &V2Store) {
    store
        .database
        .connection()
        .execute(
            "INSERT INTO projection_outbox(event_id,topic,revision,payload) \
             SELECT event_id,'old',sequence,payload FROM events \
             WHERE event_id NOT IN (SELECT event_id FROM projection_outbox)",
            [],
        )
        .unwrap();
}

/// Streams `count` deltas of the fixture's step, one at a time; returns the text they make.
fn streamed_text(store: &mut V2Store, fixture: &support::Fixture, count: u64) -> String {
    let snapshot =
        model::step_snapshot(store.database.connection(), fixture.step.model_step_id).unwrap();
    let mut attempt =
        aura_contract_schema::canonical_fixture().unwrap().models.transport_attempt.unwrap();
    attempt.id = TransportAttemptId::new();
    store.database.unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt)).unwrap();
    let mut whole = String::new();
    for sequence in 1..=count {
        let piece = format!("piece {sequence}. ");
        let event = ModelStreamEventEnvelope {
            model_step_id: fixture.step.model_step_id,
            transport_attempt_id: attempt.id,
            sequence,
            received_at: fixture.at,
            event: ModelStreamEvent::AssistantTextDelta {
                block_id: ModelContentBlockId::new(),
                text: piece.clone(),
            },
        };
        store.database.unit_of_work(|tx| stream::append(tx, &event)).unwrap();
        whole.push_str(&piece);
    }
    whole
}

/// A store an older version left: a completed reply and a stopped one, each with the deltas it
/// streamed, and the outbox copy of every event.
fn replies() -> Replies {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let mut store = V2Store::create(&root).unwrap();
    let completed = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let stopped =
        support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/second");
    // While an outbox copy is left a seal keeps the deltas, as every older version did.
    outbox(&store);
    let answer = streamed_text(&mut store, &completed, 30);
    let mut output = completed.template.clone();
    output.model_step_id = completed.step.model_step_id;
    output.tool_calls.clear();
    output.assistant_blocks = vec![AssistantContentBlock {
        block_id: ModelContentBlockId::new(),
        text: answer.clone(),
        provider_reference: None,
    }];
    store
        .database
        .unit_of_work(|tx| history::record_output(tx, &output, None, completed.at))
        .unwrap();
    let partial = streamed_text(&mut store, &stopped, 20);
    store
        .database
        .unit_of_work(|tx| {
            let previous = turns::step(tx, stopped.step.model_step_id)?;
            let mut next = previous.clone();
            next.state = ModelStepState::Cancelled;
            next.finished_at = Some(stopped.at);
            turns::save_step(tx, &previous, &next)
        })
        .unwrap();
    outbox(&store);
    store.mark_clean_exit().unwrap();
    drop(store);
    Replies {
        _dir: dir,
        root,
        completed: Reply {
            session: completed.turn.session_id,
            step: completed.step.model_step_id,
            text: answer,
        },
        stopped: Reply {
            session: stopped.turn.session_id,
            step: stopped.step.model_step_id,
            text: partial,
        },
    }
}

/// The rows a step's stream left: its delta events of all three kinds and the derived rows.
fn streamed(root: &Path, step: ModelStepId) -> i64 {
    let db = reader(root);
    let owner = step.to_string();
    let events = count(
        &db,
        &format!(
            "SELECT count(*) FROM events WHERE owner='{owner}' AND kind IN \
             ('agent.visible_delta','agent.visible_delta_index','agent.visible_delta_epoch')"
        ),
    );
    ["agent_visible_delta", "agent_visible_index", "agent_visible_coverage"]
        .iter()
        .map(|table| count(&db, &format!("SELECT count(*) FROM {table} WHERE owner='{owner}'")))
        .sum::<i64>()
        + events
}

fn shown(store: &V2Store, reply: &Reply) -> (String, bool) {
    let view = desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            within_turn: None,
            session_id: reply.session,
            before_turn_task_id: None,
            limit: 5,
        },
        RuntimeInstanceId::new(),
    )
    .unwrap();
    let message = &view.turns[0].assistant_messages[0];
    (message.text.clone(), message.partial)
}

fn slim(root: &Path) -> slimming::Report {
    slimming::slim(root, &room, &mut |_| ControlFlow::Continue(())).unwrap()
}

#[test]
fn a_completed_replys_deltas_go_and_a_stopped_replys_stay() {
    let old = replies();
    assert!(streamed(&old.root, old.completed.step) > 0, "an older version kept them");
    let kept = streamed(&old.root, old.stopped.step);
    assert!(kept > 0);

    let report = slim(&old.root);
    assert_eq!(report.streamed_replies, 1, "{report:?}");
    assert_eq!(streamed(&old.root, old.completed.step), 0);
    assert_eq!(streamed(&old.root, old.stopped.step), kept);
    assert_eq!(count(&reader(&old.root), "SELECT count(*) FROM projection_outbox"), 0);

    let store = V2Store::open(&old.root).unwrap();
    assert_eq!(shown(&store, &old.completed), (old.completed.text.clone(), false));
    assert_eq!(shown(&store, &old.stopped), (old.stopped.text.clone(), true));
    assert_eq!(
        count(
            store.database.connection(),
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' \
             AND name='immutable_event_delete'"
        ),
        1,
        "the guard is back"
    );
}

#[test]
fn a_store_0_1_9_left_is_slimmed_as_it_is_and_gains_its_indexes_when_core_opens_it() {
    let old = replies();
    let database = old.root.join("aura-v2.db");
    release_0_1_9::as_0_1_9(&database);

    let report = slim(&old.root);
    assert_eq!(report.streamed_replies, 1, "{report:?}");
    assert!(!report.waits_for_migration);
    // The copy the launcher takes next is a store 0.1.9 opens: its own check still passes.
    schema::open_checked(&database, &release_0_1_9::modules(), schema::OpenCheck::Structure)
        .unwrap();
    assert!(release_0_1_9::indexes(&reader(&old.root)).is_empty());
    assert!(migration_pending(&database).unwrap());

    let store = V2Store::open(&old.root).unwrap();
    assert_eq!(release_0_1_9::indexes(store.database.connection()), release_0_1_9::INDEXES);
    assert_eq!(shown(&store, &old.completed), (old.completed.text.clone(), false));
}

/// A store from before the agent stream's repair tables: migrating it during the slimming would
/// leave the launcher a copy the version it replaces cannot read.
#[test]
fn a_store_that_needs_new_tables_first_is_slimmed_after_core_migrates_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    std::fs::create_dir(&root).unwrap();
    let agent = include_str!("../../src/agent.sql");
    let (head, _) = agent.split_once("\nCREATE TABLE agent_visible_repair").unwrap();
    let older: &'static str = Box::leak(format!("{head}\n").into_boxed_str());
    let modules = MODULES
        .iter()
        .map(|module| match module.owner {
            "agent" => schema::SchemaModule { owner: "agent", sql: older },
            _ => *module,
        })
        .collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(&root.join("aura-v2.db")).unwrap();
    schema::initialize(&mut database, &modules).unwrap();
    database
        .connection()
        .execute_batch(
            "INSERT INTO events(event_id, owner, kind, version, payload, created_at) \
             VALUES('evt_old','owner','note',1,'{}','2026-01-01T00:00:00Z'); \
             INSERT INTO projection_outbox(event_id,topic,revision,payload) \
             VALUES('evt_old','old',1,'{}');",
        )
        .unwrap();
    drop(database);
    std::fs::write(
        root.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .unwrap();
    let path = root.join("aura-v2.db");

    let report = slim(&root);
    assert!(report.waits_for_migration, "{report:?}");
    assert_eq!(
        slimming::owed(&root),
        Some(Owed { schema_version: 1, stage: Stage::Delete, runs: 0 })
    );
    assert!(migration_pending(&path).unwrap(), "the slimming did not migrate the store");
    assert_eq!(count(&reader(&root), "SELECT count(*) FROM projection_outbox"), 1);

    let store = V2Store::open(&root).unwrap();
    store.mark_clean_exit().unwrap();
    drop(store);
    let report = slim(&root);
    assert!(!report.waits_for_migration);
    assert_eq!(report.outbox_rows, 1);
    assert_eq!(slimming::owed(&root), None);
}
