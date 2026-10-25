use super::*;

/// An external write that really changed the file must not be reported as no effect.
///
/// The completion review found this by reading the code: an external target never
/// enters `paths(call)`, the workspace inspection before and after therefore returns
/// the same facts, and an external write does not enter the workspace mutation journal
/// either — so the success path lands on "journal empty and before == after", which the
/// shared evidence rule reads as `NoEffect`. The payload says `applied` and the bytes
/// really are on disk, so the model is told the opposite of what happened.
///
/// Ignored because the fix is not in yet: an external mutation needs its own persistent
/// change fact (operation and attempt identity, the authorization it used, the real
/// target identity, the expected and resulting content, and the publish/rollback
/// stage), and the attempt's side-effect state has to come from that fact instead of
/// from a workspace journal the external file never enters. Asserting the correct
/// behaviour here keeps the gap recorded rather than silently accepted, and this test
/// turns green the moment the fact exists — it is a defect marker, not an acceptance.
#[tokio::test]
async fn a_successful_external_write_is_not_reported_as_no_effect() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalWrite).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-effect-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-effect");
    std::fs::create_dir_all(&external).expect("external root");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External effect"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let record = external_directory_grant(
        session,
        workspace.workspace_id,
        &external,
        vec![
            aura_permission_domain::ExternalGrantCapability::Write,
            aura_permission_domain::ExternalGrantCapability::Create,
        ],
    );
    services
        .agent_required()
        .execution
        .persistence
        .create_external_grant(record, CorrelationId::new())
        .expect("directory grant");
    let target = external.join("written.txt");

    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Create the file AURA_EXTERNAL_PATH={} with the body you were given.",
                target.display()
            ),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("start");
    let done = wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "{:?}", done.turn.state);
    assert_eq!(
        std::fs::read_to_string(&target).expect("the external file must exist"),
        provider_fixture::EXTERNAL_WRITE_TOKEN,
        "the write really happened, so the effect fact must not say otherwise"
    );
    let bodies = provider.requests.lock().expect("requests").clone();
    let tool_results = bodies
        .iter()
        .flat_map(|body| {
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|message| message["role"] == "tool")
                .filter_map(|message| message["content"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // The run also contains a discovery call, whose result is legitimately `no_effect`
    // because it changes nothing. The envelope under test is the one carrying the
    // external write's own facts, so the assertion is bound to that message instead of
    // to the whole conversation text.
    let write_envelope = tool_results
        .iter()
        .find(|content| content.contains("\"created\":true"))
        .expect("the external create result must reach the model");
    assert!(
        write_envelope.contains("\"side_effect_state\":\"fully_applied\""),
        "a write that really happened must be reported as applied: {write_envelope}"
    );
}

/// A refused external write is reported as no effect, and the target is untouched.
///
/// The write asks for an absent target while the file already exists, so it is refused
/// before anything could be published. The honest answer is that nothing happened — the
/// stage fact says `not_performed`, and the model's envelope is read from that fact instead
/// of from a missing reply. "Unknown" here would make the model re-derive state it already
/// has, and "applied" would be false.
#[tokio::test]
async fn a_refused_external_write_is_reported_as_no_effect() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalWrite).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-refused-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-refused");
    std::fs::create_dir_all(&external).expect("external root");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External refused"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let record = external_directory_grant(
        session,
        workspace.workspace_id,
        &external,
        vec![
            aura_permission_domain::ExternalGrantCapability::Write,
            aura_permission_domain::ExternalGrantCapability::Create,
        ],
    );
    services
        .agent_required()
        .execution
        .persistence
        .create_external_grant(record, CorrelationId::new())
        .expect("directory grant");
    // The target the scenario creates already exists, so its `expected: absent` write is a
    // conflict rather than a publication.
    let target = external.join("written.txt");
    std::fs::write(&target, "already here").expect("existing target");

    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Create the file AURA_EXTERNAL_PATH={} with the body you were given.",
                target.display()
            ),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("start");
    let _ = wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    assert_eq!(
        std::fs::read_to_string(&target).expect("the target must still exist"),
        "already here",
        "a refused write must not change the file"
    );
    let writes = provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .flat_map(|body| {
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|message| message["role"] == "tool")
                .filter_map(|message| message["content"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .filter(|content| {
            content.contains("expected_state") || content.contains("side_effect_state")
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        writes.contains("\"side_effect_state\":\"no_effect\""),
        "a refusal before publication is no effect: {writes}"
    );
    assert!(
        !writes.contains("\"side_effect_state\":\"fully_applied\""),
        "a refused write was never applied: {writes}"
    );
}

/// The state one external mutation reports is read from the stage it recorded.
///
/// The table is asserted directly because it is the whole reading: a published change is
/// applied, a rolled-back publication is compensated, a refusal before any publication is
/// no effect, and a prepared or unconfirmable change — including a call whose fact is
/// missing even though it ran — is Unknown. Unknown is what stops a replay, so it is never
/// replaced by the cheaper "no effect".
#[test]
fn an_external_mutation_reports_the_state_its_stage_recorded() {
    use aura_execution_contract::SideEffectStateV1 as State;
    use aura_persistence_v2::tool::external_change::ExternalChangeStage as Stage;
    assert_eq!(
        crate::agent_execution::files::external_side_effect(Some(Stage::Published)),
        State::FullyApplied
    );
    assert_eq!(
        crate::agent_execution::files::external_side_effect(Some(Stage::Compensated)),
        State::Compensated
    );
    assert_eq!(
        crate::agent_execution::files::external_side_effect(Some(Stage::NotPerformed)),
        State::NoEffect
    );
    assert_eq!(
        crate::agent_execution::files::external_side_effect(Some(Stage::Prepared)),
        State::Unknown
    );
    assert_eq!(
        crate::agent_execution::files::external_side_effect(Some(Stage::Unknown)),
        State::Unknown
    );
    assert_eq!(crate::agent_execution::files::external_side_effect(None), State::Unknown);
}

#[tokio::test]
async fn an_ungranted_external_write_needs_the_external_capability() {
    let full = external_write_outcome(PermissionMode::FullAccess).await;
    assert!(full.finished, "FullAccess completes an ordinary external write");
    assert!(!full.asked, "FullAccess does not ask for an ordinary external file");
    assert!(full.written, "the external file must really be written");

    let lower = external_write_outcome(PermissionMode::WorkspaceReadWrite).await;
    assert!(
        lower.asked,
        "a lower mode must ask before writing an external file, and the approval request \
         has to be a real fact on the run"
    );
    assert!(
        !lower.written,
        "the file must not exist while the write waits for the user's approval"
    );
}
