//! A user profile in TEMP with an installation's data at the default places: the stores, a
//! backup, the launcher's files, the Chrome folder, Core's log, a window profile with Chromium's
//! caches, and global memory. Destinations can be on D: under the repository's `target`, so a
//! move crosses drives as the owner's will.
#![allow(dead_code, clippy::unwrap_used)]
use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
};

use aura_data_location::{InstallCopy, Locations, Roots};
use aura_data_move::{Fault, Hooks, Outcome, Stage, perform, resume};
use aura_runtime_contract::{BundleNamespace, PRODUCTION_BUNDLE_NAMESPACE};

pub struct Profile {
    pub folder: tempfile::TempDir,
    pub roots: Roots,
    pub namespace: BundleNamespace,
}

pub fn production() -> BundleNamespace {
    PRODUCTION_BUNDLE_NAMESPACE.parse().unwrap()
}

pub fn test_namespace() -> BundleNamespace {
    "aura.desktop.test.data-move".parse().unwrap()
}

/// What each moved file holds, by its path below its part's folder.
pub const FILES: [(&str, &[u8]); 6] = [
    ("data/aura.db", b"legacy store"),
    ("data/v2-execution/aura-v2.db", b"execution store"),
    ("backups/before-0.3.0/aura-v2.db", b"copy before the update"),
    ("launcher/crash-state.v1.json", b"{}"),
    ("logs/launcher.jsonl", b"{\"event\":\"desktop_started\"}\n"),
    ("data/memory-vaults/3b2a.records/maintenance/6336.json", b"{\"vault\":1}"),
];

impl Profile {
    pub fn new(namespace: BundleNamespace) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let roots = Roots {
            local: folder.path().join("local"),
            roaming: folder.path().join("roaming"),
            profile: folder.path().join("profile"),
            known_local: folder.path().join("local"),
        };
        let profile = Self { folder, roots, namespace };
        let at = profile.default_place();
        for (path, bytes) in FILES {
            write(&at.data_root.join(path), bytes);
        }
        write(&at.chrome_root.join("manifest.json"), b"{\"name\":\"relay\"}");
        write(&at.core_log, b"{\"event\":\"core_started\"}\n");
        write(&at.ui_profile.join("Preferences"), b"{\"window\":1}");
        write(&at.ui_profile.join("Local Storage/leveldb/000003.log"), b"drafts");
        write(&at.ui_profile.join("Cache/Cache_Data/data_0"), &[7; 4096]);
        write(&at.ui_profile.join("Partitions/aura-browser/Cookies"), b"signed in");
        write(&at.ui_profile.join("Partitions/aura-browser/Code Cache/js/index"), &[1; 512]);
        if profile.namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE {
            write(&profile.roots.profile.join(".aura/memories/MEMORY.md"), b"- remembered");
        }
        profile
    }

    pub fn default_place(&self) -> Locations {
        Locations::for_place(&self.roots, &self.namespace, None)
            .with_global_memory(&self.roots, &self.namespace)
            .unwrap()
    }

    /// The locations as they resolve now, as the launcher and Core work them out.
    pub fn current(&self) -> Locations {
        Locations::resolve(&self.roots, &self.namespace, &InstallCopy::NotInstalled)
            .and_then(|locations| locations.with_global_memory(&self.roots, &self.namespace))
            .unwrap()
    }

    pub fn production(&self) -> bool {
        self.namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE
    }

    /// A new folder in TEMP, on the system drive.
    pub fn here(&self, name: &str) -> PathBuf {
        self.folder.path().join(name)
    }
}

/// A folder on D: under the repository's `target`, removed when dropped.
pub fn on_other_drive() -> tempfile::TempDir {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let root = repository.join("target").join("aura-data-move-tests");
    fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

pub fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// One run of the mover, recording what it reports.
#[derive(Default)]
pub struct Run {
    pub stages: RefCell<Vec<Stage>>,
    pub install_copies: RefCell<Vec<Option<PathBuf>>>,
    pub stores_checked: RefCell<Vec<PathBuf>>,
    /// The user presses 取消 once this many bytes are copied.
    pub stop_after: Option<u64>,
    /// The stores' check fails with this.
    pub stores_fail: Option<String>,
    /// Bytes copied so far, as the mover reported them.
    pub copied: Cell<u64>,
}

impl Run {
    fn with_hooks<R>(
        &self,
        profile: &Profile,
        fault: Option<Fault>,
        act: impl FnOnce(&Hooks<'_>) -> R,
    ) -> R {
        let progress = |stage: Stage, done: u64, _total: u64| {
            if self.stages.borrow().last() != Some(&stage) {
                self.stages.borrow_mut().push(stage);
            }
            if stage == Stage::Copying {
                self.copied.set(done);
            }
        };
        let cancelled = || self.stop_after.is_some_and(|limit| self.copied.get() >= limit);
        let check_stores = |data_root: &Path| {
            self.stores_checked.borrow_mut().push(data_root.to_path_buf());
            self.stores_fail.clone().map_or(Ok(()), Err)
        };
        let install_copy = |location: Option<&Path>| {
            self.install_copies.borrow_mut().push(location.map(Path::to_path_buf));
            Ok(())
        };
        act(&Hooks {
            progress: &progress,
            cancelled: &cancelled,
            check_stores: &check_stores,
            install_copy: &install_copy,
            production: profile.production(),
            fault,
        })
    }

    pub fn perform(&self, profile: &Profile, fault: Option<Fault>) -> Outcome {
        self.with_hooks(profile, fault, |hooks| {
            perform(&profile.roots, &profile.namespace, &profile.current(), hooks)
        })
    }

    pub fn resume(&self, profile: &Profile) -> Outcome {
        self.with_hooks(profile, None, |hooks| resume(&profile.roots, &profile.namespace, hooks))
    }
}

/// Asks for a move of `profile`'s data to `destination` (`None`: back to the default places).
pub fn ask(profile: &Profile, destination: Option<&Path>) {
    aura_data_move::request(
        &profile.namespace,
        &profile.current(),
        destination.map(Path::to_path_buf),
    )
    .unwrap();
}

/// Every file under `folder` with its bytes, by its path below `folder`, sorted.
pub fn files(folder: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, folder: &Path, found: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                walk(base, &path, found);
            } else {
                let below = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                found.push((below, fs::read(&path).unwrap()));
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, folder, &mut found);
    found.sort();
    found
}
