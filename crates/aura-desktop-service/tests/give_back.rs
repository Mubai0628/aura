//! The user gives the desktop back after taking it over: the turn acts again in a new hold, and a
//! turn that ended taken over stays listed until the user gives the desktop back or the
//! session's next turn runs.
mod support;

use aura_desktop_domain::*;
use aura_domain_common::{SessionId, TurnTaskId};
use support::Stand;
use support::turns::*;

#[tokio::test]
async fn giving_the_desktop_back_lets_the_turn_act_again_in_a_new_hold() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), TurnTaskId::new());
    setup.control.turn_runs(session, turn);
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.stand.say(&taken_over(&turn.to_string()));
    until(|| setup.control.taken_over(session, turn)).await;
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect_err("refused");

    setup.control.give_back(session).await;
    assert!(!setup.control.taken_over(session, turn));
    assert_eq!(setup.control.takeover_cause(turn), None);
    assert!(setup.control.controlling(session).is_empty(), "nothing kept until it acts again");
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("acts again");
    let again = format!("{turn}#1");
    assert_eq!(setup.stand.sent().last(), Some(&kept_as(&again)));
    assert_eq!(setup.control.controlling(session), [notepad_controlled(false)]);

    // Taken over and given back once more: another new hold.
    setup.stand.say(&taken_over(&again));
    until(|| setup.control.taken_over(session, turn)).await;
    assert_eq!(setup.control.controlling(session), [notepad_controlled(true)]);
    setup.control.give_back(session).await;
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("acts again");
    let third = format!("{turn}#2");
    assert_eq!(setup.stand.sent().last(), Some(&kept_as(&third)));

    setup.control.turn_ended(session, turn).await;
    assert_eq!(setup.stand.sent().last(), Some(&HostCommand::Release { hold: third }));
    assert!(setup.control.controlling(session).is_empty());
}

#[tokio::test]
async fn a_turn_that_ended_taken_over_stays_listed_until_the_desktop_is_given_back() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), TurnTaskId::new());
    setup.control.turn_runs(session, turn);
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.stand.say(&taken_over(&turn.to_string()));
    until(|| setup.control.taken_over(session, turn)).await;
    setup.control.turn_ended(session, turn).await;
    assert_eq!(setup.control.controlling(session), [notepad_controlled(true)]);
    assert!(setup.control.controlling(SessionId::new()).is_empty());

    let sent = setup.stand.sent().len();
    setup.control.give_back(session).await;
    assert!(setup.control.controlling(session).is_empty());
    assert_eq!(setup.stand.sent().len(), sent, "nothing was kept to give back");
}

#[tokio::test]
async fn the_sessions_next_turn_running_ends_the_listing_of_a_turn_that_ended_taken_over() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn, next) = (SessionId::new(), TurnTaskId::new(), TurnTaskId::new());
    setup.control.turn_runs(session, turn);
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.stand.say(&taken_over(&turn.to_string()));
    until(|| setup.control.taken_over(session, turn)).await;
    setup.control.turn_ended(session, turn).await;
    setup.control.turn_runs(SessionId::new(), TurnTaskId::new());
    assert_eq!(setup.control.controlling(session), [notepad_controlled(true)], "another session");
    setup.control.turn_runs(session, next);
    assert!(setup.control.controlling(session).is_empty());
    setup.control.run_for(typing(), LIMIT, caller(session, next)).await.expect("the next turn");
    assert_eq!(setup.stand.sent().last(), Some(&kept(next)));
}

#[tokio::test]
async fn the_end_of_a_taken_over_turn_heard_after_the_next_turn_began_lists_nothing() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn, next) = (SessionId::new(), TurnTaskId::new(), TurnTaskId::new());
    setup.control.turn_runs(session, turn);
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.stand.say(&taken_over(&turn.to_string()));
    until(|| setup.control.taken_over(session, turn)).await;
    setup.control.turn_runs(session, next);
    setup.control.turn_ended(session, turn).await;
    assert!(setup.control.controlling(session).is_empty());
}

#[tokio::test]
async fn giving_back_reaches_only_its_own_session() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), TurnTaskId::new());
    let sent = setup.stand.sent().len();
    setup.control.give_back(session).await;
    assert_eq!(setup.stand.sent().len(), sent, "nothing taken over, nothing to do");

    setup.control.turn_runs(session, turn);
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.stand.say(&taken_over(&turn.to_string()));
    until(|| setup.control.taken_over(session, turn)).await;
    setup.control.give_back(SessionId::new()).await;
    assert!(setup.control.taken_over(session, turn), "another session gives nothing back");
    assert_eq!(setup.control.controlling(session), [notepad_controlled(true)]);
}

#[tokio::test]
async fn stopping_the_session_ends_the_listing_of_a_turn_that_ended_taken_over() {
    let setup = setup(Stand::new(ordinary));
    let (session, turn) = (SessionId::new(), TurnTaskId::new());
    setup.control.turn_runs(session, turn);
    setup.control.run_for(typing(), LIMIT, caller(session, turn)).await.expect("typed");
    setup.stand.say(&taken_over(&turn.to_string()));
    until(|| setup.control.taken_over(session, turn)).await;
    setup.control.turn_ended(session, turn).await;
    setup.control.stop(session, NOTEPAD);
    setup.control.release(session).await;
    assert!(setup.control.controlling(session).is_empty());
}
