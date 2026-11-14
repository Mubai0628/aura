//! A process given a data folder works it out again and refuses a different one; the installation
//! folder's copy is found from the manifest's place.
use crate::support;

use std::fs;

use aura_data_location::{ConfirmError, InstallCopy, Locations, confirm, route_core_log};
use aura_domain_common::{ContentHash, ContentHashAlgorithm, ReleaseBundleId};
use aura_runtime_contract::{ActiveInstallState, ReleaseDescriptor, ReleaseIdentity};
use support::{Profile, production, test_namespace};

#[test]
fn the_same_data_folder_is_confirmed() {
    let profile = Profile::new();
    let moved = profile.folder("Moved");
    profile.record_move(&profile.roots.local, &test_namespace(), &moved);
    let confirmed =
        confirm(&profile.roots, &test_namespace(), &InstallCopy::NotInstalled, &moved.join("data"))
            .expect("confirmed");
    assert_eq!(confirmed.data_root, moved.join("data"));
}

#[test]
fn a_different_data_folder_is_refused() {
    let profile = Profile::new();
    let moved = profile.folder("Moved");
    profile.record_move(&profile.roots.local, &test_namespace(), &moved);
    let default =
        profile.roots.local.join("Aura/namespaces").join(test_namespace().object_name_hash());
    let error = confirm(&profile.roots, &test_namespace(), &InstallCopy::NotInstalled, &default)
        .expect_err("refused");
    assert!(
        matches!(&error, ConfirmError::Mismatch { passed, resolved } if *passed == default && *resolved == moved.join("data")),
        "{error}"
    );
}

#[test]
fn a_record_that_cannot_be_used_is_refused_too() {
    let profile = Profile::new();
    let record = profile.record_path(&test_namespace());
    fs::create_dir_all(record.parent().expect("folder")).expect("folder");
    fs::write(&record, b"[]").expect("broken record");
    let error = confirm(
        &profile.roots,
        &test_namespace(),
        &InstallCopy::NotInstalled,
        &profile.roots.local,
    )
    .expect_err("refused");
    assert!(matches!(error, ConfirmError::Location(_)), "{error}");
}

#[test]
fn the_launcher_passes_the_desktop_its_three_locations() {
    let profile = Profile::new();
    let locations = Locations::resolve(&profile.roots, &production(), &InstallCopy::NotInstalled)
        .expect("locations");
    assert_eq!(
        locations.desktop_arguments(),
        [
            format!("--aura-data-root={}", locations.data_root.display()),
            format!("--aura-ui-profile={}", locations.ui_profile.display()),
            format!("--aura-chrome-root={}", locations.chrome_root.display()),
        ]
    );
    assert_eq!(
        locations.data_root_argument(),
        format!("--data-root={}", locations.data_root.display())
    );
}

fn active(data_location: Option<&str>) -> ActiveInstallState {
    ActiveInstallState {
        active: "1.0.0".into(),
        release_descriptor: ReleaseDescriptor {
            bundle_namespace: production(),
            identity: ReleaseIdentity {
                release_bundle_id: ReleaseBundleId::new(),
                component_manifest_hash: ContentHash::new(
                    ContentHashAlgorithm::Sha256,
                    "0".repeat(64),
                )
                .expect("hash"),
            },
        },
        previous: None,
        pending: None,
        last_known_good: None,
        data_location: data_location.map(str::to_owned),
    }
}

#[test]
fn the_installation_copy_is_read_beside_the_versions_folder() {
    let profile = Profile::new();
    let install = profile.folder("Programs/Aura");
    let version = install.join("versions/1.0.0");
    fs::create_dir_all(&version).expect("version folder");
    let manifest = version.join("component-manifest.json");
    fs::write(
        install.join("active.json"),
        serde_json::to_vec(&active(Some("D:\\Aura"))).expect("json"),
    )
    .expect("active.json");
    assert_eq!(
        InstallCopy::for_manifest(&manifest).expect("copy"),
        InstallCopy::Installed(Some("D:\\Aura".into()))
    );
    fs::write(install.join("active.json"), serde_json::to_vec(&active(None)).expect("json"))
        .expect("active.json");
    assert_eq!(InstallCopy::for_manifest(&manifest).expect("copy"), InstallCopy::Installed(None));
    // A manifest that is not in an installation's versions folder (a development run).
    assert_eq!(
        InstallCopy::for_manifest(
            &profile.folder("target/aura-dev-runtime").join("component-manifest.json")
        )
        .expect("copy"),
        InstallCopy::NotInstalled
    );
}

#[test]
fn an_active_json_without_a_move_is_written_as_before() {
    let written = serde_json::to_value(active(None)).expect("json");
    assert!(written.get("data_location").is_none(), "{written}");
}

#[test]
fn the_core_log_route_is_set_once() {
    let profile = Profile::new();
    let first = profile.folder("logs").join("core.jsonl");
    route_core_log(first.clone()).expect("first route");
    route_core_log(first.clone()).expect("the same route again");
    assert_eq!(route_core_log(profile.folder("other").join("core.jsonl")), Err(first.clone()));
    assert_eq!(aura_data_location::core_log_path(), Some(first.as_path()));
}
