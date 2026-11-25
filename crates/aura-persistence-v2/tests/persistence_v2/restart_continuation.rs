//! A restart that cuts a turn off returns it from the start, and Aura continues it by itself at
//! most twice in a row in half an hour, and not when Core failed again within two minutes of
//! continuing it.
#![allow(clippy::unwrap_used)]
use crate::support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    agent::{
        application, recovery,
        restart_continuation::{self as continuation, Decision},
    },
};

fn configure(store: &mut V2Store, f: &support::Fixture) {
    store
        .database
        .unit_of_work(|tx| {
            application::configure(
                tx,
                f.turn.agent_run_id,
                &application::RunConfiguration {
                    request: AgentStartRequest {
                        computer_mode: None,
                        session_id: f.turn.session_id,
                        model_profile_id: ModelProfileId::new(),
                        text: "synthetic restart".into(),
                        mode: AgentTaskMode::Normal,
                        goal_budget: None,
                        explicit_backend_id: None,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        speed_mode: None,
                        attachments: Vec::new(),
                        references: Vec::new(),
                    },
                    tool_settings: aura_tool_contract::ToolSettingsSnapshotV1::default(),
                    purpose: AgentTaskPurpose::Conversation,
                },
                CommandInvocationId::new(),
                f.at,
            )
        })
        .expect("configured run");
}

fn minutes_after(at: UtcTimestamp, minutes: i64) -> UtcTimestamp {
    UtcTimestamp::from_value(at.value() + std::time::Duration::from_secs(minutes as u64 * 60))
}

#[test]
fn a_cut_off_turn_is_continued_at_most_twice_in_half_an_hour_and_not_in_a_restart_loop() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v2");
    let mut store = V2Store::create(&path).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    configure(&mut store, &f);
    drop(store);

    let mut store = V2Store::open(&path).unwrap();
    let cut = store.database.unit_of_work(|tx| recovery::startup(tx, f.at)).unwrap();
    let turn = f.turn.turn_task_id;
    assert_eq!(
        cut,
        vec![recovery::InterruptedTurn {
            session_id: f.turn.session_id,
            turn_task_id: turn,
            agent_run_id: f.turn.agent_run_id,
        }],
        "the start returns the turn it cut off"
    );
    let decide = |store: &V2Store, minutes: i64| {
        continuation::decide(store.database.connection(), turn, minutes_after(f.at, minutes))
            .unwrap()
    };
    assert_eq!(decide(&store, 5), Decision::Continue);
    assert_eq!(continuation::view(store.database.connection(), turn).unwrap(), None);

    // The cut-off turn was itself Aura's continuation of an earlier one.
    let earlier = TurnTaskId::new();
    store
        .database
        .unit_of_work(|tx| {
            continuation::record(tx, turn, earlier, CoreRestartCause::Unresponsive, f.at, f.at)
        })
        .unwrap();
    assert_eq!(
        continuation::view(store.database.connection(), turn).unwrap(),
        Some(AgentAutoContinuedView { cause: CoreRestartCause::Unresponsive, restarted_at: f.at }),
        "the conversation can say why and when"
    );
    assert_eq!(decide(&store, 1), Decision::RestartLoop, "cut off again within two minutes");
    assert_eq!(decide(&store, 5), Decision::Continue, "once in a row is allowed again");

    // And that earlier one was a continuation too, a minute later: twice in half an hour.
    let first = TurnTaskId::new();
    store
        .database
        .unit_of_work(|tx| {
            continuation::record(
                tx,
                earlier,
                first,
                CoreRestartCause::Crash,
                minutes_after(f.at, 1),
                f.at,
            )
        })
        .unwrap();
    assert_eq!(decide(&store, 5), Decision::LimitReached);
    assert_eq!(decide(&store, 40), Decision::Continue, "half an hour later the count starts again");
}

#[test]
fn only_the_cut_off_last_turn_of_a_session_is_continued() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    // Still running: nothing cut it off.
    let decision = continuation::decide(store.database.connection(), f.turn.turn_task_id, f.at);
    assert_eq!(decision.unwrap(), Decision::Moved);
    assert!(!continuation::cut_off(store.database.connection(), f.turn.turn_task_id).unwrap());
}

#[test]
fn only_unsettled_peer_sends_of_the_interrupted_run_stop_automatic_continuation() {
    use Decision::{Continue, UnsettledPeerDelivery};
    use aura_execution_contract::SideEffectStateV1::{FullyApplied, NoEffect, Unknown};

    for (kind, state, receipt, same_run, expected) in [
        ("peers.dispatch", "running", None, false, Continue),
        ("peers.list", "running", None, true, Continue),
        ("peers.dispatch", "running", None, true, UnsettledPeerDelivery),
        ("peers.ask", "failed", Some(Unknown), true, UnsettledPeerDelivery),
        ("peers.ask", "failed", Some(NoEffect), true, Continue),
        ("peers.dispatch", "succeeded", Some(FullyApplied), true, Continue),
    ] {
        assert_eq!(
            decision_with_peer_tool(kind, state, receipt, same_run),
            expected,
            "{kind} {state} {receipt:?}, same_run={same_run}"
        );
    }
}

fn decision_with_peer_tool(
    kind: &str,
    state: &str,
    receipt: Option<aura_execution_contract::SideEffectStateV1>,
    same_run: bool,
) -> Decision {
    let temp = tempfile::tempdir().expect("temp store");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("test store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    configure(&mut store, &f);
    store.database.unit_of_work(|tx| recovery::startup(tx, f.at)).expect("cut-off turn");
    let op = SecurityOperationId::new().to_string();
    let batch = ToolBatchId::new().to_string();
    let registry = ToolRegistrySnapshotId::new().to_string();
    let run = if same_run { f.turn.agent_run_id } else { AgentRunId::new() };
    let invocation =
        serde_json::json!({"batch":{"invocation_identity":{"agent_run_id":run}}}).to_string();
    store
        .database
        .unit_of_work(|tx| -> aura_persistence_core::StoreResult<Decision> {
            let at = f.at.to_string();
            tx.execute(
                "INSERT INTO tool_registry_snapshots VALUES(?1,'fixture','{}')",
                [&registry],
            )?;
            tx.execute(
                "INSERT INTO tool_batches VALUES(?1,?2,?3,'running',1,?4)",
                rusqlite::params![batch, f.turn.session_id.to_string(), registry, invocation],
            )?;
            tx.execute(
                "INSERT INTO operations VALUES(?1,'execution',?2,'fixture',1,'{}',1,?3,?4,?4)",
                rusqlite::params![op, kind, state, at],
            )?;
            tx.execute(
                "INSERT INTO tool_calls VALUES(?1,?2,1,?3,'{}')",
                rusqlite::params![ToolCallId::new().to_string(), batch, op],
            )?;
            if let Some(side_effect) = receipt {
                let artifact = ArtifactId::new();
                aura_persistence_core::artifact::put(
                    tx,
                    artifact,
                    &op,
                    "text/plain",
                    b"fixture receipt",
                    f.at,
                )?;
                tx.execute(
                    "INSERT INTO execution_attempts VALUES(?1,1,'fixture','terminal',1,'{}')",
                    [&op],
                )?;
                tx.execute(
                    "INSERT INTO execution_results VALUES(?1,1,?2,?3,?4,'{}')",
                    rusqlite::params![
                        op,
                        aura_persistence_core::record::encode(&state)?,
                        aura_persistence_core::record::encode(&side_effect)?,
                        artifact.to_string()
                    ],
                )?;
            }
            continuation::decide(tx, f.turn.turn_task_id, f.at)
        })
        .expect("isolated continuation and receipt fixture")
}

#[test]
fn a_continuation_record_written_by_a_later_version_is_no_record() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    store
        .database
        .connection()
        .execute(
            "INSERT INTO events(event_id, owner, kind, version, payload, created_at)              VALUES(?1, ?2, 'agent.auto_continued', 1, ?3, ?4)",
            rusqlite::params![
                EventId::new().to_string(),
                f.turn.turn_task_id.to_string(),
                r#"{"cause":"cosmic_ray","restarted_at":"2026-09-07T00:00:00Z","after_turn_task_id":"x"}"#,
                f.at.to_string(),
            ],
        )
        .unwrap();
    assert_eq!(continuation::view(store.database.connection(), f.turn.turn_task_id).unwrap(), None);
}
