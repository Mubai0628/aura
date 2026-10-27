//! A call that
//! ended before it started still gets its result, whichever tool it was, and one run that cannot be
//! settled after a restart no longer keeps the others, and the whole Agent, from starting.
#![allow(clippy::unwrap_used)]
#[allow(dead_code)] // Shared transaction fixture also serves other tests.
#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_execution_contract::SideEffectStateV1;
use aura_persistence_v2::{
    V2Store,
    agent::{application, recovery, turns},
    execution, tool,
};
use aura_tool_contract::{ToolCallStatusV1, ToolResultOutcomeV1, ToolResultV1};
use aura_windows_user_contract::*;

fn not_started(result: &ToolResultV1) -> &str {
    let ToolResultOutcomeV1::Failed { error } = &result.outcome else {
        panic!("a call that never started failed: {:?}", result.outcome);
    };
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
    assert!(!result.attempts[0].dispatched, "{result:?}");
    assert!(error.message.contains("was not started"), "{}", error.message);
    &error.message
}

#[test]
fn a_read_refused_because_its_file_changed_tells_the_model_and_changes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch = f.admit(&mut store, vec![("fs.read", serde_json::json!({"path":"game.html"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    let result = store
        .database
        .unit_of_work(|tx| {
            let request = f.reserve_execution(tx, &batch, 0, None)?;
            // As in the owner's turn: a PowerShell command rewrote the file between the read's
            // preparation and its start, so the broker refused to start it.
            execution::reject_prepared(tx, &request, "prepared domain facts changed", f.at)?;
            tool::result::materialize(tx, batch.batch.tool_batch_id, call)
        })
        .expect("a call refused before it started has its result");
    let message = not_started(&result);
    assert!(message.contains("target changed"), "{message}");
    assert!(!message.contains("prepared domain facts"), "the refusal's own words stay inside");
}

#[test]
fn a_command_stopped_before_it_started_has_its_result() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let input = ShellExecuteInput {
        requires_network: false,
        script: "Set-Content game.html 'edited'".into(),
        working_directory: ".".into(),
        stdin: String::new(),
        timeout_ms: 1000,
        max_output_bytes: 4096,
        secret_environment: Vec::new(),
    };
    let batch =
        f.admit(&mut store, vec![("shell.powershell", serde_json::to_value(&input).unwrap())]);
    let call = batch.batch.calls[0].tool_call_id;
    let mut command = input.command(ManagedShell::PowerShell).unwrap();
    command.working_directory = "C:/synthetic".into();
    let result = store
        .database
        .unit_of_work(|tx| {
            let request = f.reserve_execution(
                tx,
                &batch,
                0,
                Some(("shell.powershell", serde_json::to_value(&command)?)),
            )?;
            // The run stopped before the command was started.
            execution::shell::lost(tx, request.operation.operation_id, f.at)?;
            tool::result::materialize(tx, batch.batch.tool_batch_id, call)
        })
        .expect("a command that never started has its result");
    not_started(&result);
}

fn configure(store: &mut V2Store, f: &support::Fixture) {
    store
        .database
        .unit_of_work(|tx| {
            application::configure(
                tx,
                f.turn.agent_run_id,
                &application::RunConfiguration {
                    request: AgentStartRequest {
                        session_id: f.turn.session_id,
                        model_profile_id: ModelProfileId::new(),
                        text: "synthetic restart".into(),
                        mode: AgentTaskMode::Normal,
                        goal_budget: None,
                        explicit_backend_id: None,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
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

#[test]
fn a_run_that_cannot_be_settled_after_a_restart_does_not_hold_back_the_others() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v2");
    let mut store = V2Store::create(&path).unwrap();
    let settled = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/a");
    let stuck = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/b");
    configure(&mut store, &settled);
    configure(&mut store, &stuck);
    // The stuck run's turn no longer reads back as the run's turn, so it cannot be interrupted.
    store
        .database
        .connection()
        .execute(
            "UPDATE turn_tasks SET state='completed' WHERE id=?1",
            [stuck.turn.turn_task_id.to_string()],
        )
        .unwrap();
    let before = turns::load_run(store.database.connection(), stuck.turn.agent_run_id).unwrap();
    drop(store);

    let mut store = V2Store::open(&path).unwrap();
    store
        .database
        .unit_of_work(|tx| recovery::startup(tx, settled.at))
        .expect("the restart goes on whatever one run holds");
    let connection = store.database.connection();
    let run = turns::load_run(connection, settled.turn.agent_run_id).unwrap();
    assert!(matches!(run.state, AgentRunState::RecoveryRequired { .. }), "{:?}", run.state);
    assert_eq!(turns::load_run(connection, stuck.turn.agent_run_id).unwrap(), before);
    let deferred: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM events WHERE owner=?1 AND kind='agent.restart_settlement_deferred'",
            [stuck.turn.agent_run_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(deferred, 1, "why the run is still open is kept");
}

#[test]
fn a_call_left_unanswered_when_its_run_ended_is_not_shown_as_running() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let batch = f.admit(&mut store, vec![("fs.read", serde_json::json!({"path":"game.html"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    store.database.unit_of_work(|tx| f.begin_execution(tx, &batch, 0, None).map(|_| ())).unwrap();
    let state = |store: &V2Store| {
        let request = aura_tool_projection::AgentToolActivityRequest {
            session_id: f.turn.session_id,
            before_call_id: None,
            limit: 10,
            call_ids: Some(vec![call]),
        };
        tool::activity::page(store.database.connection(), &request).unwrap().calls[0].summary.status
    };
    assert_eq!(state(&store), ToolCallStatusV1::Running, "a started call runs");
    // 停止: the read never reported back, so it stays unanswered and the run ends.
    store
        .database
        .unit_of_work(|tx| {
            recovery::interrupt(tx, f.turn.agent_run_id, "agent.cancelled", true, f.at)
        })
        .unwrap();
    assert_eq!(
        state(&store),
        ToolCallStatusV1::RecoveryRequired,
        "its run is over, so it is no longer 运行中"
    );
}
