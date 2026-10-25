//! A running step's tool input is counted as it streams, nothing of it is
//! stored, a new attempt starts again and an ended stream leaves nothing behind.
use super::*;
use aura_domain_common::{ModelContentBlockId, UtcTimestamp};
use aura_model_domain::ProviderToolCallId;

fn event(
    step: ModelStepId,
    attempt: TransportAttemptId,
    event: ModelStreamEvent,
) -> ModelStreamEventEnvelope {
    ModelStreamEventEnvelope {
        model_step_id: step,
        transport_attempt_id: attempt,
        sequence: 1,
        received_at: UtcTimestamp::now(),
        event,
    }
}

fn call(id: &str) -> ProviderToolCallId {
    ProviderToolCallId::new(id).expect("call id")
}

#[test]
fn a_tool_input_is_counted_as_it_streams_and_never_stored() {
    let (step, attempt) = (ModelStepId::new(), TransportAttemptId::new());
    let started = ModelStreamEvent::ToolCallStarted {
        provider_call_id: call("c1"),
        tool_name: "fs_write".into(),
        position: 0,
    };
    assert!(observe(&event(step, attempt, started)));
    for delta in [r#"{"path":"notes/é.md","#, r#""content":"hello"}"#] {
        let growing = ModelStreamEvent::ToolCallArgumentsDelta {
            provider_call_id: call("c1"),
            delta: delta.into(),
        };
        assert!(observe(&event(step, attempt, growing)), "only counted, nothing to store");
    }
    let (seen_attempt, calls) = snapshot().remove(&step).expect("the running step");
    assert_eq!(seen_attempt, attempt);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].wire_name, "fs_write");
    assert_eq!(calls[0].received_bytes, 40);
    assert_eq!(calls[0].head, r#"{"path":"notes/é.md","content":"hello"}"#);
    assert!(!calls[0].complete);
    assert!(observe(&event(
        step,
        attempt,
        ModelStreamEvent::ToolCallCompleted { provider_call_id: call("c1") }
    )));
    assert!(snapshot()[&step].1[0].complete);
    // Visible text is still stored.
    let text = ModelStreamEvent::AssistantTextDelta {
        block_id: ModelContentBlockId::new(),
        text: "x".into(),
    };
    assert!(!observe(&event(step, attempt, text)));
    forget(step);
}

#[test]
fn only_the_start_of_a_long_input_is_kept_and_cut_between_characters() {
    let (step, attempt) = (ModelStepId::new(), TransportAttemptId::new());
    observe(&event(
        step,
        attempt,
        ModelStreamEvent::ToolCallStarted {
            provider_call_id: call("c"),
            tool_name: "fs_write".into(),
            position: 0,
        },
    ));
    let long = "é".repeat(800);
    observe(&event(
        step,
        attempt,
        ModelStreamEvent::ToolCallArgumentsDelta {
            provider_call_id: call("c"),
            delta: long.clone(),
        },
    ));
    let input = &snapshot()[&step].1[0];
    assert_eq!(input.received_bytes, long.len() as u64);
    assert!(input.head.len() <= HEAD_BYTES && input.head.len() >= HEAD_BYTES - 1);
    assert!(input.head.chars().all(|character| character == 'é'));
    forget(step);
}

#[test]
fn a_new_attempt_starts_again_and_an_ended_stream_leaves_nothing() {
    let step = ModelStepId::new();
    let (first, second) = (TransportAttemptId::new(), TransportAttemptId::new());
    let started = |id: &str| ModelStreamEvent::ToolCallStarted {
        provider_call_id: call(id),
        tool_name: "shell_powershell".into(),
        position: 0,
    };
    observe(&event(step, first, started("a")));
    observe(&event(step, second, started("b")));
    let (attempt, calls) = snapshot().remove(&step).expect("running");
    assert_eq!((attempt, calls.len()), (second, 1));
    drop(Forget(step));
    assert!(!snapshot().contains_key(&step));
}
