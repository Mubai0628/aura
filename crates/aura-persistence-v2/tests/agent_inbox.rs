#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    agent::{inbox, input_consumption, session, turns},
};

#[test]
fn batch_consumption_rolls_back_with_turn_and_restarts_without_duplicate_messages() {
    use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
    use aura_persistence_v2::agent::{lifecycle, recovery};
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("v2");
    let mut store = V2Store::create(&path).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    for text in ["first", "/plan remains plain input", "third"] {
        store
            .database
            .unit_of_work(|tx| {
                inbox::submit(
                    tx,
                    CommandInvocationId::new(),
                    &AgentSubmitRequest {
                        session_id: f.turn.session_id,
                        text: text.into(),
                        destination: AgentInputLane::NextTurn,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                    f.at,
                )
            })
            .expect("queue");
    }
    store
        .database
        .unit_of_work(|tx| recovery::interrupt(tx, f.turn.agent_run_id, "stopped", true, f.at))
        .expect("stop");
    let paused = inbox::load(store.database.connection(), f.turn.session_id).expect("inbox");
    assert!(paused.paused);
    let pending = store
        .database
        .unit_of_work(|tx| {
            inbox::mutate(
                tx,
                CommandInvocationId::new(),
                &AgentInboxMutation {
                    session_id: f.turn.session_id,
                    expected_revision: paused.revision,
                    action: AgentInboxAction::Resume,
                },
                f.at,
            )
        })
        .expect("explicit resume");
    let user = pending.messages[0].freeze(f.turn.session_id, f.at).expect("root");
    let prior_bundle = turns::load_run(store.database.connection(), f.turn.agent_run_id)
        .expect("run")
        .prompt_bundle_snapshot_id;
    let mut next_bundle = aura_persistence_v2::agent::model_context::bundle(
        store.database.connection(),
        prior_bundle,
        f.turn.session_id,
    )
    .expect("bundle");
    next_bundle.id = PromptBundleSnapshotId::new();
    next_bundle.turn_task_id = TurnTaskId::new();
    let bundle = next_bundle.id;
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::context::insert_bundle(tx, f.turn.session_id, &next_bundle)
        })
        .expect("new turn bundle");
    let failed: StoreResult<()> = store.database.unit_of_work(|tx| {
        let turn = lifecycle::start(tx, &user, AgentTaskMode::Normal, None, bundle)?;
        input_consumption::start_batch(tx, &pending, &turn, f.at)?;
        Err(Error::Invalid("simulated crash before commit"))
    });
    assert!(failed.is_err());
    drop(store);
    let mut store = V2Store::open(&path).expect("restart");
    assert_eq!(
        inbox::load(store.database.connection(), f.turn.session_id).expect("queue intact"),
        pending
    );
    assert!(
        session::user_message(store.database.connection(), f.turn.session_id, user.event_id)
            .is_err()
    );
    let turn = store
        .database
        .unit_of_work(|tx| {
            let turn = lifecycle::start(tx, &user, AgentTaskMode::Normal, None, bundle)?;
            input_consumption::start_batch(tx, &pending, &turn, f.at)?;
            Ok::<_, Error>(turn)
        })
        .expect("atomic consumption");
    assert!(
        store
            .database
            .unit_of_work(|tx| input_consumption::start_batch(tx, &pending, &turn, f.at))
            .is_err(),
        "consumed CAS cannot replay"
    );
    let ids = pending.messages.iter().map(|m| m.message_id).collect::<Vec<_>>();
    assert_eq!(
        inbox::consumed(store.database.connection(), turn.turn_task_id).expect("batch")[0]
            .user_event_ids,
        ids
    );
    let ordered =
        session::ordered_messages(store.database.connection(), f.turn.session_id, ids.clone())
            .expect("source order");
    assert_eq!(ordered.iter().map(|m| m.event_id).collect::<Vec<_>>(), ids);
    assert_eq!(
        ordered.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
        ["first", "/plan remains plain input", "third"]
    );
    assert!(
        inbox::load(store.database.connection(), f.turn.session_id)
            .expect("consumed")
            .messages
            .is_empty()
    );
    let projection = aura_persistence_v2::agent::desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            within_turn: None,
            session_id: f.turn.session_id,
            before_turn_task_id: None,
            limit: 1,
        },
        RuntimeInstanceId::new(),
    )
    .expect("history");
    assert_eq!(projection.turns.len(), 1);
    assert_eq!(projection.turns[0].user_messages.len(), ordered.len());
    for (view, source) in projection.turns[0].user_messages.iter().zip(&ordered) {
        assert_eq!(view.event_id, source.event_id);
        assert_eq!(view.text, source.text);
        assert_eq!(view.total_bytes, source.text.len() as u64);
        assert_eq!(view.origin, source.origin);
    }
    assert_eq!(projection.turns[0].turn.mode, AgentTaskMode::Normal);
    let older = aura_persistence_v2::agent::desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            within_turn: None,
            session_id: f.turn.session_id,
            before_turn_task_id: projection.next_before_turn_task_id,
            limit: 1,
        },
        RuntimeInstanceId::new(),
    )
    .expect("older");
    assert_eq!(older.turns[0].turn.turn_task_id, f.turn.turn_task_id);
    assert!(
        aura_persistence_v2::agent::desktop::projection(
            store.database.connection(),
            &AgentHistoryRequest {
                within_turn: None,
                session_id: SessionId::new(),
                before_turn_task_id: Some(turn.turn_task_id),
                limit: 1,
            },
            RuntimeInstanceId::new()
        )
        .is_err(),
        "cursor cannot cross sessions"
    );
}

#[test]
fn next_turn_never_becomes_query_or_special_mode_input() {
    for mode in [AgentTaskMode::Normal, AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let root = tempfile::tempdir().expect("directory");
        let mut store = V2Store::create(&root.path().join("v2")).expect("store");
        let f = support::Fixture::new(&mut store, mode);
        let command = CommandInvocationId::new();
        let request = AgentSubmitRequest {
            session_id: f.turn.session_id,
            text: "/goal ignored as a mode command".into(),
            destination: AgentInputLane::NextTurn,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        };
        let queued = store
            .database
            .unit_of_work(|tx| inbox::submit(tx, command, &request, f.at))
            .expect("submit");
        let repeat = store
            .database
            .unit_of_work(|tx| inbox::submit(tx, command, &request, f.at))
            .expect("idempotent");
        assert_eq!(repeat, queued);
        let mut replaced = request.clone();
        replaced.text = "replacement".into();
        assert!(
            store.database.unit_of_work(|tx| inbox::submit(tx, command, &replaced, f.at)).is_err()
        );
        assert_eq!(
            turns::load(store.database.connection(), f.turn.turn_task_id).expect("turn").mode,
            mode
        );
        assert!(!inbox::pending_additions(store.database.connection(), &f.turn).expect("pending"));
        assert!(
            session::user_message(
                store.database.connection(),
                f.turn.session_id,
                queued.messages[0].message_id
            )
            .is_err()
        );
        assert!(serde_json::from_value::<AgentSubmitRequest>(serde_json::json!({"session_id":f.turn.session_id,"text":"hello","destination":{"lane":"next_turn"},"mode":"goal"})).is_err());
        assert!(serde_json::from_value::<AgentSubmitRequest>(serde_json::json!({"session_id":f.turn.session_id,"text":"hello","destination":{"lane":"next_turn"},"model_profile_id":ModelProfileId::new()})).is_err());
    }
}

#[test]
fn additions_freeze_independent_sources_only_at_a_safe_point() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Plan);
    let mut ids = vec![];
    for text in ["First addition", "Second addition"] {
        let p = store
            .database
            .unit_of_work(|tx| {
                inbox::submit(
                    tx,
                    CommandInvocationId::new(),
                    &AgentSubmitRequest {
                        session_id: f.turn.session_id,
                        text: text.into(),
                        destination: AgentInputLane::Addition { turn_task_id: f.turn.turn_task_id },
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                    f.at,
                )
            })
            .expect("append");
        ids.push(p.messages.last().expect("message").message_id);
    }
    assert!(
        store
            .database
            .unit_of_work(|tx| input_consumption::additions(tx, f.turn.agent_run_id, f.at))
            .is_err(),
        "model request is already in flight"
    );
    store
        .database
        .unit_of_work(|tx| {
            let old = turns::load_run(tx, f.turn.agent_run_id)?;
            let mut next = old.clone();
            next.revision = Revision::new(old.revision.value() + 1);
            next.state = AgentRunState::PreparingContext;
            turns::save_run(tx, &old, &next)?;
            input_consumption::additions(tx, f.turn.agent_run_id, f.at)
        })
        .expect("safe point");
    let consumed =
        inbox::consumed(store.database.connection(), f.turn.turn_task_id).expect("relations");
    assert_eq!(consumed[0].user_event_ids, ids);
    for id in &ids {
        let message = session::user_message(store.database.connection(), f.turn.session_id, *id)
            .expect("original");
        assert_eq!(message.origin, UserMessageOrigin::Steer);
    }
    let line =
        aura_persistence_v2::agent::lineage::load(store.database.connection(), f.turn.lineage_id)
            .expect("lineage");
    assert_eq!(line.sources.len(), 3);
    assert_eq!(
        turns::load(store.database.connection(), f.turn.turn_task_id).expect("turn").mode,
        AgentTaskMode::Plan
    );
    let pending = inbox::load(store.database.connection(), f.turn.session_id).expect("inbox");
    assert!(pending.messages.is_empty());
    assert!(
        store
            .database
            .unit_of_work(|tx| inbox::mutate(
                tx,
                CommandInvocationId::new(),
                &AgentInboxMutation {
                    session_id: f.turn.session_id,
                    expected_revision: pending.revision,
                    action: AgentInboxAction::Revoke { message_id: ids[0] }
                },
                f.at
            ))
            .is_err(),
        "consumed history cannot be revoked"
    );
}

#[test]
fn stale_routing_and_restart_preserve_input_and_pause_queue() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
    let submitted = store
        .database
        .unit_of_work(|tx| {
            inbox::submit(
                tx,
                CommandInvocationId::new(),
                &AgentSubmitRequest {
                    session_id: f.turn.session_id,
                    text: "Keep this message".into(),
                    destination: AgentInputLane::Addition { turn_task_id: TurnTaskId::new() },
                    skill_ids: Vec::new(),
                    reasoning_effort: None,
                    attachments: Vec::new(),
                },
                f.at,
            )
        })
        .expect("stale target becomes queued");
    assert_eq!(submitted.messages[0].destination, AgentInputLane::NextTurn);
    let edit = AgentInboxMutation {
        session_id: f.turn.session_id,
        expected_revision: submitted.revision,
        action: AgentInboxAction::Edit {
            message_id: submitted.messages[0].message_id,
            text: "Edited instruction".into(),
        },
    };
    store
        .database
        .unit_of_work(|tx| inbox::mutate(tx, CommandInvocationId::new(), &edit, f.at))
        .expect("edit");
    assert!(
        store
            .database
            .unit_of_work(|tx| inbox::mutate(tx, CommandInvocationId::new(), &edit, f.at))
            .is_err(),
        "stale revision"
    );
    store.database.unit_of_work(|tx| inbox::startup(tx, f.at)).expect("startup");
    let after = inbox::load(store.database.connection(), f.turn.session_id).expect("durable queue");
    assert!(after.paused);
    assert_eq!(after.messages[0].message_id, submitted.messages[0].message_id);
    assert_eq!(after.messages[0].text, "Edited instruction");
    assert_eq!(after.messages[0].revision, Revision::new(2));
    assert_eq!(after.messages[0].destination, AgentInputLane::NextTurn);
}

/// A restart, stop or failure pauses the queue so already-queued input is never
/// sent without an explicit resume. An empty queue holds nothing to protect, so
/// the next authenticated send must start instead of inheriting that pause.
#[test]
fn a_restart_pause_gates_only_input_that_is_already_queued() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = f.turn.session_id;
    let first = store
        .database
        .unit_of_work(|tx| {
            inbox::submit(
                tx,
                CommandInvocationId::new(),
                &AgentSubmitRequest {
                    session_id: session,
                    text: "Drain me".into(),
                    destination: AgentInputLane::NextTurn,
                    skill_ids: Vec::new(),
                    reasoning_effort: None,
                    attachments: Vec::new(),
                },
                f.at,
            )
        })
        .expect("first queued message");
    // The user removes it, so the session keeps an inbox fact with no pending
    // input - exactly the state a restart leaves behind.
    store
        .database
        .unit_of_work(|tx| {
            inbox::mutate(
                tx,
                CommandInvocationId::new(),
                &AgentInboxMutation {
                    session_id: session,
                    expected_revision: first.revision,
                    action: AgentInboxAction::Remove { message_id: first.messages[0].message_id },
                },
                f.at,
            )
        })
        .expect("remove");
    store.database.unit_of_work(|tx| inbox::startup(tx, f.at)).expect("startup");
    let restarted = inbox::load(store.database.connection(), session).expect("durable queue");
    assert!(restarted.paused, "the restart still records its pause");
    assert!(restarted.messages.is_empty());
    let resumed = store
        .database
        .unit_of_work(|tx| {
            inbox::submit(
                tx,
                CommandInvocationId::new(),
                &AgentSubmitRequest {
                    session_id: session,
                    text: "New explicit send".into(),
                    destination: AgentInputLane::NextTurn,
                    skill_ids: Vec::new(),
                    reasoning_effort: None,
                    attachments: Vec::new(),
                },
                f.at,
            )
        })
        .expect("send after restart");
    assert!(!resumed.paused, "an empty paused queue must not gate a new send");
    assert_eq!(resumed.pause_reason, None);
    assert_eq!(resumed.messages.len(), 1);
    // Queued input still holds the pause: the new message must not jump ahead of
    // it, and a further send joins the same paused queue.
    store.database.unit_of_work(|tx| inbox::startup(tx, f.at)).expect("second restart");
    let queued = store
        .database
        .unit_of_work(|tx| {
            inbox::submit(
                tx,
                CommandInvocationId::new(),
                &AgentSubmitRequest {
                    session_id: session,
                    text: "Second queued message".into(),
                    destination: AgentInputLane::NextTurn,
                    skill_ids: Vec::new(),
                    reasoning_effort: None,
                    attachments: Vec::new(),
                },
                f.at,
            )
        })
        .expect("second queued message");
    assert!(queued.paused, "pending input keeps the queue paused for an explicit resume");
    let texts = queued.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>();
    assert_eq!(texts, vec!["New explicit send", "Second queued message"]);
}
