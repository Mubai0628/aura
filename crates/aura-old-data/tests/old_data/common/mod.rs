//! A user's folders inside a temporary folder, and the layouts each Aura version left there.
#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use aura_data_location::{
    DataMarker, LocationRecord, MARKER_FILE, RECORD_FILE, Roots, write_marker, write_record,
};
use aura_old_data::{Installed, OldData, Scope};
use aura_runtime_contract::{BundleNamespace, PRODUCTION_BUNDLE_NAMESPACE};

pub fn namespace(name: &str) -> BundleNamespace {
    name.parse().expect("namespace")
}

pub fn current() -> BundleNamespace {
    namespace("aura.desktop.test.current")
}

pub fn production() -> BundleNamespace {
    PRODUCTION_BUNDLE_NAMESPACE.parse().expect("production")
}

pub fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    fs::write(path, bytes).expect("file");
}

pub struct Home {
    pub folder: tempfile::TempDir,
    pub roots: Roots,
}

impl Home {
    pub fn new() -> Self {
        let folder = tempfile::tempdir().expect("home");
        let roots = Roots {
            local: folder.path().join("Local"),
            roaming: folder.path().join("Roaming"),
            profile: folder.path().join("Profile"),
            known_local: folder.path().join("Local"),
        };
        Self { folder, roots }
    }

    pub fn local(&self) -> PathBuf {
        self.roots.local_aura()
    }

    pub fn roaming(&self) -> PathBuf {
        self.roots.roaming_aura()
    }

    /// The current version's own folders, which are never found: its namespace and the Core
    /// log every installation at the default places writes.
    pub fn current_version(&self) {
        let [local, roaming] = self.roots.namespace_folders();
        let hash = current().object_name_hash();
        write(&local.join(&hash).join(r"data\v2-execution\aura-v2.db"), b"current");
        write(&roaming.join(&hash).join("aura.toml"), b"current = true");
        write(&self.local().join(r"logs\core.jsonl"), b"{}\n");
    }

    /// What the versions before namespaces wrote at the top of Aura's folders (08-29).
    pub fn early_layout(&self) {
        let local = self.local();
        write(&local.join(r"data\aura.db"), &[7; 4096]);
        write(&local.join(r"desktop\window-state.json"), b"{}");
        write(&local.join(r"launcher\crash-state.v1.json"), b"{}");
        write(&local.join(r"config-history\aura\1.toml"), b"a = 1");
        for empty in ["e2e-runs", "plugins", "runtime", "skills", "tools", "web", "worktrees"] {
            fs::create_dir_all(local.join(empty)).expect("folder");
        }
        let roaming = self.roaming();
        fs::create_dir_all(roaming.join("settings")).expect("settings");
        for file in ["aura.toml", "commands.json", "hooks.toml", "mcp.json", "models.json"] {
            write(&roaming.join(file), b"{}");
        }
    }

    /// A namespace's folders as every version since namespaces left them.
    pub fn namespace(&self, namespace: &BundleNamespace) -> (PathBuf, PathBuf) {
        let [local, roaming] = self.roots.namespace_folders();
        let hash = namespace.object_name_hash();
        let (local, roaming) = (local.join(&hash), roaming.join(&hash));
        write(&local.join(r"data\v2-execution\aura-v2.db"), &[1; 8192]);
        write(&local.join(r"logs\main.jsonl"), b"{}\n");
        write(&local.join(r"launcher\ready.v1.json"), b"{}");
        for empty in ["config-history", "plugins", "runtime", "worktrees"] {
            fs::create_dir_all(local.join(empty)).expect("folder");
        }
        write(&roaming.join(r"settings\ui.toml"), b"theme = 'light'");
        write(&roaming.join("models.json"), b"{}");
        (local, roaming)
    }

    /// A namespace whose data was moved to a folder of its own, with the record that says so.
    pub fn moved_namespace(&self, namespace: &BundleNamespace) -> (PathBuf, PathBuf, PathBuf) {
        let (local, roaming) = self.namespace(namespace);
        fs::remove_dir_all(local.join("data")).expect("data moves out");
        let location = self.folder.path().join(r"D\Aura 数据");
        write(&location.join(r"data\v2-execution\aura-v2.db"), &[2; 8192]);
        write(&location.join(r"logs\core.jsonl"), b"{}\n");
        fs::create_dir_all(location.join("ui-profile")).expect("profile");
        write_marker(&location, &DataMarker::new(namespace.clone())).expect("marker");
        write_record(
            &local.join(RECORD_FILE),
            &LocationRecord::new(namespace.clone(), location.clone()),
        )
        .expect("record");
        assert!(location.join(MARKER_FILE).is_file());
        (local, roaming, location)
    }

    pub fn find(&self, installed: &Installed) -> Vec<OldData> {
        aura_old_data::find(&Scope { roots: &self.roots, current: &current(), installed })
    }
}

pub fn nothing_installed() -> Installed {
    Installed::Known(Vec::new())
}

pub fn ids(found: &[OldData]) -> Vec<&str> {
    found.iter().map(|item| item.id.as_str()).collect()
}

pub fn namespace_id(namespace: &BundleNamespace) -> String {
    format!("namespace-{}", namespace.object_name_hash())
}
