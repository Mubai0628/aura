use super::super::*;
use aura_config::{AuraPaths, SettingsFacade};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(super) struct Fixture {
    pub _root: tempfile::TempDir,
    pub settings: std::sync::Arc<SettingsFacade>,
    pub runtime: std::sync::Arc<SkillRuntime>,
    pub context: SkillContext,
    /// The TEMP folder standing in for the user profile.
    pub profile: PathBuf,
    pub store: PathBuf,
}

pub(super) fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, text).expect("file");
}

/// A Skill folder `name` under `parent`, with one reference file.
pub(super) fn skill(parent: &Path, name: &str) -> PathBuf {
    let directory = parent.join(name);
    write(
        &directory.join("SKILL.md"),
        &format!(
            "---\nname: {name}\ndescription: Test skill {name}\n---\n\nUse [the guide](references/guide.md).\n"
        ),
    );
    write(&directory.join("references/guide.md"), "Step one.\n");
    directory
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary Skill fixture");
        let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
        let settings = SettingsFacade::open(paths).expect("temporary settings");
        let runtime = SkillRuntime::new(std::sync::Arc::clone(&settings), None);
        let store = runtime.store_root();
        let profile = root.path().join("local").join("test-profile");
        let context = SkillContext::new(std::sync::Arc::clone(&runtime));
        Self { _root: root, settings, runtime, context, profile, store }
    }

    pub fn global(&self) -> PathBuf {
        self.profile.join(".agents").join("skills")
    }

    pub fn dispatch(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.context.dispatch(method, params)
    }

    pub fn list(&self) -> Value {
        self.dispatch("agent.skills.list", json!({})).expect("list")
    }

    pub fn revision(&self) -> u64 {
        self.list()["revision"].as_u64().expect("revision")
    }

    /// The listed Skill whose folder is `relative` under its root.
    pub fn view(&self, relative: &str) -> Value {
        self.list()["skills"]
            .as_array()
            .expect("skills")
            .iter()
            .find(|skill| skill["relative"] == relative)
            .cloned()
            .unwrap_or_else(|| panic!("{relative} listed"))
    }

    pub fn trust(&self, relative: &str) -> Value {
        let view = self.view(relative);
        self.dispatch(
            "agent.skills.trust",
            json!({"id": view["id"], "expected_fingerprint": view["fingerprint"], "expected_revision": self.revision()}),
        )
        .expect("trusted")
    }
}
