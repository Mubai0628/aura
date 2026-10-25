//! A long conversation's branch opens with its newest
//! turns while the older ones follow, and in the end it is sent exactly its source's history.
use super::*;
use crate::agent_application::branch::FIRST_TURNS;

async fn wait_for(what: &str, mut done: impl AsyncFnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !done().await {
        assert!(std::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_long_conversation_opens_its_branch_with_its_newest_turns_and_the_rest_follows() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Branching).await;
    let (runtime, source, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    turn(&runtime, source, model, FIRST).await;
    for n in 0..(FIRST_TURNS + 1) {
        turn(&runtime, source, model, &format!("AURA-L1014-TURN-{n}: answer briefly")).await;
    }
    let source_turns: Vec<TurnTaskId> =
        history(&runtime, source).await.turns.iter().map(|t| t.turn.turn_task_id).collect();
    assert_eq!(source_turns.len(), FIRST_TURNS + 2);
    let last = *source_turns.last().expect("a turn");

    // The older turns' copy is held, so what the branch opened with can be looked at.
    let hold =
        services(&runtime).agent_required().store().arm_hold("agent.session.branch_copy", 30_000);
    let made = branch(&runtime, source, last).await;
    let branch_id = made.session_id;
    wait_for("the background copy starts", async || hold.was_reached()).await;
    let page = history(&runtime, branch_id).await;
    let copying = page.branch_copying.clone().expect("the older turns are still copied");
    assert_eq!(
        (copying.copied_turns as usize, copying.total_turns as usize, copying.failed),
        (FIRST_TURNS, source_turns.len(), false)
    );
    let shown: Vec<TurnTaskId> = page.turns.iter().map(|t| t.turn.turn_task_id).collect();
    assert_eq!(
        shown,
        source_turns[source_turns.len() - FIRST_TURNS..],
        "the newest turns, in order"
    );
    assert!(page.turns.iter().all(|t| t.imported));
    assert!(page.branch_origin.is_some());

    // The rest follows; a message sent at once is answered only from the whole history.
    hold.release_all();
    turn(&runtime, branch_id, model, NEXT).await;
    wait_for("the copy finishes", async || {
        history(&runtime, branch_id).await.branch_copying.is_none()
    })
    .await;
    let page = history(&runtime, branch_id).await;
    let carried: Vec<TurnTaskId> =
        page.turns.iter().filter(|t| t.imported).map(|t| t.turn.turn_task_id).collect();
    assert_eq!(carried, source_turns, "every turn, oldest first");

    // The branch was sent exactly what its source is sent at the same point.
    turn(&runtime, source, model, NEXT).await;
    let sent = requests(&provider);
    let [.., in_branch, in_source] = sent.as_slice() else { panic!("two continuations") };
    assert_eq!(history_part(in_branch), history_part(in_source));
}
