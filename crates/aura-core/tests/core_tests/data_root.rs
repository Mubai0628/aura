//! Core takes its data folder from Main (`--data-root=`) and works it out again itself: one that is
//! not where this installation's data is makes it refuse to start, before it reads Main's secret,
//! opens a store or creates a folder. Asked `--data-locations`, it tells the desktop the locations.
#![allow(clippy::unwrap_used)]
use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use aura_domain_common::RuntimeInstanceId;
use aura_runtime_contract::BundleNamespace;
use serde_json::Value;

struct Profile {
    folder: tempfile::TempDir,
    manifest: PathBuf,
    namespace: BundleNamespace,
}

impl Profile {
    fn new() -> Self {
        let folder = tempfile::tempdir().unwrap();
        let bytes = include_bytes!("../../../../resources/component-manifest.example.json");
        let manifest = folder.path().join("component-manifest.json");
        std::fs::write(&manifest, bytes).unwrap();
        let parsed: Value = serde_json::from_slice(bytes).unwrap();
        let namespace = parsed["bundle_namespace"].as_str().unwrap().parse().unwrap();
        Self { folder, manifest, namespace }
    }

    fn local(&self) -> PathBuf {
        self.folder.path().join("local")
    }

    fn core(&self) -> Command {
        let mut core = Command::new(env!("CARGO_BIN_EXE_aura-core"));
        core.env("LOCALAPPDATA", self.local()).env("APPDATA", self.folder.path().join("roaming"));
        core
    }

    fn start(&self, data_root: &Path) -> Output {
        let runtime = RuntimeInstanceId::new();
        let pipe = format!(
            r"\\.\pipe\Aura\{}\{runtime}\{}",
            self.namespace.object_name_hash(),
            "0123456789abcdef0123456789abcdef"
        );
        self.core()
            .args([
                format!("--pipe-name={pipe}"),
                "--bootstrap-stdin".to_owned(),
                format!("--parent-pid={}", std::process::id()),
                format!("--runtime-instance-id={runtime}"),
                format!("--component-manifest={}", self.manifest.display()),
                "--allow-dev-component-mismatch".to_owned(),
                format!("--data-root={}", data_root.display()),
            ])
            .stdin(Stdio::piped())
            .output()
            .unwrap()
    }
}

#[test]
fn core_refuses_a_data_folder_that_is_not_its_installations() {
    let profile = Profile::new();
    let elsewhere = profile.folder.path().join("elsewhere");
    let output = profile.start(&elsewhere);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(&elsewhere.display().to_string()), "{stderr}");
    assert!(!profile.local().join("Aura").exists(), "Core created data before it refused");
    assert!(!elsewhere.exists());
}

#[test]
fn core_tells_the_desktop_its_locations() {
    let profile = Profile::new();
    let output = profile
        .core()
        .args([
            "--data-locations".to_owned(),
            format!("--component-manifest={}", profile.manifest.display()),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    let answer: Value = serde_json::from_str(stdout.trim()).unwrap();
    let data_root =
        profile.local().join("Aura").join("namespaces").join(profile.namespace.object_name_hash());
    assert_eq!(answer["locations"]["data_root"], data_root.display().to_string());
    assert_eq!(
        answer["locations"]["ui_profile"],
        data_root.join("ui-profile").display().to_string()
    );
    assert_eq!(answer["locations"]["chrome_root"], data_root.join("chrome").display().to_string());
    assert!(!profile.local().join("Aura").exists(), "answering created a folder");
}
