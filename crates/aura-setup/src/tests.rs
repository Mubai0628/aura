//! Installing, upgrading, repairing and uninstalling against a host inside a temporary folder.
#![allow(clippy::unwrap_used)]
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use aura_chrome_domain::protocol::{native_host_name, pairing_credential};
use aura_domain_common::{ContentHash, ContentHashAlgorithm, ContractVersion, ReleaseBundleId};
use aura_runtime_contract::{
    BuildComponent, BuildComponentKind, BuildComponentManifest, BundleNamespace, ReleaseChannel,
};
use sha2::{Digest, Sha256};
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    host::{FolderHost, Host, credential_prefix},
    install::{self, Kind, Options, Signature},
    pack, payload,
    uninstall::{plan_uninstall, uninstall},
};

const NAMESPACE: &str = "aura.desktop.test.setup";

fn namespace() -> BundleNamespace {
    NAMESPACE.parse().unwrap()
}

struct Work {
    folder: tempfile::TempDir,
}

impl Work {
    fn new() -> Self {
        let work = Self { folder: tempfile::tempdir().unwrap() };
        fs::write(work.stub(), b"MZ stand-in setup program").unwrap();
        work
    }

    fn stub(&self) -> PathBuf {
        self.folder.path().join("stub.exe")
    }

    fn host(&self) -> FolderHost {
        FolderHost { root: self.folder.path().join("host") }
    }

    /// A payload folder for `version` and the setup program packed from it.
    fn release(&self, version: &str) -> PathBuf {
        let payload = self.folder.path().join(format!("payload-{version}"));
        let folder = payload.join("versions").join(version);
        fs::create_dir_all(folder.join("resources")).unwrap();
        fs::write(payload.join("Aura.ico"), b"icon").unwrap();
        let desktop = format!("desktop {version}").into_bytes();
        let core = format!("core {version}").into_bytes();
        let launcher = format!("launcher {version}").into_bytes();
        let setup = format!("setup {version}").into_bytes();
        fs::write(folder.join("AuraLauncher.exe"), &launcher).unwrap();
        fs::write(folder.join("aura-setup.exe"), &setup).unwrap();
        fs::write(folder.join("Aura.exe"), &desktop).unwrap();
        fs::write(folder.join("aura-core.exe"), &core).unwrap();
        fs::write(folder.join("resources/app.asar"), b"app").unwrap();
        let component = |id: &str, kind, path: &str, bytes: &[u8]| BuildComponent {
            component_id: id.into(),
            kind,
            version: version.into(),
            relative_path: path.into(),
            content_hash: ContentHash::new(
                ContentHashAlgorithm::Sha256,
                format!("{:x}", Sha256::digest(bytes)),
            )
            .unwrap(),
            protocol_version: Some(ContractVersion::CURRENT),
        };
        let manifest = BuildComponentManifest {
            manifest_version: ContractVersion::CURRENT,
            bundle_namespace: namespace(),
            release_bundle_id: ReleaseBundleId::new(),
            release_channel: ReleaseChannel::Stable,
            product_version: version.into(),
            build_commit: "test".into(),
            target_triple: "x86_64-pc-windows-msvc".into(),
            components: vec![
                component(
                    "aura-launcher",
                    BuildComponentKind::Launcher,
                    "AuraLauncher.exe",
                    &launcher,
                ),
                component("aura-setup", BuildComponentKind::Setup, "aura-setup.exe", &setup),
                component("aura-desktop", BuildComponentKind::Desktop, "Aura.exe", &desktop),
                component("aura-core", BuildComponentKind::Core, "aura-core.exe", &core),
            ],
        };
        fs::write(
            folder.join("component-manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let setup = self.folder.path().join(format!("Aura-Setup-{version}.exe"));
        let update = self.folder.path().join(format!("Aura-Update-{version}.aura-update"));
        let packed = pack::pack(&payload, &self.stub(), &setup, &update).unwrap();
        assert_eq!(packed.version, version);
        setup
    }

    fn install(
        &self,
        setup: &Path,
        root: Option<PathBuf>,
        desktop: bool,
    ) -> anyhow::Result<install::Plan> {
        let host = self.host();
        let mut archive = payload::open_in_program(setup)?;
        let release = install::read_release(&mut archive)?;
        assert_eq!(release.signature, Signature::NotRequired);
        let plan = install::plan(&host, &release, root)?;
        install::install(
            &host,
            setup,
            &mut archive,
            &release,
            &plan,
            &Options { desktop_shortcut: desktop, while_running: false },
            &mut |_, _| {},
        )?;
        Ok(plan)
    }

    fn root(&self) -> PathBuf {
        self.host().default_root(&namespace()).unwrap()
    }

    fn state(&self) -> aura_runtime_contract::ActiveInstallState {
        aura_launcher::read_install_state(&self.root()).unwrap()
    }
}

#[test]
fn a_packed_setup_carries_one_release_that_extracts_whole() {
    let work = Work::new();
    let setup = work.release("0.1.0");
    let mut archive = payload::open_in_program(&setup).unwrap();
    assert_eq!(payload::version_of(&mut archive).unwrap(), "0.1.0");
    let out = work.folder.path().join("out");
    let written = payload::extract(&mut archive, &out, &mut |_, _| {}).unwrap();
    assert_eq!(written.len(), 7);
    assert_eq!(fs::read(out.join("versions/0.1.0/aura-setup.exe")).unwrap(), b"setup 0.1.0");
    assert_eq!(fs::read(out.join("versions/0.1.0/aura-core.exe")).unwrap(), b"core 0.1.0");
    assert_eq!(payload::program_without_release(&setup).unwrap(), fs::read(work.stub()).unwrap());

    let mut damaged = fs::read(&setup).unwrap();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xff;
    let damaged_path = work.folder.path().join("damaged.exe");
    fs::write(&damaged_path, damaged).unwrap();
    assert!(payload::open_in_program(&damaged_path).is_err());
}

#[test]
fn only_the_programs_the_manifest_names_go_to_the_root() {
    let work = Work::new();
    work.release("0.1.0");
    let payload = work.folder.path().join("payload-0.1.0");
    fs::write(payload.join("AuraLauncher.exe"), b"a launcher no manifest names").unwrap();
    fs::write(payload.join("Uninstall Aura.exe"), b"an uninstaller no manifest names").unwrap();
    let setup = work.folder.path().join("unsigned-root.exe");
    pack::pack(&payload, &work.stub(), &setup, &work.folder.path().join("unsigned-root.update"))
        .unwrap();
    work.install(&setup, None, false).unwrap();
    assert_eq!(fs::read(work.root().join("AuraLauncher.exe")).unwrap(), b"launcher 0.1.0");
    assert_eq!(fs::read(work.root().join("Uninstall Aura.exe")).unwrap(), b"setup 0.1.0");
}

#[test]
fn a_release_that_names_no_setup_program_keeps_the_one_installing_it() {
    let work = Work::new();
    work.release("0.1.0");
    let payload = work.folder.path().join("payload-0.1.0");
    let manifest_path = payload.join("versions/0.1.0/component-manifest.json");
    let mut manifest: BuildComponentManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.components.retain(|component| component.kind != BuildComponentKind::Setup);
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    let setup = work.folder.path().join("no-setup.exe");
    pack::pack(&payload, &work.stub(), &setup, &work.folder.path().join("no-setup.update"))
        .unwrap();
    work.install(&setup, None, false).unwrap();
    assert_eq!(
        fs::read(work.root().join("Uninstall Aura.exe")).unwrap(),
        fs::read(work.stub()).unwrap()
    );
}

#[test]
fn an_archive_naming_a_path_outside_its_folder_is_refused() {
    let work = Work::new();
    let zip_path = work.folder.path().join("evil.zip");
    let mut writer = ZipWriter::new(fs::File::create(&zip_path).unwrap());
    writer.start_file("../outside.txt", SimpleFileOptions::default()).unwrap();
    writer.write_all(b"escape").unwrap();
    writer.finish().unwrap();
    let mut archive = payload::open_package(&zip_path).unwrap();
    let out = work.folder.path().join("out");
    assert!(payload::extract(&mut archive, &out, &mut |_, _| {}).is_err());
    assert!(!work.folder.path().join("outside.txt").exists());
}

#[test]
fn a_fresh_install_puts_the_program_shortcuts_and_entry_in_place() {
    let work = Work::new();
    let setup = work.release("0.1.0");
    let plan = work.install(&setup, None, true).unwrap();
    assert_eq!(plan.kind, Kind::Fresh);
    let root = work.root();
    let state = work.state();
    assert_eq!(state.active, "0.1.0");
    assert_eq!(state.pending, None);
    assert_eq!(fs::read(root.join("AuraLauncher.exe")).unwrap(), b"launcher 0.1.0");
    assert_eq!(fs::read(root.join("Uninstall Aura.exe")).unwrap(), b"setup 0.1.0");
    aura_launcher::resolve_version(&root, "0.1.0", Some(&state.release_descriptor)).unwrap();
    let host = work.host();
    assert!(host.start_menu().unwrap().join("Aura Test setup.lnk").is_file());
    assert!(host.desktop().unwrap().join("Aura Test setup.lnk").is_file());
    let entry = host.entry(&namespace()).unwrap();
    assert_eq!(entry["DisplayVersion"], "0.1.0");
    assert_eq!(entry["InstallLocation"].as_str().map(PathBuf::from), Some(root.clone()));
    assert!(entry["UninstallString"].as_str().unwrap().contains("Uninstall Aura.exe"));
    let names: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|name| name.starts_with('.')), "staging left behind: {names:?}");
}

#[test]
fn an_upgrade_waits_as_a_trial_and_keeps_the_installed_version() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let before = work.state();
    let plan = work.install(&work.release("0.1.1"), None, false).unwrap();
    assert_eq!(plan.kind, Kind::Upgrade { from: "0.1.0".into() });
    let state = work.state();
    assert_eq!(state.active, "0.1.0");
    assert_eq!(state.release_descriptor, before.release_descriptor);
    assert_eq!(state.pending.as_deref(), Some("0.1.1"));
    assert!(work.root().join("versions/0.1.0").is_dir());
    assert!(work.root().join("versions/0.1.1").is_dir());
    assert!(!work.host().desktop().unwrap().join("Aura Test setup.lnk").exists());
}

#[test]
fn running_the_same_version_again_repairs_its_files() {
    let work = Work::new();
    let setup = work.release("0.1.0");
    work.install(&setup, None, false).unwrap();
    let core = work.root().join("versions/0.1.0/aura-core.exe");
    fs::write(&core, b"damaged").unwrap();
    let plan = work.install(&setup, None, false).unwrap();
    assert_eq!(plan.kind, Kind::Repair);
    assert_eq!(fs::read(&core).unwrap(), b"core 0.1.0");
    assert_eq!(work.state().active, "0.1.0");
}

#[test]
fn a_release_whose_files_do_not_match_its_manifest_leaves_the_installation_as_it_was() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let before = work.state();
    let payload = work.folder.path().join("payload-0.1.1");
    work.release("0.1.1");
    fs::write(payload.join("versions/0.1.1/aura-core.exe"), b"not what the manifest says").unwrap();
    let broken = work.folder.path().join("broken.exe");
    pack::pack(&payload, &work.stub(), &broken, &work.folder.path().join("broken.aura-update"))
        .unwrap();
    assert!(work.install(&broken, None, false).is_err());
    assert_eq!(work.state(), before);
    assert!(!work.root().join("versions/0.1.1").exists());
}

#[test]
fn a_new_installation_goes_only_into_an_empty_folder() {
    let work = Work::new();
    let chosen = work.folder.path().join("chosen");
    fs::create_dir_all(&chosen).unwrap();
    fs::write(chosen.join("someone else's.txt"), b"keep").unwrap();
    let error = work.install(&work.release("0.1.0"), Some(chosen.clone()), false).unwrap_err();
    assert!(error.to_string().contains("不是空文件夹"), "{error:#}");
    assert_eq!(fs::read(chosen.join("someone else's.txt")).unwrap(), b"keep");
}

#[test]
fn uninstalling_keeps_the_data_unless_asked_and_refuses_a_folder_that_is_not_aura() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, true).unwrap();
    let host = work.host();
    let data = host.data_root(&namespace()).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("aura-v2.db"), b"conversations").unwrap();

    let plan = plan_uninstall(&host, &work.root()).unwrap();
    assert_eq!(plan.version, "0.1.0");
    assert_eq!(plan.data_bytes, 13);
    let removed = uninstall(&host, &plan, false).unwrap();
    assert_eq!(removed.shortcuts, 2);
    assert!(removed.entry);
    assert!(!removed.data);
    assert!(!work.root().exists());
    assert!(host.entry(&namespace()).is_none());
    assert_eq!(fs::read(data.join("aura-v2.db")).unwrap(), b"conversations");

    work.install(&work.release("0.1.0"), None, false).unwrap();
    let plan = plan_uninstall(&host, &work.root()).unwrap();
    assert!(uninstall(&host, &plan, true).unwrap().data);
    assert!(!data.exists());

    let not_aura = work.folder.path().join("not-aura");
    fs::create_dir_all(&not_aura).unwrap();
    assert!(plan_uninstall(&host, &not_aura).is_err());
    assert!(not_aura.exists());
}

#[test]
fn uninstalling_removes_chromes_registration_and_folder_and_with_the_data_the_settings() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let host = work.host();
    let data = host.data_root(&namespace()).unwrap();
    let config = host.config_root(&namespace()).unwrap();
    let chrome = data.join("chrome");
    fs::create_dir_all(chrome.join("extension")).unwrap();
    let manifest = chrome.join("host.json");
    fs::write(&manifest, b"{}").unwrap();
    fs::write(chrome.join("extension/manifest.json"), b"{}").unwrap();
    fs::write(data.join("aura-v2.db"), b"conversations").unwrap();
    fs::create_dir_all(config.join("settings")).unwrap();
    fs::write(config.join("settings/chrome.toml"), b"enabled = true\n").unwrap();
    let own = native_host_name(NAMESPACE);
    let other = native_host_name("aura.desktop.test.other");
    host.register_chrome_host(&own, &manifest);
    host.register_chrome_host(&other, &manifest);

    let plan = plan_uninstall(&host, &work.root()).unwrap();
    assert_eq!(plan.config_bytes, 15);
    let removed = uninstall(&host, &plan, false).unwrap();
    assert!(removed.chrome);
    assert_eq!(host.chrome_host(&own), None);
    assert_eq!(host.chrome_host(&other), Some(manifest.clone()));
    assert!(!chrome.exists(), "Chrome's manifest and the extension copy go with the program");
    assert_eq!(fs::read(data.join("aura-v2.db")).unwrap(), b"conversations");
    assert!(config.join("settings/chrome.toml").is_file());

    work.install(&work.release("0.1.0"), None, false).unwrap();
    let plan = plan_uninstall(&host, &work.root()).unwrap();
    let removed = uninstall(&host, &plan, true).unwrap();
    assert!(!removed.chrome);
    assert!(removed.data);
    assert!(!data.exists());
    assert!(!config.exists(), "the settings are part of the data the user chose to delete");
}

#[test]
fn chromes_pairing_secret_is_among_the_credentials_deleting_the_data_removes() {
    let reference = pairing_credential(
        &namespace().object_name_hash(),
        aura_domain_common::CredentialId::new(),
    )
    .unwrap();
    assert!(reference.windows_target_name().starts_with(&credential_prefix(&namespace())));
}

#[test]
fn uninstalling_waits_for_a_program_file_held_open_for_a_moment() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let host = work.host();
    let program = work.root().join("versions/0.1.0/aura-core.exe");
    let held =
        fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&program).unwrap();
    let releasing = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        drop(held);
    });
    let plan = plan_uninstall(&host, &work.root()).unwrap();
    let removed = uninstall(&host, &plan, false);
    releasing.join().unwrap();
    removed.unwrap();
    assert!(!work.root().exists());
}

/// Runs a copy of a system program from `folder` until the guard drops, so the folder has a
/// running program the way an installation has a running Aura.
struct Running(std::process::Child);

impl Running {
    fn start(folder: &Path) -> Self {
        let program = folder.join("aura-running.exe");
        fs::copy(r"C:\Windows\System32\PING.EXE", &program).unwrap();
        Self(std::process::Command::new(program).args(["-n", "60", "127.0.0.1"]).spawn().unwrap())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn an_update_applies_while_aura_runs_and_waits_as_a_trial() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    work.release("0.1.1");
    let package = work.folder.path().join("Aura-Update-0.1.1.aura-update");
    let running = Running::start(&work.root());
    assert!(
        work.install(&work.folder.path().join("Aura-Setup-0.1.1.exe"), None, false).is_err(),
        "a plain install went ahead while Aura ran"
    );
    let release =
        install::apply_update(&work.host(), &work.stub(), &work.root(), &package).unwrap();
    drop(running);
    assert_eq!(release.version, "0.1.1");
    let state = work.state();
    assert_eq!(state.active, "0.1.0");
    assert_eq!(state.pending.as_deref(), Some("0.1.1"));
    aura_launcher::resolve_version(&work.root(), "0.1.1", None).unwrap();
    assert_eq!(work.host().entry(&namespace()).unwrap()["DisplayVersion"], "0.1.1");
    assert_eq!(fs::read(work.root().join("AuraLauncher.exe")).unwrap(), b"launcher 0.1.1");
    assert_eq!(fs::read(work.root().join("Uninstall Aura.exe")).unwrap(), b"setup 0.1.1");
}

#[test]
fn an_update_that_is_not_newer_or_of_another_installation_is_refused() {
    let work = Work::new();
    work.install(&work.release("0.1.1"), None, false).unwrap();
    work.release("0.1.0");
    let older = work.folder.path().join("Aura-Update-0.1.0.aura-update");
    let same = work.folder.path().join("Aura-Update-0.1.1.aura-update");
    for package in [&older, &same] {
        assert!(install::apply_update(&work.host(), &work.stub(), &work.root(), package).is_err());
    }
    assert_eq!(work.state().pending, None);
}

#[test]
fn going_back_lets_the_previous_version_wait_and_needs_one() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    assert!(install::roll_back(&work.root()).is_err(), "went back with no previous version");
    let package = {
        work.release("0.1.1");
        work.folder.path().join("Aura-Update-0.1.1.aura-update")
    };
    install::apply_update(&work.host(), &work.stub(), &work.root(), &package).unwrap();
    // The launcher promoted the trial: 0.1.1 is active, 0.1.0 the previous version.
    let mut state = work.state();
    let next = aura_launcher::resolve_version(&work.root(), "0.1.1", None).unwrap();
    state = aura_launcher::promote(&state, "0.1.1", next.descriptor);
    aura_launcher::write_install_state(&work.root(), &state).unwrap();
    assert_eq!(install::roll_back(&work.root()).unwrap(), "0.1.0");
    let back = work.state();
    assert_eq!(back.active, "0.1.1");
    assert_eq!(back.pending.as_deref(), Some("0.1.0"));
}
