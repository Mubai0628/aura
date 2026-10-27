//! The model's activity query lists its own session only; a Shell's relation follows its script.
use super::tests_support::synthetic_record;
use aura_activity_domain::{
    ActivityCoverageState, ActivityOmissionReason, ActivityQuery, ActivityResource,
    ActivityResourceRelation, ActivityResourceTarget,
};
use aura_activity_runtime::{ActivityIndex, ActivityVisibility};
use aura_domain_common::{SessionId, WorkspaceId};
use aura_security_contract::OperationRiskFlag;

fn unknown_writer(
    runtime: aura_domain_common::RuntimeInstanceId,
    session: SessionId,
    workspace: WorkspaceId,
) -> aura_activity_domain::ActivityRecord {
    let mut record = synthetic_record();
    record.runtime_instance_id = runtime;
    record.owner.runtime_instance_id = runtime;
    record.owner.session_id = session;
    record.owner.workspace_id = workspace;
    record.resources = vec![ActivityResource {
        target: ActivityResourceTarget::Unproven { hint: "shell".to_owned() },
        relation: ActivityResourceRelation::UnknownWriteSet,
        display_name: ".".to_owned(),
        display_name_truncated: false,
    }];
    record
}

#[test]
fn the_models_query_lists_its_own_session_and_counts_the_rest_as_filtered() {
    let index = ActivityIndex::new(16);
    let runtime = aura_domain_common::RuntimeInstanceId::new();
    let (session, workspace) = (SessionId::new(), WorkspaceId::new());
    for owner in [session, SessionId::new()] {
        assert!(index.publish(unknown_writer(runtime, owner, workspace)).was_recorded());
    }
    let page = |own_only| {
        super::query::page(
            &index,
            ActivityVisibility::for_session(runtime, session, workspace),
            ActivityQuery::UnknownWriteSet,
            None,
            0,
            10,
            own_only,
        )
        .expect("query")
    };
    let user = page(false);
    assert_eq!(user.rows.len(), 2, "the person sees both, each with its session");
    let model = page(true);
    assert_eq!(model.rows.len(), 1);
    assert_eq!(model.rows[0].session_id, session, "only the model's own session");
    assert_eq!(model.header.coverage.visibility_filtered, 1, "the other one is counted");
    assert_eq!(model.header.coverage.state, ActivityCoverageState::Partial);
    assert!(
        model
            .header
            .coverage
            .omission_reasons
            .contains(&ActivityOmissionReason::VisibilityFiltered)
    );
}

#[test]
fn a_shell_writing_outside_its_workspace_is_published_as_such() {
    assert_eq!(super::shell_relation(&[]), ActivityResourceRelation::UnknownWriteSet);
    assert_eq!(
        super::shell_relation(&[
            OperationRiskFlag::PermanentFileDeletion,
            OperationRiskFlag::OutsideWorkspaceWrite
        ]),
        ActivityResourceRelation::UnknownWriteSetOutsideWorkspace
    );
}
