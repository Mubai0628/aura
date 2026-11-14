//! What the port keeps while Aura runs: who waits in `agents.wait`, whose model answer is under
//! way, and how far each conversation's tokens are counted.
use aura_agent_team::{Tally, TeamPort};
use aura_domain_common::SessionId;

#[test]
fn a_conversation_waits_while_any_of_its_waits_is_under_way() {
    let port = TeamPort::default();
    let (session, other) = (SessionId::new(), SessionId::new());
    assert!(!port.is_waiting(session));
    let first = port.waiting(session);
    let second = port.waiting(session);
    assert!(port.is_waiting(session));
    assert!(!port.is_waiting(other));
    drop(first);
    assert!(port.is_waiting(session), "one wait is still under way");
    drop(second);
    assert!(!port.is_waiting(session));
}

#[test]
fn a_conversation_is_answering_while_any_of_its_model_answers_is_under_way() {
    let port = TeamPort::default();
    let (session, other) = (SessionId::new(), SessionId::new());
    assert!(!port.is_answering(session));
    let first = port.answering(session);
    let second = port.answering(session);
    assert!(port.is_answering(session));
    assert!(!port.is_answering(other));
    // Waiting and answering are told apart.
    assert!(!port.is_waiting(session));
    drop(first);
    assert!(port.is_answering(session), "one answer is still under way");
    drop(second);
    assert!(!port.is_answering(session));
}

#[test]
fn a_tally_moves_only_from_the_count_it_was_read_at() {
    let port = TeamPort::default();
    let session = SessionId::new();
    let start = port.tally(session);
    assert_eq!(start, Tally::default());
    let first = Tally { after: 10, tokens: 300 };
    assert_eq!(port.settle_tally(session, start, first), first);
    // A count that also began at the start finished second: it adds nothing twice.
    let late = Tally { after: 10, tokens: 600 };
    assert_eq!(port.settle_tally(session, start, late), first);
    assert_eq!(port.tally(session), first);
    let next = Tally { after: 12, tokens: 420 };
    assert_eq!(port.settle_tally(session, first, next), next);
    assert_eq!(port.tally(SessionId::new()), Tally::default());
}
