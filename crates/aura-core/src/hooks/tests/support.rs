//! Hook packages in TEMP that run the repository's test Hook, a temporary settings root and a
//! temporary V2 store. Nothing here reads the owner's configuration or runs a real model.
use super::super::*;
use aura_config::AuraPaths;
use aura_hooks::HookInvocationListRequest;
use serde_json::json;
use std::path::{Path, PathBuf};

pub(super) struct Fixture {
    pub root: tempfile::TempDir,
    pub runner: HookRunner,
    pub context: HookContext,
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary Hook fixture");
        let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
        let settings = SettingsFacade::open(paths).expect("temporary settings");
        let journal = Arc::new(
            crate::broker::BrokerJournal::open(&root.path().join("journal")).expect("journal"),
        );
        let runner = HookRunner {
            settings: settings.clone(),
            journal: journal.clone(),
            scratch_root: root.path().join("scratch"),
            plugins: None,
        };
        let context = HookContext::new(settings, journal);
        Self { root, runner, context }
    }

    /// A package that runs `aura-hook-fixture.exe` with `arguments`; `extra` is more TOML.
    pub fn package(&self, name: &str, events: &[&str], arguments: &[&str], extra: &str) -> PathBuf {
        let directory = self.root.path().join("hooks").join(name);
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
        let quote = |items: &[&str]| {
            items.iter().map(|item| format!("'{item}'")).collect::<Vec<_>>().join(", ")
        };
        std::fs::write(
            directory.join("hook.toml"),
            format!(
                "schema_version = 1\nname = \"{name}\"\nversion = \"1.0.0\"\nevents = [{}]\n{extra}\n[command]\nexecutable = \"bin/hook.exe\"\narguments = [{}]\n",
                quote(events),
                quote(arguments),
            ),
        )
        .expect("manifest");
        directory
    }

    pub fn dispatch(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.context.dispatch(method, params)
    }

    pub fn revision(&self) -> u64 {
        self.dispatch("agent.hooks.list", json!({})).expect("list")["revision"]
            .as_u64()
            .expect("revision")
    }

    /// Preview, then trust exactly what was previewed, enabled.
    pub fn trust(&self, path: &Path) {
        let preview = self
            .dispatch("agent.hooks.preview", json!({"path": path.display().to_string()}))
            .expect("preview");
        assert!(preview["problems"].as_array().is_some_and(Vec::is_empty), "{preview}");
        self.dispatch(
            "agent.hooks.trust",
            json!({
                "path": path.display().to_string(),
                "expected_fingerprint": preview["fingerprint"],
                "enabled": true,
                "expected_revision": self.revision(),
            }),
        )
        .expect("trusted");
    }

    pub fn call(&self, event: HookEvent, tool: Option<&str>, payload: Value) -> HookCall {
        HookCall {
            event,
            session_id: Some(SessionId::new()),
            turn_task_id: Some(TurnTaskId::new()),
            tool_call_id: None,
            tool_name: tool.map(str::to_owned),
            workspace_root: None,
            subject: None,
            payload,
        }
    }

    pub fn run(&self, call: HookCall) -> HookRunOutcome {
        self.runner.run(call, None, &AtomicBool::new(false)).expect("runs")
    }

    pub fn records(&self) -> Vec<HookInvocationView> {
        let store = self.runner.journal.lock().expect("journal");
        aura_persistence_v2::hooks::list(
            store.database.connection(),
            &HookInvocationListRequest { path: None, turn_task_id: None, limit: 100 },
        )
        .expect("records")
    }

    /// The stored stdin document of the newest record.
    pub fn newest_input(&self) -> Value {
        let store = self.runner.journal.lock().expect("journal");
        let text: String = store
            .database
            .connection()
            .query_row(
                "SELECT input FROM hook_invocations ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("input");
        serde_json::from_str(&text).expect("JSON envelope")
    }
}
