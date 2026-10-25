use super::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The built-in tools the test sessions expose.
pub(super) const NAMES: &[&str] = &[
    "skills.search",
    "skills.describe",
    "skills.load",
    "skills.unload",
    "skills.list_loaded",
    "skills.read_resource",
    "fs.write",
    "normal.complete",
];

/// The user's `.agents\skills` in the tests' TEMP stand-in for the profile.
pub(super) fn agents(f: &Fixture) -> PathBuf {
    f.settings_facade().paths().local_root.join("test-profile").join(".agents").join("skills")
}

pub(super) fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, text).expect("file");
}

/// A Skill folder `name` under `root`, with instructions and one reference file.
pub(super) fn skill(root: &Path, name: &str, body: &str) -> PathBuf {
    let directory = root.join(name);
    write(
        &directory.join("SKILL.md"),
        &format!(
            "---\nname: {name}\ndescription: Skill {name} for tests\nallowed-tools: fs.write\n---\n\n{body}\n"
        ),
    );
    write(&directory.join("references/guide.md"), &format!("Guide of {name}.\n"));
    directory
}

pub(super) async fn route(f: &Fixture, method: &str, params: Value) -> Value {
    crate::r03_tests::success(&f.runtime, method, params).await
}

/// Trusts exactly the listed content (switching the Skill on) and returns its id.
pub(super) async fn trust(f: &Fixture, workspace: Option<WorkspaceId>, relative: &str) -> String {
    let list = route(f, "agent.skills.list", json!({"workspace_id": workspace})).await;
    let view = list["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .find(|skill| skill["relative"] == relative)
        .cloned()
        .unwrap_or_else(|| panic!("{relative} listed: {list}"));
    route(
        f,
        "agent.skills.trust",
        json!({"id": view["id"], "workspace_id": workspace, "expected_fingerprint": view["fingerprint"], "expected_revision": list["revision"]}),
    )
    .await;
    view["id"].as_str().expect("id").to_owned()
}

/// Runs one call to its result and moves the turn on.
pub(super) async fn call(
    f: &mut Fixture,
    name: &str,
    arguments: Value,
) -> (ToolCallId, ToolResultV1) {
    let batch = f.admit(&[(name, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let stored = {
        let store = f.service().broker.lock().expect("journal");
        aura_persistence_v2::tool::execution::call(store.database.connection(), id)
            .expect("call exists")
            .result
            .expect("terminal result")
    };
    f.continue_after_tools(batch_id, &group);
    (id, stored)
}

/// The model-visible error of a refused call.
pub(super) fn refusal(result: &ToolResultV1) -> String {
    match &result.outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error }
        | ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } => error.message.to_string(),
        other => panic!("the call was not refused: {other:?}"),
    }
}

/// What the next step's task state would hold for this turn's loaded Skills.
pub(super) fn context(f: &Fixture) -> Vec<Result<String, &'static str>> {
    let runtime = f.service().agent_execution_required().skills.get().expect("skills").clone();
    let loaded = {
        let store = f.service().broker.lock().expect("journal");
        aura_persistence_v2::agent::skills::loaded(store.database.connection(), f.turn.agent_run_id)
            .expect("loaded set")
    };
    crate::agent_execution::skill_tools::for_context(&runtime, f.turn.workspace_id, &loaded)
        .into_iter()
        .map(|item| item.body)
        .collect()
}
