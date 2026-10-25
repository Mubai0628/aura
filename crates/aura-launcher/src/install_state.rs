//! Which version the launcher starts, and how `active.json` changes around a version that waits to
//! be tried. A waiting version (`pending`) is started once as a trial: it becomes the active one
//! when the desktop reports it ready, and the waiting entry is dropped when it fails before that.
//! Going back to the previous version is the same trial with the previous version waiting.
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::Path,
};

use anyhow::{Context, Result};
use aura_runtime_contract::{ActiveInstallState, ReleaseDescriptor};

/// What the launcher starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Start {
    Active,
    Trial(String),
}

pub fn choose(state: &ActiveInstallState) -> Start {
    match &state.pending {
        Some(pending) if *pending != state.active => Start::Trial(pending.clone()),
        _ => Start::Active,
    }
}

/// The trial version reported ready: it is active from now on, and the version it replaced is the
/// one to go back to.
pub fn promote(
    state: &ActiveInstallState,
    version: &str,
    descriptor: ReleaseDescriptor,
) -> ActiveInstallState {
    ActiveInstallState {
        active: version.to_owned(),
        release_descriptor: descriptor,
        previous: Some(state.active.clone()),
        pending: None,
        last_known_good: Some(version.to_owned()),
    }
}

/// The trial failed before it was ready: the waiting entry goes and the active version stays.
pub fn abandon(state: &ActiveInstallState) -> ActiveInstallState {
    ActiveInstallState { pending: None, ..state.clone() }
}

/// The previous version waits to be tried, so going back is checked like an update.
pub fn roll_back(state: &ActiveInstallState) -> Option<ActiveInstallState> {
    let previous = state.previous.clone().filter(|previous| *previous != state.active)?;
    Some(ActiveInstallState { pending: Some(previous), ..state.clone() })
}

/// Whether dotted version `left` is newer than `right`, comparing each part as a number.
pub fn newer(left: &str, right: &str) -> bool {
    let parts = |text: &str| -> Vec<u64> {
        text.split('.').map(|part| part.parse().unwrap_or(0)).collect()
    };
    parts(left) > parts(right)
}

/// The versions an installation keeps on disk.
pub fn kept(state: &ActiveInstallState) -> BTreeSet<String> {
    [Some(&state.active), state.previous.as_ref(), state.pending.as_ref()]
        .into_iter()
        .flatten()
        .cloned()
        .collect()
}

pub fn read(root: &Path) -> Result<ActiveInstallState> {
    let path = root.join("active.json");
    let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).context("active.json is not a valid Aura contract")
}

pub fn write(root: &Path, state: &ActiveInstallState) -> Result<()> {
    let path = root.join("active.json");
    let mut bytes = serde_json::to_vec_pretty(state)?;
    bytes.push(b'\n');
    replace(&path, &bytes).with_context(|| format!("failed to write {}", path.display()))
}

/// Writes `bytes` beside `path` and moves them over it, so a reader sees the old or the new
/// content and never a part.
pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(temporary, path)
}

/// Removes version folders the state no longer names. Folders starting with a dot are an update
/// still being staged and stay.
pub fn remove_unkept_versions(root: &Path, state: &ActiveInstallState) -> Vec<String> {
    let keep = kept(state);
    let mut removed = Vec::new();
    let Ok(entries) = fs::read_dir(root.join("versions")) else {
        return removed;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || keep.contains(&name) {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && fs::remove_dir_all(entry.path()).is_ok()
        {
            removed.push(name);
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{ContentHash, ContentHashAlgorithm, ReleaseBundleId};
    use aura_runtime_contract::ReleaseIdentity;

    fn descriptor() -> ReleaseDescriptor {
        ReleaseDescriptor {
            bundle_namespace: "aura.desktop.test.launcher".parse().expect("namespace"),
            identity: ReleaseIdentity {
                release_bundle_id: ReleaseBundleId::new(),
                component_manifest_hash: ContentHash::new(
                    ContentHashAlgorithm::Sha256,
                    "00".repeat(32),
                )
                .expect("hash"),
            },
        }
    }

    fn state(active: &str, previous: Option<&str>, pending: Option<&str>) -> ActiveInstallState {
        ActiveInstallState {
            active: active.to_owned(),
            release_descriptor: descriptor(),
            previous: previous.map(str::to_owned),
            pending: pending.map(str::to_owned),
            last_known_good: Some(active.to_owned()),
        }
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.1.1"));
    }

    #[test]
    fn a_waiting_version_is_tried_and_otherwise_the_active_one_starts() {
        assert_eq!(choose(&state("0.1.0", None, None)), Start::Active);
        assert_eq!(choose(&state("0.1.0", None, Some("0.1.1"))), Start::Trial("0.1.1".into()));
        assert_eq!(choose(&state("0.1.0", None, Some("0.1.0"))), Start::Active);
    }

    #[test]
    fn a_ready_trial_becomes_active_and_keeps_the_one_it_replaced() {
        let promoted =
            promote(&state("0.1.0", Some("0.0.9"), Some("0.1.1")), "0.1.1", descriptor());
        assert_eq!(promoted.active, "0.1.1");
        assert_eq!(promoted.previous.as_deref(), Some("0.1.0"));
        assert_eq!(promoted.pending, None);
        assert_eq!(promoted.last_known_good.as_deref(), Some("0.1.1"));
        assert_eq!(kept(&promoted), ["0.1.0".to_owned(), "0.1.1".to_owned()].into());
    }

    #[test]
    fn a_failed_trial_leaves_the_active_version_as_it_was() {
        let before = state("0.1.0", Some("0.0.9"), Some("0.1.1"));
        let after = abandon(&before);
        assert_eq!(after.active, "0.1.0");
        assert_eq!(after.previous.as_deref(), Some("0.0.9"));
        assert_eq!(after.pending, None);
        assert_eq!(after.release_descriptor, before.release_descriptor);
    }

    #[test]
    fn going_back_tries_the_previous_version_and_needs_one() {
        let back = roll_back(&state("0.1.1", Some("0.1.0"), None)).expect("previous version");
        assert_eq!(choose(&back), Start::Trial("0.1.0".into()));
        assert!(roll_back(&state("0.1.1", None, None)).is_none());
        assert!(roll_back(&state("0.1.1", Some("0.1.1"), None)).is_none());
    }

    #[test]
    fn only_versions_the_state_names_and_staging_folders_stay() {
        let root = tempfile::tempdir().expect("install root");
        for name in ["0.0.8", "0.0.9", "0.1.0", "0.1.1", ".staging-0.1.2"] {
            fs::create_dir_all(root.path().join("versions").join(name)).expect("version folder");
        }
        let mut removed = remove_unkept_versions(root.path(), &state("0.1.1", Some("0.1.0"), None));
        removed.sort();
        assert_eq!(removed, ["0.0.8", "0.0.9"]);
        for name in ["0.1.0", "0.1.1", ".staging-0.1.2"] {
            assert!(root.path().join("versions").join(name).is_dir(), "{name} was removed");
        }
    }

    #[test]
    fn the_state_file_is_replaced_whole() {
        let root = tempfile::tempdir().expect("install root");
        let first = state("0.1.0", None, Some("0.1.1"));
        write(root.path(), &first).expect("write");
        assert_eq!(read(root.path()).expect("read"), first);
        let second = abandon(&first);
        write(root.path(), &second).expect("rewrite");
        assert_eq!(read(root.path()).expect("reread"), second);
        assert!(!root.path().join("active.tmp").exists());
    }
}
