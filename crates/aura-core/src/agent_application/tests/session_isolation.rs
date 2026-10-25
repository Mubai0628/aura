//! One session's held start must not delay another session's ordinary input.
//!
//! WHY THESE CASES EXIST
//! `agent_execution::broker::invoke` takes `permission_start_lease` and held it, process-wide,
//! until the tool's terminal receipt. `AgentApplication::submit` / `mutate_inbox` and a session's
//! own mode change all take the write side of that same gate, so a slow tool in session A delayed
//! session B's ordinary message enqueue and B's own permission change even though B used none of
//! A's capabilities or resources. The original D7 gate recorded the real first failure of exactly
//! this chain: `permission.set_session_mode` timed out during A's hold.
//!
//! WHAT THESE CASES MEASURE, AND WHAT THEY PROVE
//! * the hold is the real product call on the real running Core's coordinator - the same
//!   `permission_start_lease` the tool path takes - bound to session A.
//! * the verified actions are the ordinary product input paths, not `agent.runs.start`: submit,
//!   the inbox mutation that a queue edit really performs, and B's own permission mode change.
//! * each is measured separately, with its own bound, its own resulting fact and its own side
//!   effect, and A's authority is shown to be untouched.
//! * the same-session ordering is not weakened: A's own mode change still cannot commit inside
//!   A's live start.
use super::*;
use aura_security_contract::PermissionMode;
use std::time::{Duration, Instant};

/// The budget each ordinary input keeps in this case. It is the caller's own bound: the point is
/// that the answer arrives inside it, not that the action is fast.
const INPUT_BUDGET: Duration = Duration::from_secs(5);

/// One workspace with two independent sessions, each with its own configured Provider.
async fn two_sessions() -> (crate::r03_tests::Fixture, crate::RuntimeState, SessionId, SessionId) {
    let fixture = crate::r03_tests::Fixture::new();
    let runtime = fixture.start().await;
    let root = fixture.root.path().join("s04-r6-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"S04 R6"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace summary");
    let session_a =
        create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let session_b =
        create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    (fixture, runtime, session_a, session_b)
}

/// R6 - while session A's start is held, B's ordinary input, B's inbox mutation and B's own mode
/// change each obtain their correct fact inside their own budget.
#[tokio::test]
async fn r6_a_held_start_of_one_session_does_not_delay_another_sessions_ordinary_input() {
    let (_fixture, runtime, session_a, session_b) = two_sessions().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let before_a = services.persistence.session_permission_state(session_a).expect("A's state");
    // The real hold: the same product call the tool path takes, bound to session A.
    let held = services.persistence.permission_start_lease(session_a).await;

    // 1. B's ordinary message enqueue.
    let began = Instant::now();
    let inbox = tokio::time::timeout(
        INPUT_BUDGET,
        services.agent_required().submit(
            CommandInvocationId::new(),
            AgentSubmitRequest {
                session_id: session_b,
                text: "B's ordinary message while A is held".into(),
                destination: AgentInputLane::NextTurn,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                attachments: Vec::new(),
            },
        ),
    )
    .await
    .expect("B's submit must answer inside its own budget, not behind A's held start")
    .expect("B's submit is served");
    let submit_ms = began.elapsed().as_millis();
    assert_eq!(inbox.session_id, session_b);
    assert!(
        inbox.messages.iter().any(|m| m.text == "B's ordinary message while A is held"),
        "the enqueue really happened: {inbox:?}"
    );

    // 2. The inbox mutation a queue edit really performs, on the same session.
    let began = Instant::now();
    let mutated = tokio::time::timeout(
        INPUT_BUDGET,
        services.agent_required().mutate_inbox(
            CommandInvocationId::new(),
            AgentInboxMutation {
                session_id: session_b,
                expected_revision: inbox.revision,
                action: AgentInboxAction::Pause,
            },
        ),
    )
    .await
    .expect("B's inbox mutation must answer inside its own budget")
    .expect("B's inbox mutation is served");
    let mutate_ms = began.elapsed().as_millis();
    assert!(mutated.paused, "the queue really paused: {mutated:?}");

    // 3. B's own permission mode change, through the same call the mode route performs.
    let before_b = services.persistence.session_permission_state(session_b).expect("B's state");
    let began = Instant::now();
    let changed = tokio::time::timeout(
        INPUT_BUDGET,
        services.persistence.change_session_permission_mode(
            session_b,
            before_b.revision,
            PermissionMode::WorkspaceReadWrite,
            CorrelationId::new(),
        ),
    )
    .await
    .expect("B's own mode change must answer inside its own budget")
    .expect("B's mode change commits");
    let change_ms = began.elapsed().as_millis();
    assert_eq!(changed.mode, PermissionMode::WorkspaceReadWrite);
    assert_eq!(changed.revision, before_b.revision + 1, "a real revision bump, not a fake success");

    // A is untouched by B's control: same mode, same revision.
    let after_a = services.persistence.session_permission_state(session_a).expect("A's state");
    assert_eq!(after_a.mode, PermissionMode::ReadOnly);
    assert_eq!(after_a.revision, before_a.revision, "B's control did not move A's authority");

    // 4. The same-session ordering is not weakened: A's own change still waits for A's start.
    let same = services.persistence.change_session_permission_mode(
        session_a,
        after_a.revision,
        PermissionMode::WorkspaceReadWrite,
        CorrelationId::new(),
    );
    tokio::pin!(same);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut same).await.is_err(),
        "A's own mode change must not commit inside A's live start"
    );
    drop(held);
    let settled = same.await.expect("A's change commits once its own start is over");
    assert_eq!(settled.mode, PermissionMode::WorkspaceReadWrite);

    println!(
        "R6 session_a_start_held=true submit_ms={submit_ms} mutate_inbox_ms={mutate_ms} \
         own_mode_change_ms={change_ms} b_revision={} a_revision={} same_session_excluded=true",
        changed.revision, settled.revision
    );
}

/// R6 counterfactual - with the one shared gate of the pre-R6 code injected, the identical input
/// really does wait behind A's held start. This is what makes the case above evidence about the
/// per-session binding rather than about a fast machine.
#[tokio::test]
async fn r6_counterfactual_a_shared_gate_really_delays_another_sessions_input() {
    let (_fixture, runtime, session_a, session_b) = two_sessions().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.persistence.force_shared_gate_for_test(true);
    let held = services.persistence.permission_start_lease(session_a).await;
    let before_b = services.persistence.session_permission_state(session_b).expect("B's state");
    let blocked = services.persistence.change_session_permission_mode(
        session_b,
        before_b.revision,
        PermissionMode::WorkspaceReadWrite,
        CorrelationId::new(),
    );
    tokio::pin!(blocked);
    let waited = tokio::time::timeout(Duration::from_millis(200), &mut blocked).await.is_err();
    println!("R6 counterfactual shared_gate=true b_change_blocked_while_a_held={waited}");
    assert!(
        waited,
        "the counterfactual must really delay the unrelated session, otherwise the R6 case \
         proves nothing"
    );
    drop(held);
    let changed = blocked.await.expect("B's change commits once the shared gate is free");
    assert_eq!(changed.mode, PermissionMode::WorkspaceReadWrite);
    assert_eq!(changed.revision, before_b.revision + 1);
}
