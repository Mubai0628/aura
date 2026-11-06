//! Aura keeps the desktop for a turn: from its first action to its end, between its commands too,
//! and gives it back when Core ends the hold, the user takes over, or [`HOLD_IDLE_MS`] pass
//! without a command of it or Core keeping it. A command of no hold keeps it while it runs.
use aura_desktop_control::hold::{Begin, Ended, Holds, TAKEN_REMEMBERED};
use aura_desktop_domain::{HOLD_IDLE_MS, HoldEnd};

fn ended(key: &str, reason: HoldEnd) -> Option<Ended> {
    Some(Ended { key: Some(key.into()), reason })
}

#[test]
fn a_turns_hold_lasts_between_its_commands_until_core_ends_it() {
    let mut holds = Holds::new();
    assert_eq!(holds.begin(Some("t1"), false, 0), (Begin::Started, None));
    assert!(!holds.finish(100), "kept between commands");
    assert_eq!(holds.tick(false, 5_000), None);
    assert_eq!(holds.begin(Some("t1"), false, 5_000), (Begin::Continued, None));
    assert!(!holds.finish(5_100));
    assert!(holds.release("t1"));
    assert!(!holds.release("t1"), "nothing is kept any more");
    assert_eq!(holds.begin(Some("t2"), false, 6_000), (Begin::Started, None));
}

#[test]
fn a_hold_is_not_given_back_under_its_running_command() {
    let mut holds = Holds::new();
    holds.begin(Some("t1"), false, 0);
    assert!(!holds.release("t1"), "the running command keeps its watch on the user's input");
    assert!(!holds.finish(10));
    assert!(holds.release("t1"));
}

#[test]
fn a_command_of_no_hold_keeps_the_desktop_only_while_it_runs() {
    let mut holds = Holds::new();
    assert_eq!(holds.begin(None, false, 0), (Begin::Started, None));
    assert!(holds.finish(10), "given back with the command");
    assert_eq!(holds.begin(Some("t1"), false, 20), (Begin::Started, None));
}

#[test]
fn another_hold_waits_while_one_is_kept() {
    let mut holds = Holds::new();
    holds.begin(Some("t1"), false, 0);
    holds.finish(10);
    assert_eq!(holds.begin(Some("t2"), false, 20), (Begin::Busy, None));
    assert_eq!(holds.begin(None, false, 20), (Begin::Busy, None));
    assert_eq!(holds.begin(Some("t1"), false, 30), (Begin::Continued, None));
}

#[test]
fn core_keeps_a_turns_hold_while_the_model_thinks_between_its_commands() {
    let mut holds = Holds::new();
    holds.begin(Some("t1"), false, 0);
    holds.finish(1_000);
    assert!(holds.keep("t1", HOLD_IDLE_MS), "kept for another while");
    assert_eq!(holds.tick(false, 1_000 + HOLD_IDLE_MS), None, "past its first idle end");
    assert_eq!(holds.tick(false, 2 * HOLD_IDLE_MS - 1), None);
    assert_eq!(holds.tick(false, 2 * HOLD_IDLE_MS), ended("t1", HoldEnd::Idle));
    assert!(!holds.keep("t1", 2 * HOLD_IDLE_MS + 1), "a hold that ended is not kept again");
}

#[test]
fn only_the_hold_kept_now_is_kept() {
    let mut holds = Holds::new();
    assert!(!holds.keep("t1", 0), "nothing is kept");
    holds.begin(Some("t1"), false, 0);
    assert!(holds.keep("t1", 10), "its command runs");
    assert!(!holds.keep("t2", 10), "another turn's");
    holds.finish(20);
    assert!(!holds.keep("t2", 30));
    assert!(holds.release("t1"));
    holds.begin(None, false, 40);
    assert!(!holds.keep("t1", 50), "a command of no hold");
}

#[test]
fn a_hold_is_given_back_once_no_command_came_for_its_idle_time() {
    let mut holds = Holds::new();
    holds.begin(Some("t1"), false, 0);
    holds.finish(1_000);
    holds.begin(Some("t1"), false, 50_000);
    holds.finish(52_000);
    assert_eq!(holds.tick(false, 51_999 + HOLD_IDLE_MS), None);
    assert_eq!(holds.tick(false, 52_000 + HOLD_IDLE_MS), ended("t1", HoldEnd::Idle));
    assert_eq!(holds.begin(Some("t2"), false, 52_001 + HOLD_IDLE_MS), (Begin::Started, None));
}

#[test]
fn a_running_command_is_never_ended_by_the_ticks() {
    let mut holds = Holds::new();
    holds.begin(Some("t1"), false, 0);
    assert_eq!(holds.tick(false, 10 * HOLD_IDLE_MS), None);
    assert_eq!(holds.tick(true, 10 * HOLD_IDLE_MS), None, "the command itself stops");
    assert!(!holds.finish(10 * HOLD_IDLE_MS));
    assert_eq!(holds.tick(true, 10 * HOLD_IDLE_MS + 1), ended("t1", HoldEnd::TakenOver));
}

#[test]
fn a_taken_over_hold_ends_at_the_next_tick_and_its_turn_cannot_take_the_desktop_again() {
    let mut holds = Holds::new();
    holds.begin(Some("t1"), false, 0);
    holds.finish(10);
    assert_eq!(holds.tick(true, 20), ended("t1", HoldEnd::TakenOver));
    assert_eq!(holds.tick(true, 60), None, "ended once");
    assert_eq!(holds.begin(Some("t1"), false, 30), (Begin::TakenOver, None));
    assert_eq!(holds.begin(Some("t2"), false, 40), (Begin::Started, None));
}

#[test]
fn a_command_that_comes_before_the_tick_that_saw_a_takeover_is_refused_and_ends_the_hold() {
    let mut holds = Holds::new();
    holds.begin(Some("t1"), false, 0);
    holds.finish(10);
    assert_eq!(
        holds.begin(Some("t1"), true, 20),
        (Begin::TakenOver, ended("t1", HoldEnd::TakenOver))
    );
    assert_eq!(holds.tick(false, 30), None);
    assert_eq!(holds.begin(Some("t2"), false, 40), (Begin::Started, None));
}

#[test]
fn a_hold_another_turn_finds_taken_over_or_out_of_time_gives_way_before_the_tick() {
    let mut holds = Holds::new();
    holds.begin(Some("t1"), false, 0);
    holds.finish(0);
    assert_eq!(
        holds.begin(Some("t2"), false, HOLD_IDLE_MS),
        (Begin::Started, ended("t1", HoldEnd::Idle))
    );
    holds.finish(HOLD_IDLE_MS);
    assert_eq!(
        holds.begin(None, true, HOLD_IDLE_MS + 1),
        (Begin::Started, ended("t2", HoldEnd::TakenOver))
    );
    assert_eq!(holds.begin(Some("t3"), false, HOLD_IDLE_MS + 2), (Begin::Busy, None));
    assert!(holds.finish(HOLD_IDLE_MS + 3));
    assert_eq!(holds.begin(Some("t2"), false, HOLD_IDLE_MS + 4), (Begin::TakenOver, None));
}

#[test]
fn a_command_of_no_hold_that_the_user_took_over_ends_with_nothing_to_say() {
    let mut holds = Holds::new();
    holds.begin(None, false, 0);
    assert!(holds.finish(10));
    assert_eq!(holds.tick(true, 20), None);
}

#[test]
fn the_turns_whose_hold_was_taken_over_are_remembered_up_to_a_limit() {
    let mut holds = Holds::new();
    for n in 0..=TAKEN_REMEMBERED {
        let key = format!("t{n}");
        holds.begin(Some(&key), false, n as u64);
        holds.finish(n as u64);
        assert_eq!(holds.tick(true, n as u64), ended(&key, HoldEnd::TakenOver));
    }
    assert_eq!(holds.begin(Some("t1"), false, 1_000), (Begin::TakenOver, None));
    assert_eq!(holds.begin(Some("t0"), false, 1_000), (Begin::Started, None), "forgotten");
}
