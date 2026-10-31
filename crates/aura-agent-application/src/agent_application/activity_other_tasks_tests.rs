//! Another session's task reaches the model by what it may touch; its session reaches the Inspector.
use super::activity::FrozenActivity;
use aura_activity_domain::*;
use aura_activity_runtime::{ActivityRelevance, ActivitySummary, ActivitySummaryRow};
use aura_domain_common::{
    ModelStepId, Revision, RuntimeInstanceId, SessionId, ToolCallId, ToolId, UtcTimestamp,
    WorkspaceId,
};
use aura_security_contract::NetworkDataClass;

fn row(
    byte: u8,
    session: SessionId,
    workspace: WorkspaceId,
    tool: &str,
    tool_call_id: ToolCallId,
    resource: ActivityResource,
) -> ActivityRow {
    ActivityRow {
        id: ActivityId::from_bytes([byte; 16]),
        kind: ActivityKind::AgentTool,
        tool_id: ToolId::from_stable_name(tool),
        tool_name: tool.to_owned(),
        private_source: ActivityPrivateSource::PersonalData,
        tool_group: ActivityToolGroup::Shell,
        session_id: session,
        workspace_id: workspace,
        agent_run_id: None,
        execution: ActivityExecutionRef::AgentToolCall {
            tool_call_id,
            operation_id: aura_domain_common::SecurityOperationId::new(),
            attempt_ordinal: 1,
        },
        resources: vec![resource],
        parameters: Vec::new(),
        started_at: UtcTimestamp::now(),
    }
}

#[test]
fn another_sessions_task_is_named_by_what_it_may_touch_and_its_session_kept_for_the_inspector() {
    let (own_call, other_call) = (ToolCallId::new(), ToolCallId::new());
    let (other_session, other_workspace) = (SessionId::new(), WorkspaceId::new());
    let own = row(
        1,
        SessionId::new(),
        WorkspaceId::new(),
        "fs.write",
        own_call,
        ActivityResource {
            target: ActivityResourceTarget::Unproven { hint: String::new() },
            relation: ActivityResourceRelation::Write,
            display_name: "src/own.rs".to_owned(),
            display_name_truncated: false,
        },
    );
    let other = row(
        2,
        other_session,
        other_workspace,
        "shell.powershell",
        other_call,
        ActivityResource {
            target: ActivityResourceTarget::Unproven { hint: String::new() },
            relation: ActivityResourceRelation::UnknownWriteSet,
            display_name: "crates/tools".to_owned(),
            display_name_truncated: false,
        },
    );
    let other_id = serde_json::to_value(other.id).expect("id");
    let summary = ActivitySummary {
        header: ActivitySnapshotHeader {
            sampled_at: UtcTimestamp::now(),
            runtime_instance_id: RuntimeInstanceId::new(),
            revision: Revision::new(4),
            coverage: ActivityCoverage::complete(2, 0),
            stale: false,
        },
        rows: [(own, ActivityRelevance::Direct), (other, ActivityRelevance::Related)]
            .into_iter()
            .map(|(row, relevance)| ActivitySummaryRow { row, relevance, rendered: String::new() })
            .collect(),
        body: String::new(),
    };
    let frozen =
        FrozenActivity { summary, model_step_id: ModelStepId::new(), revision: Revision::new(4) };
    let fragments = frozen.fragments().expect("render");
    let public = &fragments
        .iter()
        .find(|fragment| fragment.data_class == NetworkDataClass::PublicContent)
        .expect("public")
        .content;
    assert!(public.contains(&own_call.to_string()), "its own call is named: {public}");
    assert!(public.contains("\"another_task\":1"), "{public}");
    for absent in [
        other_session.to_string(),
        other_workspace.to_string(),
        other_call.to_string(),
        other_id.as_str().expect("hex").to_owned(),
        "shell.powershell".to_owned(),
        "agent.activity.query".to_owned(),
    ] {
        assert!(!public.contains(&absent), "{absent} must not reach the model: {public}");
    }
    let private = &fragments
        .iter()
        .find(|fragment| fragment.data_class == NetworkDataClass::PersonalData)
        .expect("classified names")
        .content;
    assert!(
        private.contains("\"another_task\":1") && private.contains("crates/tools"),
        "{private}"
    );
    assert!(!private.contains(&other_session.to_string()), "{private}");
    assert_eq!(
        frozen.sources(),
        vec![aura_context_domain::PromptInspectorActivitySourceView {
            another_task: 1,
            session_id: other_session,
            workspace_id: other_workspace,
        }],
        "the Inspector learns whose task it is"
    );
}
