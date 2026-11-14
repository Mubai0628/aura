//! A user profile in TEMP: `LOCALAPPDATA`, `APPDATA`, the profile folder and the known local
//! application data folder, each its own folder, and a way to move a namespace's data.
#![allow(dead_code)]
use std::{
    fs,
    path::{Path, PathBuf},
};

use aura_data_location::{DataMarker, LocationRecord, Roots, write_marker, write_record};
use aura_runtime_contract::{BundleNamespace, PRODUCTION_BUNDLE_NAMESPACE};

pub struct Profile {
    pub folder: tempfile::TempDir,
    pub roots: Roots,
}

impl Profile {
    pub fn new() -> Self {
        let folder = tempfile::tempdir().expect("TEMP profile");
        let roots = Roots {
            local: folder.path().join("env-local"),
            roaming: folder.path().join("env-roaming"),
            profile: folder.path().join("profile"),
            known_local: folder.path().join("known-local"),
        };
        Self { folder, roots }
    }

    /// A folder elsewhere on the same disk, created empty.
    pub fn folder(&self, name: &str) -> PathBuf {
        let path = self.folder.path().join(name);
        fs::create_dir_all(&path).expect("folder");
        path
    }

    /// Moves `namespace`'s data to `location` the way the record and marker say it: under
    /// `LOCALAPPDATA` (`local`) or the known local folder.
    pub fn record_move(&self, local: &Path, namespace: &BundleNamespace, location: &Path) {
        fs::create_dir_all(location).expect("location");
        write_marker(location, &DataMarker::new(namespace.clone())).expect("marker");
        let fixed = local.join("Aura/namespaces").join(namespace.object_name_hash());
        fs::create_dir_all(&fixed).expect("fixed folder");
        write_record(
            &fixed.join("location.v1.json"),
            &LocationRecord::new(namespace.clone(), location.to_path_buf()),
        )
        .expect("record");
    }

    pub fn record_path(&self, namespace: &BundleNamespace) -> PathBuf {
        self.roots
            .local
            .join("Aura/namespaces")
            .join(namespace.object_name_hash())
            .join("location.v1.json")
    }

    /// Every file and folder under the profile, to show resolving changed nothing.
    pub fn listing(&self) -> Vec<PathBuf> {
        fn walk(path: &Path, found: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(path).into_iter().flatten().flatten() {
                found.push(entry.path());
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    walk(&entry.path(), found);
                }
            }
        }
        let mut found = Vec::new();
        walk(self.folder.path(), &mut found);
        found.sort();
        found
    }
}

pub fn production() -> BundleNamespace {
    PRODUCTION_BUNDLE_NAMESPACE.parse().expect("production namespace")
}

pub fn test_namespace() -> BundleNamespace {
    "aura.desktop.test.data-location".parse().expect("test namespace")
}
