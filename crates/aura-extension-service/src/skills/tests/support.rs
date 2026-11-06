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

/// One workspace a fixture lists, and its root folder.
pub(super) struct Workspace {
    pub id: aura_domain_common::WorkspaceId,
    pub folder: PathBuf,
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

    /// A fixture whose listings also cover one workspace, with its root folder.
    pub fn with_workspace() -> (Self, Workspace) {
        let root = tempfile::tempdir().expect("temporary Skill fixture");
        let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
        let settings = SettingsFacade::open(paths).expect("temporary settings");
        let data = root.path().join("data");
        let persistence = match aura_persistence::PersistenceCoordinator::start(
            aura_persistence::PersistenceOptions::new(
                data.join("aura.sqlite3"),
                data.join("artifacts"),
                aura_domain_common::RuntimeInstanceId::new(),
            ),
        )
        .expect("persistence")
        {
            aura_persistence::PersistenceStartup::Ready(value) => std::sync::Arc::new(value),
            aura_persistence::PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected recovery: {}", value.reason)
            }
        };
        let workspaces = aura_workspace::WorkspaceRuntime::open(
            persistence,
            aura_workspace::WorkspaceRuntimeOptions::default(),
        )
        .expect("workspace runtime");
        let folder = root.path().join("work");
        std::fs::create_dir_all(&folder).expect("workspace folder");
        let created = workspaces
            .create(
                aura_workspace_domain::WorkspaceCreateCommand {
                    selected_root: folder.display().to_string(),
                    display_name: "skills".to_owned(),
                },
                aura_domain_common::CorrelationId::new(),
            )
            .expect("workspace");
        let runtime = SkillRuntime::new(std::sync::Arc::clone(&settings), Some(workspaces));
        let store = runtime.store_root();
        let profile = root.path().join("local").join("test-profile");
        let context = SkillContext::new(std::sync::Arc::clone(&runtime));
        let workspace = Workspace { id: created.workspace.workspace_id, folder };
        (Self { _root: root, settings, runtime, context, profile, store }, workspace)
    }

    pub fn global(&self) -> PathBuf {
        self.profile.join(".agents").join("skills")
    }

    pub fn claude(&self) -> PathBuf {
        self.profile.join(".claude").join("skills")
    }

    /// The listing for one workspace.
    pub fn listed_in(&self, workspace: &Workspace) -> Value {
        self.dispatch("agent.skills.list", json!({"workspace_id": workspace.id})).expect("list")
    }

    /// The Skill whose folder is `relative` and whose source is `source`, listed for `workspace`.
    pub fn view_in(&self, workspace: &Workspace, source: &str, relative: &str) -> Value {
        self.listed_in(workspace)["skills"]
            .as_array()
            .expect("skills")
            .iter()
            .find(|skill| skill["relative"] == relative && skill["source"] == source)
            .cloned()
            .unwrap_or_else(|| panic!("{source} {relative} listed"))
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
}
