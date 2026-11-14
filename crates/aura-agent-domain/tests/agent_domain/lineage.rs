use aura_agent_domain::*;
use aura_domain_common::*;
use serde_json::json;

#[test]
fn expired_is_closed_and_cannot_activate() {
    assert!(TaskLineageState::Expired.is_closed());
    assert!(!TaskLineageState::Expired.may_activate());
    assert!(!TaskLineageState::InterruptedResumable.may_activate());
    assert!(!TaskLineageState::SuspendedByTopicSwitch.may_activate());
    assert!(TaskLineageState::Active.may_activate());
    let state: TaskLineageState =
        serde_json::from_value(json!("expired")).expect("expired decodes");
    assert_eq!(state, TaskLineageState::Expired);
}

#[test]
fn resume_suspended_decodes_as_history_but_is_absent_from_tool_input() {
    let lineage_id = TaskLineageId::new();
    let decision: TaskLineageReconcileDecision = serde_json::from_value(json!({
        "decision": "resume_suspended",
        "lineage_id": lineage_id,
    }))
    .expect("historical resume_suspended still decodes");
    assert!(matches!(
        decision,
        TaskLineageReconcileDecision::ResumeSuspended { lineage_id: id } if id == lineage_id
    ));
    assert!(
        serde_json::from_value::<TaskLineageReconcileToolInput>(json!({
            "decision": { "decision": "resume_suspended", "lineage_id": lineage_id },
            "reason": "must not be a public tool option"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<TaskLineageReconcileToolDecision>(json!({
            "decision": "resume_suspended",
            "lineage_id": lineage_id
        }))
        .is_err()
    );
}
