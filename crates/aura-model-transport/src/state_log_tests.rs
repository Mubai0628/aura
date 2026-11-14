use super::*;
use serde_json::json;

fn items(count: usize, salt: &str) -> Vec<Value> {
    (0..count).map(|n| json!({"type": "message", "text": format!("{salt}-{n}")})).collect()
}

fn placed(leading: &str, changes: &[(usize, &str)]) -> StatePlacement {
    StatePlacement {
        leading: leading.to_owned(),
        changes: changes.iter().map(|(at, state)| (*at, (*state).to_owned())).collect(),
    }
}

#[test]
fn a_conversation_keeps_its_first_state_in_front_and_adds_each_change_where_it_happened() {
    let session = SessionId::new();
    let all = items(8, "a");
    assert_eq!(place_state(session, "rules", &all[..2], "s1"), placed("s1", &[]));
    assert_eq!(place_state(session, "rules", &all[..4], "s1"), placed("s1", &[]));
    assert_eq!(place_state(session, "rules", &all[..4], "s2"), placed("s1", &[(4, "s2")]));
    // The same request again, as a retry sends it, adds nothing.
    assert_eq!(place_state(session, "rules", &all[..4], "s2"), placed("s1", &[(4, "s2")]));
    assert_eq!(place_state(session, "rules", &all[..6], "s2"), placed("s1", &[(4, "s2")]));
    assert_eq!(
        place_state(session, "rules", &all[..8], "s3"),
        placed("s1", &[(4, "s2"), (8, "s3")])
    );
}

#[test]
fn a_conversation_whose_earlier_items_changed_starts_again_from_its_state_then() {
    let session = SessionId::new();
    let all = items(6, "b");
    let _ = place_state(session, "rules", &all[..2], "s1");
    let _ = place_state(session, "rules", &all[..4], "s2");
    let mut rewritten = all.clone();
    rewritten[3] = json!({"type": "message", "text": "summary"});
    assert_eq!(place_state(session, "rules", &rewritten, "s3"), placed("s3", &[]));
    assert_eq!(place_state(session, "rules", &rewritten, "s3"), placed("s3", &[]));
}

#[test]
fn a_request_about_part_of_the_conversation_changes_nothing_kept() {
    let session = SessionId::new();
    let all = items(8, "c");
    let _ = place_state(session, "rules", &all[..2], "s1");
    let _ = place_state(session, "rules", &all[..6], "s2");
    assert_eq!(place_state(session, "rules", &all[..3], "s3"), placed("s1", &[(3, "s3")]));
    assert_eq!(place_state(session, "rules", &all[..8], "s2"), placed("s1", &[(6, "s2")]));
}

#[test]
fn conversations_of_other_sessions_and_other_instructions_are_kept_apart() {
    let (one, other) = (SessionId::new(), SessionId::new());
    let all = items(4, "d");
    let _ = place_state(one, "rules", &all[..2], "s1");
    let _ = place_state(one, "rules", &all[..4], "s2");
    assert_eq!(place_state(other, "rules", &all[..4], "t1"), placed("t1", &[]));
    assert_eq!(place_state(one, "other rules", &all[..4], "u1"), placed("u1", &[]));
    assert_eq!(place_state(one, "rules", &all[..4], "s2"), placed("s1", &[(4, "s2")]));
}

#[test]
fn a_conversation_that_changed_too_often_starts_again() {
    let session = SessionId::new();
    let all = items(CHANGES + 2, "e");
    let _ = place_state(session, "rules", &all[..1], "s0");
    for at in 1..=CHANGES {
        let _ = place_state(session, "rules", &all[..at], &format!("s{at}"));
    }
    let next = format!("s{}", CHANGES + 1);
    assert_eq!(place_state(session, "rules", &all[..=CHANGES], &next), placed(&next, &[]));
}
