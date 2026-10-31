use super::super::*;
use aura_config::{AuraPaths, SettingsFacade};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) struct Fixture {
    pub root: tempfile::TempDir,
    pub settings: Arc<SettingsFacade>,
    pub runtime: Arc<PluginRuntime>,
    pub context: PluginContext,
}

pub(super) fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, text).expect("file");
}

/// An Aura plugin `name` at `version` under `parent`: one Skill, one command and one Hook.
pub(super) fn plugin(parent: &Path, name: &str, version: &str) -> PathBuf {
    let root = parent.join(format!("{name}-source"));
    write(
        &root.join("aura-plugin.toml"),
        &format!(
            "schema_version = 1\nname = \"{name}\"\nversion = \"{version}\"\ndescription = \"测试插件\"\n"
        ),
    );
    write(
        &root.join("skills/guide/SKILL.md"),
        &format!("---\nname: guide\ndescription: Guide of {version}\n---\n\nRead the diff.\n"),
    );
    write(
        &root.join("commands/review/command.toml"),
        "name = \"review\"\ndescription = \"审查改动\"\nversion = 1\n",
    );
    write(&root.join("commands/review/template.md"), "请审查 $ARGUMENTS 的改动。\n");
    write(
        &root.join("hooks/check/hook.toml"),
        "schema_version = 1\nname = \"check\"\nversion = \"1\"\nevents = [\"tool.before_call\"]\n[command]\npowershell_script = \"check.ps1\"\n",
    );
    write(&root.join("hooks/check/check.ps1"), "'{\"decision\":\"continue\"}'\n");
    root
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary plugin fixture");
        let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
        let settings = SettingsFacade::open(paths).expect("temporary settings");
        let runtime = PluginRuntime::new(Arc::clone(&settings), None);
        let context = PluginContext::new(Arc::clone(&runtime));
        Self { root, settings, runtime, context }
    }

    /// A new runtime on the same folders, as when Core starts again.
    pub fn restart(&mut self) {
        self.runtime = PluginRuntime::new(Arc::clone(&self.settings), None);
        self.context = PluginContext::new(Arc::clone(&self.runtime));
    }

    pub fn source(&self) -> PathBuf {
        self.root.path().join("sources")
    }

    pub fn dispatch(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.context.dispatch(method, params)
    }

    pub fn list(&self) -> Value {
        self.dispatch("agent.plugins.list", json!({})).expect("list")
    }

    pub fn revision(&self) -> u64 {
        self.list()["revision"].as_u64().expect("revision")
    }

    pub fn view(&self, name: &str) -> Value {
        self.list()["plugins"]
            .as_array()
            .expect("plugins")
            .iter()
            .find(|plugin| plugin["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} listed"))
    }

    pub fn listed(&self, name: &str) -> bool {
        self.list()["plugins"].as_array().expect("plugins").iter().any(|p| p["name"] == name)
    }

    pub fn import(&self, source: &Path) -> anyhow::Result<Value> {
        self.dispatch(
            "agent.plugins.import",
            json!({"path": source.display().to_string(), "expected_revision": self.revision()}),
        )
    }

    pub fn trust(&self, name: &str, pending: bool) -> anyhow::Result<Value> {
        let preview =
            self.dispatch("agent.plugins.preview", json!({"name": name, "pending": pending}))?;
        self.dispatch(
            "agent.plugins.trust",
            json!({"name": name, "pending": pending, "expected_fingerprint": preview["fingerprint"], "expected_revision": self.revision()}),
        )
    }

    pub fn change(&self, method: &str, name: &str) -> anyhow::Result<Value> {
        self.dispatch(method, json!({"name": name, "expected_revision": self.revision()}))
    }

    pub fn library(&self) -> PathBuf {
        self.runtime.library()
    }

    pub fn saved(&self, name: &str) -> Option<aura_plugins::PluginSetting> {
        self.settings.plugin_registry().plugin(name).cloned()
    }
}
