//! What the background service tests share: the plugin page's routes over a temporary Aura
//! folder, a plugin in development with a Node.js service or files of its own, the Skill's
//! helper libraries, and waiting for what it logs; a fake Aura for a service run by itself, and
//! the Skill's fake chat platform.
#![allow(dead_code)]
pub mod fake_aura;
pub mod fake_platform;

use aura_config::{AuraPaths, SettingsFacade};
use aura_extension_service::plugins::{PluginContext, PluginRuntime};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

pub struct Fixture {
    pub root: tempfile::TempDir,
    pub plugins: Arc<PluginRuntime>,
    context: PluginContext,
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary Aura folders");
        let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
        let settings = SettingsFacade::open(paths).expect("settings");
        let plugins = PluginRuntime::new(settings, None);
        let context = PluginContext::new(Arc::clone(&plugins));
        Self { root, plugins, context }
    }

    pub fn call(&self, method: &str, mut params: Value) -> Value {
        if method != "agent.plugins.preview" && method != "agent.plugins.list" {
            let revision = self.call("agent.plugins.list", json!({}))["revision"].clone();
            params["expected_revision"] = revision;
        }
        self.context.dispatch(method, params).unwrap_or_else(|error| panic!("{method}: {error}"))
    }

    /// A plugin in development, trusted: `manifest` is its manifest after its name, version and
    /// description, `service` its `service.mjs`.
    pub fn plugin(&self, name: &str, manifest: &str, service: &str) {
        self.plugin_with(name, manifest, &[("service.mjs", service)]);
    }

    /// As [`Self::plugin`], with the plugin's other files as (path in its folder, content).
    pub fn plugin_with(&self, name: &str, manifest: &str, files: &[(&str, &str)]) {
        let manifest = format!(
            "schema_version = 2\nname = \"{name}\"\nversion = \"0.1.0\"\n\
             description = \"测试服务\"\n{manifest}"
        );
        let mut all = vec![("aura-plugin.toml", manifest.as_str())];
        all.extend_from_slice(files);
        self.plugin_files(name, &all);
    }

    /// A plugin in development made of these files as they are, trusted.
    pub fn plugin_files(&self, name: &str, files: &[(&str, &str)]) {
        let folder = self.root.path().join("sources").join(name);
        std::fs::create_dir_all(&folder).expect("plugin folder");
        for (path, content) in files {
            let path = folder.join(path);
            std::fs::create_dir_all(path.parent().expect("its folder")).expect("its folder");
            std::fs::write(path, content).expect("plugin file");
        }
        self.call("agent.plugins.add_development", json!({"path": folder}));
        let preview = self.call("agent.plugins.preview", json!({"name": name}));
        self.call(
            "agent.plugins.trust",
            json!({"name": name, "expected_fingerprint": preview["fingerprint"]}),
        );
    }

    pub fn configure(&self, name: &str, change: Value) {
        let mut request = json!({"name": name});
        request
            .as_object_mut()
            .expect("object")
            .extend(change.as_object().expect("change").clone());
        self.call("agent.plugins.configure", request);
    }

    pub fn view(&self, name: &str) -> Value {
        let listed = self.call("agent.plugins.list", json!({}));
        let plugins = listed["plugins"].as_array().expect("plugins").clone();
        plugins.into_iter().find(|plugin| plugin["name"] == name).expect("listed")
    }

    pub fn logged(&self, name: &str) -> Vec<String> {
        let (entries, _) = self.plugins.recent_logs(name, 100).expect("logs");
        entries.into_iter().map(|entry| entry.text).collect()
    }
}

/// The `[service]` table that runs `service.mjs` with Node.js.
pub const NODE_SERVICE: &str =
    "[service]\ncommand = \"node\"\narguments = [\"${PLUGIN_ROOT}/service.mjs\"]\n";

/// A file of the plugin-writing Skill as Aura ships it, `/`-separated under its folder.
pub fn skill_path(file: &str) -> std::path::PathBuf {
    let skill = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/skills/write-aura-plugin");
    file.split('/').fold(skill, |path, part| path.join(part))
}

pub fn skill_file(file: &str) -> String {
    let path = skill_path(file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// One of the helper libraries the Skill gives a service.
pub fn sdk(file: &str) -> String {
    skill_file(&format!("sdk/{file}"))
}

pub async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..300 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{what} did not happen in 30 s");
}

/// What the service logged for its request `id`: `ok <result>` or the error's code.
pub fn answer(logged: &[String], id: usize) -> Option<String> {
    let prefix = format!("{id} ");
    logged.iter().find_map(|text| text.strip_prefix(&prefix).map(str::to_owned))
}
