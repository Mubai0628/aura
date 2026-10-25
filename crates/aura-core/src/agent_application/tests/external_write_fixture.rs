use super::*;

/// The same create in a mode that must ask: the run waits for the approval, so this
/// returns the target path after a bounded wait for the caller to assert on the disk.
pub(super) async fn external_create_asking_for_approval() -> std::path::PathBuf {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalWrite).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-write-ask-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-ask-write");
    std::fs::create_dir_all(&external).expect("external root");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External write ask"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::WorkspaceReadWrite).await;
    let target = external.join("asked.txt");
    crate::r03_tests::success(
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
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    target
}

/// What one ungranted external write did under a mode.
pub(super) struct ExternalWriteOutcome {
    /// The run really asked for an approval: this is the approval request the user would
    /// see, not an inference from a sleep.
    pub(super) asked: bool,
    /// The target existed while the run was waiting, or after it finished.
    pub(super) written: bool,
    /// The turn reached a terminal state within the bounded wait.
    pub(super) finished: bool,
}

/// An ungranted external write is decided by the external capability it really needs.
///
/// FullAccess follows the existing policy for an ordinary external file, so the write
/// happens without asking. WorkspaceReadWrite has to ask: the file must not exist while
/// the run waits for the user, and the approval request has to be a real fact on the run.
/// If the operation kept the workspace write capability a tool definition declares, the
/// lower mode would write the file with no approval at all — which is what this test
/// would then see.
pub(super) async fn external_write_outcome(mode: PermissionMode) -> ExternalWriteOutcome {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalWrite).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-capability-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-capability");
    std::fs::create_dir_all(&external).expect("external root");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External capability"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, mode).await;
    let target = external.join("ungranted.txt");
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
    let request = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    let mut asked = false;
    let mut finished = false;
    let mut written_while_waiting = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let view: AgentRunProjection = serde_json::from_value(
            crate::r03_tests::success(&runtime, "agent.status", json!(&request)).await,
        )
        .expect("status");
        asked = asked
            || !view.pending_approval_ids.is_empty()
            || matches!(view.run.state, AgentRunState::AwaitingApproval { .. });
        if view.turn.state.terminal() {
            finished = true;
            break;
        }
        if asked {
            // The run really is waiting for the user, so the target must not be there.
            written_while_waiting = target.exists();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    if asked && !finished {
        // A mode that never asks keeps running; wait for it to finish so the written
        // fact is read after the run settled rather than while it was mid-flight.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(45), async {
            loop {
                let view: AgentRunProjection = serde_json::from_value(
                    crate::r03_tests::success(&runtime, "agent.status", json!(&request)).await,
                )
                .expect("status");
                if view.turn.state.terminal() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
        })
        .await;
        finished = true;
    }
    ExternalWriteOutcome { asked, written: target.exists() || written_while_waiting, finished }
}
