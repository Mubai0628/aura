//! Hooks through the real model loop. The Hooks are the repository's test program,
//! trusted from TEMP through the real settings routes; the model is the scripted provider.
mod events;
mod tools;
use super::*;
use provider_fixture::{HOOK_SOURCE, Scenario};
use waiting::{prepare_in_mode, start, wait};

pub(super) async fn hook(
    runtime: &crate::RuntimeState,
    root: &std::path::Path,
    name: &str,
    events: &[&str],
    arguments: &[&str],
) {
    let directory = root.join("hooks").join(name);
    std::fs::create_dir_all(directory.join("bin")).expect("package folder");
    let debug = std::env::current_exe()
        .expect("test image")
        .parent()
        .expect("deps")
        .parent()
        .expect("debug")
        .to_owned();
    std::fs::copy(debug.join("aura-hook-fixture.exe"), directory.join("bin/hook.exe"))
        .expect("build current aura-tool-fixtures --bins first");
    let quote =
        |items: &[&str]| items.iter().map(|i| format!("'{i}'")).collect::<Vec<_>>().join(", ");
    std::fs::write(
        directory.join("hook.toml"),
        format!(
            "schema_version = 1\nname = \"{name}\"\nversion = \"1.0.0\"\nevents = [{}]\n[command]\nexecutable = \"bin/hook.exe\"\narguments = [{}]\n{}",
            quote(events),
            quote(arguments),
            // Tool events only hear fs.read here; a match on other events is refused.
            if events.iter().any(|event| event.starts_with("tool.")) {
                "[match]\ntools = [\"fs.read\"]\n"
            } else {
                ""
            }
        ),
    )
    .expect("manifest");
    let path = directory.display().to_string();
    let preview =
        crate::r03_tests::success(runtime, "agent.hooks.preview", json!({"path": path})).await;
    let list = crate::r03_tests::success(runtime, "agent.hooks.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.hooks.trust",
        json!({"path": path, "expected_fingerprint": preview["fingerprint"], "enabled": true, "expected_revision": list["revision"]}),
    )
    .await;
}

pub(super) struct Prepared {
    pub host: crate::r03_tests::Fixture,
    pub provider: Provider,
    pub runtime: crate::RuntimeState,
    pub session: SessionId,
    pub model: ModelProfileId,
}

/// A WorkspaceReadWrite session on the HookRead model with these Hooks trusted. `source`
/// writes the file the model reads; without it the read fails.
pub(super) async fn prepared(hooks: &[(&str, &[&str], &[&str])], source: bool) -> Prepared {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::HookRead).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let workspace = host.root.path().join("waiting-workspace");
    if source {
        std::fs::write(workspace.join(HOOK_SOURCE), "original text").expect("source");
    }
    std::fs::write(workspace.join("other.txt"), "redirected text").expect("other");
    for (name, events, arguments) in hooks {
        hook(&runtime, host.root.path(), name, events, arguments).await;
    }
    Prepared { host, provider, runtime, session, model }
}

pub(super) struct Turn {
    pub _host: crate::r03_tests::Fixture,
    pub provider: Provider,
    pub runtime: crate::RuntimeState,
    pub request: AgentRunRequest,
}

impl Turn {
    pub(super) fn requests(&self) -> Vec<Value> {
        self.provider.requests.lock().expect("requests").clone()
    }

    pub(super) async fn invocations(&self) -> Value {
        crate::r03_tests::success(
            &self.runtime,
            "agent.hooks.invocations",
            json!({"path": null, "turn_task_id": null, "limit": 50}),
        )
        .await["invocations"]
            .clone()
    }
}

/// Starts one HookRead turn with these Hooks trusted.
pub(super) async fn turn_with(hooks: &[(&str, &[&str], &[&str])]) -> Turn {
    started(prepared(hooks, true).await).await
}

pub(super) async fn started(prepared: Prepared) -> Turn {
    let Prepared { host, provider, runtime, session, model } = prepared;
    let run = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    Turn { _host: host, provider, runtime, request }
}

/// The text of the last tool message in one model request.
pub(super) fn last_tool_message(request: &Value) -> String {
    request["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .rev()
        .find(|message| message["role"] == "tool")
        .expect("the tool result reached the model")["content"]
        .as_str()
        .expect("text")
        .to_owned()
}

/// Runs one turn to completion and returns the tool result text the model received next.
pub(super) async fn tool_result_seen(
    hooks: &[(&str, &[&str], &[&str])],
) -> (String, crate::RuntimeState) {
    let turn = turn_with(hooks).await;
    wait(&turn.runtime, &turn.request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let seen = last_tool_message(&turn.requests()[1]);
    (seen, turn.runtime)
}
