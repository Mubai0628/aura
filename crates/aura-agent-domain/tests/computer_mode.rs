use aura_agent_domain::{AgentStartRequest, AgentSubmitRequest};
use aura_domain_common::{ModelProfileId, SessionId};
use serde_json::json;

#[test]
fn a_turn_and_a_queued_message_keep_their_own_desktop_choice() {
    let session = SessionId::new();
    let request = json!({"session_id":session,"model_profile_id":ModelProfileId::new(),"text":"fixture","mode":"normal","goal_budget":null,"computer_mode":"child_session"});
    let started = serde_json::from_value::<AgentStartRequest>(request)
        .expect("child session is a supported user choice");
    assert_eq!(
        serde_json::to_value(started.receipt()).expect("receipt")["computer_mode"],
        "child_session"
    );
    let queued = json!({"session_id":session,"text":"fixture","destination":{"lane":"next_turn"},"computer_mode":"child_session"});
    let submitted =
        serde_json::from_value::<AgentSubmitRequest>(queued).expect("queued desktop choice");
    assert_eq!(
        serde_json::to_value(submitted.receipt()).expect("receipt")["computer_mode"],
        "child_session"
    );
}

#[test]
fn old_requests_keep_the_foreground_default_and_unknown_modes_are_refused() {
    let old = json!({"session_id":SessionId::new(),"model_profile_id":ModelProfileId::new(),"text":"fixture","mode":"normal","goal_budget":null});
    let request = serde_json::from_value::<AgentStartRequest>(old.clone()).expect("old request");
    assert!(serde_json::to_value(request).expect("old serialized").get("computer_mode").is_none());
    let mut invalid = old;
    invalid["computer_mode"] = json!("fallback_to_foreground");
    assert!(serde_json::from_value::<AgentStartRequest>(invalid).is_err());
}

#[test]
fn special_turns_accept_the_same_desktop_choice() {
    let base = json!({"session_id":SessionId::new(),"model_profile_id":ModelProfileId::new(),"computer_mode":"child_session"});
    let init = serde_json::from_value::<aura_agent_domain::AgentInitStartRequest>(base.clone())
        .expect("init mode");
    let compact = serde_json::from_value::<aura_agent_domain::AgentCompactionStartRequest>(base)
        .expect("compact mode");
    assert_eq!(serde_json::to_value(init).expect("init")["computer_mode"], "child_session");
    assert_eq!(serde_json::to_value(compact).expect("compact")["computer_mode"], "child_session");
}
