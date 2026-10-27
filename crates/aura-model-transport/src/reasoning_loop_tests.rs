use super::*;
use aura_domain_common::ModelContentBlockId;
use aura_model_domain::ProviderToolCallId;

const LOOP: &str = "I should read the policy file once more before I decide anything.";

fn reasoning(text: &str) -> ModelStreamEvent {
    ModelStreamEvent::ReasoningSummaryDelta {
        block_id: ModelContentBlockId::new(),
        text: text.into(),
    }
}

#[test]
fn one_paragraph_coming_back_five_times_is_a_loop() {
    let mut guard = ReasoningLoopGuard::default();
    for _ in 1..REASONING_LOOP_REPEATS {
        assert!(!guard.observe(&[reasoning(&format!("{LOOP}\n\n"))]));
    }
    assert!(guard.observe(&[reasoning(&format!("{LOOP}\n\n"))]));
}

#[test]
fn a_paragraph_split_across_deltas_and_spaced_differently_is_the_same() {
    let mut guard = ReasoningLoopGuard::default();
    let mut looped = false;
    for _ in 0..REASONING_LOOP_REPEATS {
        let (head, tail) = LOOP.split_at(20);
        looped = guard
            .observe(&[reasoning(head), reasoning(&format!("{}  \n\n", tail.replace(' ', "  ")))]);
    }
    assert!(looped);
}

#[test]
fn varied_reasoning_and_short_repeats_are_not_a_loop() {
    let mut guard = ReasoningLoopGuard::default();
    for index in 0..200 {
        assert!(!guard.observe(&[reasoning(&format!(
            "Step {index}: reading the next file and noting what it says.\n\n"
        ))]));
        assert!(!guard.observe(&[reasoning("Checking the file.\n\n")]), "a short paragraph");
    }
}

#[test]
fn once_the_answer_starts_reasoning_is_no_longer_counted() {
    let mut guard = ReasoningLoopGuard::default();
    assert!(!guard.observe(&[reasoning(&format!("{LOOP}\n\n"))]));
    let call = ModelStreamEvent::ToolCallStarted {
        provider_call_id: ProviderToolCallId::new("call-1").expect("call id"),
        tool_name: "fs.read".into(),
        position: 0,
    };
    assert!(!guard.observe(&[call]));
    for _ in 0..REASONING_LOOP_REPEATS * 2 {
        assert!(!guard.observe(&[reasoning(&format!("{LOOP}\n\n"))]));
    }
}

#[test]
fn paragraphs_ending_in_crlf_blank_lines_are_counted_even_split_across_deltas() {
    let mut guard = ReasoningLoopGuard::default();
    for _ in 1..REASONING_LOOP_REPEATS {
        assert!(!guard.observe(&[reasoning(&format!("{LOOP}\r\n\r\n"))]));
    }
    assert!(guard.observe(&[reasoning(&format!("{LOOP}\r")), reasoning("\n\r\n")]));
}

#[test]
fn text_without_blank_lines_keeps_a_bounded_buffer() {
    let mut guard = ReasoningLoopGuard::default();
    let chunk = "x".repeat(63) + "\n";
    for _ in 0..(2 * 1024 * 1024 / chunk.len()) {
        assert!(!guard.observe(&[reasoning(&chunk)]));
        assert!(
            guard.partial.len() <= MAX_PARAGRAPH_BYTES + chunk.len(),
            "{}",
            guard.partial.len()
        );
    }
}

#[test]
fn after_a_paragraph_too_long_to_track_the_next_ones_are_counted_again() {
    let mut guard = ReasoningLoopGuard::default();
    assert!(!guard.observe(&[reasoning(&"y".repeat(MAX_PARAGRAPH_BYTES * 2))]));
    assert!(!guard.observe(&[reasoning(&format!("{LOOP}\n\n"))]), "the long paragraph's end");
    let mut looped = false;
    for _ in 0..REASONING_LOOP_REPEATS {
        looped = guard.observe(&[reasoning(&format!("{LOOP}\n\n"))]);
    }
    assert!(looped);
}

/// Whether `text` loops, fed in deltas of `size` bytes (cut at character boundaries).
fn loops_in_deltas(text: &str, size: usize) -> bool {
    let mut guard = ReasoningLoopGuard::default();
    let mut rest = text;
    while !rest.is_empty() {
        let mut cut = size.min(rest.len());
        while !rest.is_char_boundary(cut) {
            cut += 1;
        }
        let (delta, tail) = rest.split_at(cut);
        if guard.observe(&[reasoning(delta)]) {
            return true;
        }
        rest = tail;
    }
    false
}

#[test]
fn the_same_text_gives_the_same_answer_however_it_is_split() {
    let paragraph = |bytes: usize| {
        let mut text = "思考".repeat(bytes / 6);
        text.push_str(&"x".repeat(bytes - text.len()));
        text
    };
    let cases = [
        // GPT's case: a 36,800-byte paragraph repeated is longer than a tracked one.
        (format!("{}\n\n", paragraph(36_800)).repeat(REASONING_LOOP_REPEATS as usize), false),
        (format!("{}\n\n", paragraph(MAX_PARAGRAPH_BYTES + 1)).repeat(5), false),
        (format!("{}\n\n", paragraph(MAX_PARAGRAPH_BYTES)).repeat(5), true),
        (format!("{}\r\n\r\n", paragraph(MAX_PARAGRAPH_BYTES)).repeat(5), true),
        (format!("{LOOP}\r\n\r\n").repeat(5), true),
        (format!("{}\n\n{LOOP}\n\n", paragraph(40_000)).repeat(5), true),
    ];
    for (index, (text, expected)) in cases.iter().enumerate() {
        for size in [1, 2, 7, 64, 1_024, 16_384, 16_385, text.len()] {
            assert_eq!(
                loops_in_deltas(text, size),
                *expected,
                "case {index} in {size}-byte deltas"
            );
        }
    }
}
