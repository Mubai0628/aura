//! `/init`'s write through the production admission, policy, evidence and file path;
//! only the model's calls are scripted, and every file lives in the fixture workspace.
use super::*;
use aura_agent_domain::{AgentStartRequest, AgentTaskMode, AgentTaskPurpose};
mod refusals;
mod writes;

const TOOLS: &[&str] = &["workspace_init.complete", "shell.powershell", "shell.cmd"];
const COMMAND: &str = "cargo test --workspace";

/// An init task in a workspace whose README defines the test command.
async fn init_fixture(mode: PermissionMode) -> Fixture {
    let f = Fixture::with_exposure_setup(mode, TOOLS, |root| {
        std::fs::write(root.join("README.md"), format!("# Project\n\nRun `{COMMAND}` first.\n"))
            .expect("the repository defines its command");
        std::fs::create_dir(root.join("crates")).expect("a module directory");
    })
    .await;
    mark_init(&f);
    f
}

/// Records the run's configuration the way `agent.init.start` does.
fn mark_init(f: &Fixture) {
    use aura_persistence_v2::agent::application::{RunConfiguration, configure};
    f.service()
        .broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| {
            configure(
                tx,
                f.turn.agent_run_id,
                &RunConfiguration {
                    request: AgentStartRequest {
                        session_id: f.turn.session_id,
                        model_profile_id: ModelProfileId::new(),
                        text: "/init".into(),
                        mode: AgentTaskMode::Normal,
                        goal_budget: None,
                        explicit_backend_id: None,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                    tool_settings: f.settings.clone(),
                    purpose: AgentTaskPurpose::WorkspaceInit,
                },
                CommandInvocationId::new(),
                UtcTimestamp::now(),
            )
        })
        .expect("an init task");
}

fn submission() -> serde_json::Value {
    json!({
        "language": "english",
        "expected": {"expected_kind": "absent"},
        "project_structure": "Rust code lives in [crates](crates).",
        "commands": {"entries": [{
            "command": COMMAND, "purpose": "Run every Rust test", "shell": "powershell",
            "evidence": {"kind": "repository_defined", "path": "README.md"}
        }]},
        "coding_style": "Format Rust with rustfmt.",
        "testing": "Write a failing test before the fix.",
        "commits_and_pull_requests": "One complete change per commit."
    })
}

/// The message the model received for a call that did not run.
fn refusal(f: &Fixture, call: ToolCallId) -> String {
    let store = f.service().broker.lock().expect("journal");
    let record = aura_persistence_v2::tool::execution::call(store.database.connection(), call)
        .expect("the call is recorded");
    match record.result.expect("a terminal result").outcome {
        ToolResultOutcomeV1::NotExecuted { error: Some(error), .. }
        | ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error } => error.message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Runs one init call to its end, continues the model step, and returns the call's id.
async fn run(f: &mut Fixture, input: serde_json::Value) -> ToolCallId {
    let batch = f.admit(&[("workspace_init.complete", input)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    id
}

/// The logical hash fs.read reports for a workspace file.
fn logical_hash(f: &Fixture, path: &str) -> aura_domain_common::ContentHash {
    f.service()
        .agent_execution_required()
        .fs
        .read_text(aura_workspace_domain::FsReadTextRequest {
            workspace_id: f.turn.workspace_id,
            session_id: f.turn.session_id,
            target: aura_workspace_domain::WorkspaceFileTarget {
                path: WorkspacePath::parse(path).expect("path"),
            },
            range: aura_workspace_domain::TextReadRange::Full,
            explicit_encoding: None,
        })
        .expect("a readable file")
        .logical_hash
}
