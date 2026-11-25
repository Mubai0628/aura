//! A conversation waits for the user when any of its waits does; the one it shows comes first in
//! the order approval, question, secure input, decision about the context.
use aura_agent_domain::{
    AgentSessionActivity, AgentSessionActivityState, AgentSessionWaitingFor, AgentSessionsActivity,
};
use aura_domain_common::{SessionId, TurnTaskId};

fn running(session: SessionId) -> AgentSessionActivity {
    AgentSessionActivity {
        session_id: session,
        state: AgentSessionActivityState::Running,
        turn_task_ids: vec![TurnTaskId::new()],
        waiting_for: None,
    }
}

#[test]
fn a_secure_input_makes_a_running_conversation_wait_and_leaves_the_others() {
    let (asking, other) = (SessionId::new(), SessionId::new());
    let mut activity = AgentSessionsActivity {
        sessions: vec![running(asking), running(other)],
        ended: Vec::new(),
        peer_results: Vec::new(),
    };
    let turns = activity.sessions[0].turn_task_ids.clone();
    activity.waiting_also(&[asking], AgentSessionWaitingFor::SensitiveInput);
    assert_eq!(
        activity.sessions[0],
        AgentSessionActivity {
            session_id: asking,
            state: AgentSessionActivityState::Waiting,
            turn_task_ids: turns,
            waiting_for: Some(AgentSessionWaitingFor::SensitiveInput),
        }
    );
    assert_eq!(activity.sessions[1].state, AgentSessionActivityState::Running);
    assert_eq!(activity.sessions[1].waiting_for, None);
}

#[test]
fn an_approval_or_a_question_shows_before_a_secure_input_and_a_secure_input_before_the_context() {
    let session = SessionId::new();
    for (known, shown) in [
        (AgentSessionWaitingFor::Approval, AgentSessionWaitingFor::Approval),
        (AgentSessionWaitingFor::Question, AgentSessionWaitingFor::Question),
        (AgentSessionWaitingFor::Context, AgentSessionWaitingFor::SensitiveInput),
    ] {
        let mut activity = AgentSessionsActivity {
            sessions: vec![AgentSessionActivity {
                state: AgentSessionActivityState::Waiting,
                waiting_for: Some(known),
                ..running(session)
            }],
            ended: Vec::new(),
            peer_results: Vec::new(),
        };
        activity.waiting_also(&[session], AgentSessionWaitingFor::SensitiveInput);
        assert_eq!(activity.sessions[0].waiting_for, Some(shown), "{known:?}");
        assert_eq!(activity.sessions[0].state, AgentSessionActivityState::Waiting);
    }
    assert_eq!(
        AgentSessionWaitingFor::Approval.first(AgentSessionWaitingFor::Question),
        AgentSessionWaitingFor::Approval
    );
    assert_eq!(
        AgentSessionWaitingFor::Context.first(AgentSessionWaitingFor::Question),
        AgentSessionWaitingFor::Question
    );
}

#[test]
fn a_conversation_not_yet_listed_is_added_as_waiting() {
    let session = SessionId::new();
    let mut activity = AgentSessionsActivity::default();
    activity.waiting_also(&[session], AgentSessionWaitingFor::SensitiveInput);
    assert_eq!(
        activity.sessions,
        [AgentSessionActivity {
            session_id: session,
            state: AgentSessionActivityState::Waiting,
            turn_task_ids: Vec::new(),
            waiting_for: Some(AgentSessionWaitingFor::SensitiveInput),
        }]
    );
}

#[test]
fn the_waiting_kind_is_sensitive_input_on_the_wire() {
    assert_eq!(
        serde_json::to_value(AgentSessionWaitingFor::SensitiveInput).expect("serializes"),
        serde_json::json!("sensitive_input")
    );
}
