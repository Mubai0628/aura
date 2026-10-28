use super::turns::{Head, group};
use super::*;

fn head(carried: bool, completed: bool, sequence: u64) -> Head {
    Head {
        completed,
        position: (u8::from(!carried), sequence),
        sent_at: "2026-10-27T13:07:00Z".parse().expect("time"),
        text: format!("m{sequence}"),
    }
}

type Span = (u32, super::rows::Position, Option<super::rows::Position>);

fn spans(rounds: &[Round]) -> Vec<Span> {
    rounds.iter().map(|round| (round.number, round.start, round.end)).collect()
}

#[test]
fn a_round_runs_from_a_request_until_a_turn_of_it_completes() {
    // Completed; stopped, then continued to completion; failed, then the current turn.
    let heads = [
        head(false, true, 1),
        head(false, false, 5),
        head(false, true, 9),
        head(false, false, 12),
        head(false, false, 20),
    ];
    let rounds = group(&heads);
    assert_eq!(
        spans(&rounds),
        [(1, (1, 1), Some((1, 5))), (2, (1, 5), Some((1, 12))), (3, (1, 12), None)]
    );
    assert_eq!(rounds[1].first_message, "m5", "the round is known by its first message");
    assert!(group(&[]).is_empty());
}

#[test]
fn a_branch_carried_rounds_come_first_and_one_may_go_on_in_its_own_history() {
    let heads =
        [head(true, true, 3), head(true, false, 8), head(false, true, 2), head(false, false, 6)];
    assert_eq!(
        spans(&group(&heads)),
        [(1, (0, 3), Some((0, 8))), (2, (0, 8), Some((1, 6))), (3, (1, 6), None)]
    );
}

#[test]
fn a_line_shows_the_first_two_hundred_characters_and_the_whole_length() {
    let mut round = group(&[head(false, false, 1)]).remove(0);
    round.first_message = "字".repeat(250);
    let line = line(&round);
    assert_eq!(line.id, "T1");
    assert_eq!(line.first_message.chars().count(), HISTORY_FIRST_MESSAGE_CHARS + 1);
    assert!(line.first_message.ends_with('…'));
    assert_eq!(line.first_message_chars, 250);
    round.first_message = "short".into();
    assert_eq!(super::line(&round).first_message, "short");
}
