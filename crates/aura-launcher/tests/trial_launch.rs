//! The launcher program with a stand-in desktop (`examples/fake_desktop.rs`): a waiting version
//! that reports ready becomes the active one, one that crashes is put back and recorded, one
//! closed before it is ready waits for the next start, one that does not check out never starts,
//! and going back starts the previous version the same way. Before a newer version starts, only
//! the stores its Core says it will migrate are copied.
#![allow(clippy::unwrap_used)]
use aura_domain_common::{ContentHash, ContentHashAlgorithm, ContractVersion, ReleaseBundleId};
use aura_launcher::{FailedTrial, TrialFailure, roll_back};
use aura_runtime_contract::{
    ActiveInstallState, BuildComponent, BuildComponentKind, BuildComponentManifest,
    BundleNamespace, ReleaseChannel, ReleaseDescriptor, ReleaseIdentity,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const NAMESPACE: &str = "aura.desktop.test.launcher-trial";

struct Install {
    root: tempfile::TempDir,
    data: tempfile::TempDir,
}

impl Install {
    fn new(active: &str, mode: &str) -> Self {
        let install =
            Self { root: tempfile::tempdir().unwrap(), data: tempfile::tempdir().unwrap() };
        fs::copy(env!("CARGO_BIN_EXE_AuraLauncher"), install.root.path().join("AuraLauncher.exe"))
            .unwrap();
        let descriptor = install.add(active, mode);
        install.write(&ActiveInstallState {
            active: active.into(),
            release_descriptor: descriptor,
            previous: None,
            pending: None,
            last_known_good: Some(active.into()),
        });
        install
    }

    /// A version folder whose `Aura.exe` is the stand-in desktop, behaving as `mode` says.
    fn add(&self, version: &str, mode: &str) -> ReleaseDescriptor {
        self.add_with_core(version, mode, format!("core {version}").into_bytes())
    }

    fn add_with_core(&self, version: &str, mode: &str, core: Vec<u8>) -> ReleaseDescriptor {
        let folder = self.root.path().join("versions").join(version);
        fs::create_dir_all(&folder).unwrap();
        let desktop = fs::read(fake_desktop()).unwrap();
        fs::write(folder.join("Aura.exe"), &desktop).unwrap();
        fs::write(folder.join("aura-core.exe"), &core).unwrap();
        fs::write(folder.join("fake-mode.txt"), mode).unwrap();
        let manifest = BuildComponentManifest {
            manifest_version: ContractVersion::CURRENT,
            bundle_namespace: NAMESPACE.parse().unwrap(),
            release_bundle_id: ReleaseBundleId::new(),
            release_channel: ReleaseChannel::Stable,
            product_version: version.into(),
            build_commit: "test".into(),
            target_triple: "x86_64-pc-windows-msvc".into(),
            components: vec![
                component("aura-desktop", BuildComponentKind::Desktop, "Aura.exe", &desktop),
                component("aura-core", BuildComponentKind::Core, "aura-core.exe", &core),
            ],
        };
        let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        fs::write(folder.join("component-manifest.json"), &bytes).unwrap();
        ReleaseDescriptor {
            bundle_namespace: manifest.bundle_namespace,
            identity: ReleaseIdentity {
                release_bundle_id: manifest.release_bundle_id,
                component_manifest_hash: hash(&bytes),
            },
        }
    }

    fn state(&self) -> ActiveInstallState {
        serde_json::from_slice(&fs::read(self.root.path().join("active.json")).unwrap()).unwrap()
    }

    fn write(&self, state: &ActiveInstallState) {
        fs::write(self.root.path().join("active.json"), serde_json::to_vec(state).unwrap())
            .unwrap();
    }

    fn wait(&self, pending: &str) {
        let mut state = self.state();
        state.pending = Some(pending.into());
        self.write(&state);
    }

    fn launch(&self) {
        let status = Command::new(self.root.path().join("AuraLauncher.exe"))
            .env("LOCALAPPDATA", self.data.path())
            .status()
            .unwrap();
        assert!(status.success(), "the launcher failed: {status}");
    }

    fn launcher_data(&self) -> PathBuf {
        let namespace: BundleNamespace = NAMESPACE.parse().unwrap();
        self.data.path().join("Aura/namespaces").join(namespace.object_name_hash()).join("launcher")
    }

    fn failed_trial(&self) -> FailedTrial {
        serde_json::from_slice(
            &fs::read(self.launcher_data().join("trial-failed.v1.json")).unwrap(),
        )
        .unwrap()
    }

    fn versions(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.root.path().join("versions"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

/// The stand-in desktop; cargo builds examples only when a crate's whole test suite runs.
fn fake_desktop() -> PathBuf {
    let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    let program = deps.parent().unwrap().join("examples").join("fake_desktop.exe");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/fake_desktop.rs");
    let built = fs::metadata(&program).and_then(|metadata| metadata.modified()).ok();
    let changed = fs::metadata(source).and_then(|metadata| metadata.modified()).unwrap();
    assert!(
        built.is_some_and(|built| built >= changed),
        "examples/fake_desktop is missing or older than its source: run `cargo test -p aura-launcher`"
    );
    program
}

fn hash(bytes: &[u8]) -> ContentHash {
    ContentHash::new(ContentHashAlgorithm::Sha256, format!("{:x}", Sha256::digest(bytes))).unwrap()
}

fn component(id: &str, kind: BuildComponentKind, path: &str, bytes: &[u8]) -> BuildComponent {
    BuildComponent {
        component_id: id.into(),
        kind,
        version: "test".into(),
        relative_path: path.into(),
        content_hash: hash(bytes),
        protocol_version: Some(ContractVersion::CURRENT),
    }
}

fn version_count(path: &Path) -> usize {
    fs::read_dir(path).map(|entries| entries.count()).unwrap_or(0)
}

#[test]
fn a_waiting_version_that_reports_ready_becomes_active_and_older_versions_go() {
    let install = Install::new("0.1.0", "exit");
    install.add("0.0.8", "exit");
    install.add("0.0.9", "exit");
    let next = install.add("0.1.1", "ready");
    install.wait("0.1.1");
    install.launch();
    let state = install.state();
    assert_eq!(state.active, "0.1.1");
    assert_eq!(state.release_descriptor, next);
    assert_eq!(state.previous.as_deref(), Some("0.1.0"));
    assert_eq!(state.pending, None);
    assert_eq!(install.versions(), ["0.1.0", "0.1.1"]);
}

#[test]
fn a_waiting_version_that_crashes_is_put_back_recorded_and_the_active_one_starts() {
    let install = Install::new("0.1.0", "exit");
    let before = install.state();
    install.add("0.1.1", "crash");
    install.wait("0.1.1");
    install.launch();
    let state = install.state();
    assert_eq!(state.active, "0.1.0");
    assert_eq!(state.release_descriptor, before.release_descriptor);
    assert_eq!(state.pending, None);
    let failed = install.failed_trial();
    assert_eq!(failed.version, "0.1.1");
    assert_eq!(failed.kept_version, "0.1.0");
    assert_eq!(failed.failure, TrialFailure::Crashed);
    assert_eq!(failed.exit_code, Some(3));
}

#[test]
fn a_waiting_version_closed_before_it_is_ready_waits_for_the_next_start() {
    let install = Install::new("0.1.0", "exit");
    install.add("0.1.1", "exit");
    install.wait("0.1.1");
    install.launch();
    let state = install.state();
    assert_eq!(state.active, "0.1.0");
    assert_eq!(state.pending.as_deref(), Some("0.1.1"));
    assert_eq!(version_count(&install.launcher_data().join("trial-failed.v1.json")), 0);
}

#[test]
fn a_waiting_version_that_does_not_check_out_never_starts() {
    let install = Install::new("0.1.0", "exit");
    install.add("0.1.1", "ready");
    fs::write(install.root.path().join("versions/0.1.1/aura-core.exe"), b"changed").unwrap();
    install.wait("0.1.1");
    install.launch();
    let state = install.state();
    assert_eq!(state.active, "0.1.0");
    assert_eq!(state.pending, None);
    let failed = install.failed_trial();
    assert_eq!(failed.version, "0.1.1");
    assert_eq!(failed.failure, TrialFailure::Invalid);
    assert!(failed.detail.contains("hash mismatch"), "{}", failed.detail);
}

#[test]
fn going_back_starts_the_previous_version_as_a_trial() {
    let install = Install::new("0.1.1", "exit");
    let previous = install.add("0.1.0", "ready");
    let mut state = install.state();
    state.previous = Some("0.1.0".into());
    install.write(&roll_back(&state).unwrap());
    install.launch();
    let state = install.state();
    assert_eq!(state.active, "0.1.0");
    assert_eq!(state.release_descriptor, previous);
    assert_eq!(state.previous.as_deref(), Some("0.1.1"));
}

impl Install {
    fn log(&self) -> String {
        fs::read_to_string(self.launcher_data().parent().unwrap().join("logs/launcher.jsonl"))
            .unwrap_or_default()
    }

    fn store(&self) -> PathBuf {
        let namespace: BundleNamespace = NAMESPACE.parse().unwrap();
        self.data
            .path()
            .join("Aura/namespaces")
            .join(namespace.object_name_hash())
            .join("data/aura.db")
    }
}

#[test]
fn a_newer_version_that_crashes_after_changing_the_data_leaves_the_data_as_it_was() {
    let install = Install::new("0.1.0", "exit");
    fs::create_dir_all(install.store().parent().unwrap()).unwrap();
    fs::write(install.store(), "conversations of 0.1.0").unwrap();
    install.add("0.1.1", "crash-after-writing");
    install.wait("0.1.1");
    install.launch();
    assert_eq!(install.state().active, "0.1.0");
    assert_eq!(fs::read_to_string(install.store()).unwrap(), "conversations of 0.1.0");
    let backups = install.store().parent().unwrap().parent().unwrap().join("backups");
    let replaced = fs::read_dir(&backups)
        .unwrap()
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().starts_with("replaced-"))
        .unwrap_or_else(|| {
            panic!("the data the failed version wrote is not kept aside: {}", install.log())
        });
    assert_eq!(
        fs::read_to_string(replaced.path().join("data/aura.db")).unwrap(),
        "written by 0.1.1"
    );
}

/// A newer version whose Core answers `--store-migration-check` with `answer`, over both stores.
fn newer_version_answering(answer: &str) -> Install {
    let install = Install::new("0.1.0", "exit");
    let v2 = install.store().parent().unwrap().join("v2-execution/aura-v2.db");
    fs::create_dir_all(v2.parent().unwrap()).unwrap();
    fs::write(install.store(), "conversations of 0.1.0").unwrap();
    fs::write(v2, "operations of 0.1.0").unwrap();
    install.add_with_core("0.1.1", "ready", fs::read(fake_desktop()).unwrap());
    fs::write(install.root.path().join("versions/0.1.1/fake-migration.json"), answer).unwrap();
    install.wait("0.1.1");
    install.launch();
    assert_eq!(install.state().active, "0.1.1", "{}", install.log());
    install
}

#[test]
fn a_newer_version_that_migrates_no_store_starts_without_a_copy() {
    let install =
        newer_version_answering(r#"{"data/aura.db":false,"data/v2-execution/aura-v2.db":false}"#);
    let backups = install.store().parent().unwrap().parent().unwrap().join("backups");
    assert!(!backups.join("before-0.1.1").exists(), "{}", install.log());
    assert!(install.log().contains("no_store_migration"), "{}", install.log());
}

#[test]
fn a_newer_version_gets_a_copy_of_only_the_store_it_migrates() {
    let install =
        newer_version_answering(r#"{"data/aura.db":true,"data/v2-execution/aura-v2.db":false}"#);
    let copy = install.store().parent().unwrap().parent().unwrap().join("backups/before-0.1.1");
    assert_eq!(fs::read_to_string(copy.join("data/aura.db")).unwrap(), "conversations of 0.1.0");
    assert!(!copy.join("data/v2-execution").exists(), "{}", install.log());
}

#[test]
fn a_desktop_that_applied_an_update_restarts_into_the_new_version() {
    let install = Install::new("0.1.0", "update-and-restart");
    fs::write(install.root.path().join("versions/0.1.0/fake-next.txt"), "0.1.1").unwrap();
    install.add("0.1.1", "ready");
    install.launch();
    let state = install.state();
    assert_eq!(state.active, "0.1.1", "{}", install.log());
    assert_eq!(state.previous.as_deref(), Some("0.1.0"));
    assert_eq!(state.pending, None);
}
