//! The release key tooling (`scripts/release`) issues what the release checks accept: a root with
//! the frozen production profile, and a signer whose manifest signatures verify against that root
//! and no other.
#![allow(clippy::unwrap_used)]
#[path = "../src/release_root_profile.rs"]
mod release_root_profile;

use aura_domain_common::{ContentHash, ContentHashAlgorithm};
use aura_runtime_contract::{BuildComponentManifest, ReleaseDescriptor, ReleaseIdentity};
use aura_windows_platform::verify_release_manifest_against_root;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const MANIFEST: &[u8] = include_bytes!("fixtures/release-trust/component-manifest.json");
const OTHER_ROOT: &[u8] = include_bytes!("fixtures/release-trust/test-root.der");

fn script(arguments: &[&str]) -> Output {
    let scripts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/release");
    Command::new("node")
        .arg(scripts.join(arguments[0]))
        .args(&arguments[1..])
        .env("AURA_RELEASE_ROOT_PASSPHRASE", "test-root-passphrase-1")
        .env("AURA_RELEASE_SIGNING_PASSPHRASE", "test-signing-passphrase-1")
        .output()
        .unwrap()
}

fn run(arguments: &[&str]) {
    let output = script(arguments);
    assert!(
        output.status.success(),
        "{arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn descriptor(bytes: &[u8]) -> ReleaseDescriptor {
    let manifest: BuildComponentManifest = serde_json::from_slice(bytes).unwrap();
    ReleaseDescriptor {
        bundle_namespace: manifest.bundle_namespace,
        identity: ReleaseIdentity {
            release_bundle_id: manifest.release_bundle_id,
            component_manifest_hash: ContentHash::new(
                ContentHashAlgorithm::Sha256,
                format!("{:x}", Sha256::digest(bytes)),
            )
            .unwrap(),
        },
    }
}

/// A version folder holding `MANIFEST`, signed with the test key in `keys`.
fn signed_folder(keys: &Path, name: &str) -> (PathBuf, Vec<u8>) {
    let folder = keys.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("component-manifest.json"), MANIFEST).unwrap();
    run(&["sign-manifest.mjs", folder.to_str().unwrap(), "--test-dir", keys.to_str().unwrap()]);
    let signature = fs::read(folder.join("component-manifest.p7s")).unwrap();
    (folder, signature)
}

#[test]
fn the_tooling_issues_a_root_aura_builds_with_and_a_signer_only_that_root_accepts() {
    let keys = tempfile::tempdir().unwrap();
    run(&["release-key.mjs", "init", "--test-dir", keys.path().to_str().unwrap()]);
    let root = fs::read(keys.path().join("aura-production-release-root-v1.der")).unwrap();
    release_root_profile::validate_production_root(&root).unwrap();

    let (_, signature) = signed_folder(keys.path(), "0.1.0");
    verify_release_manifest_against_root(MANIFEST, &signature, &root, &descriptor(MANIFEST))
        .unwrap();
    assert!(
        verify_release_manifest_against_root(
            MANIFEST,
            &signature,
            OTHER_ROOT,
            &descriptor(MANIFEST)
        )
        .is_err(),
        "another root accepted the signature"
    );
    let mut changed = MANIFEST.to_vec();
    changed.extend_from_slice(b"\n");
    assert!(
        verify_release_manifest_against_root(&changed, &signature, &root, &descriptor(&changed))
            .is_err(),
        "a changed manifest kept its signature"
    );
}

#[test]
fn a_renewed_signer_signs_for_the_same_root_and_init_never_replaces_a_key() {
    let keys = tempfile::tempdir().unwrap();
    let test_dir = keys.path().to_str().unwrap();
    run(&["release-key.mjs", "init", "--test-dir", test_dir]);
    let root = fs::read(keys.path().join("aura-production-release-root-v1.der")).unwrap();
    let first = fs::read(keys.path().join("signing/aura-production-release-signing.pem")).unwrap();

    run(&["release-key.mjs", "renew-signing", "--test-dir", test_dir]);
    let renewed =
        fs::read(keys.path().join("signing/aura-production-release-signing.pem")).unwrap();
    assert_ne!(first, renewed, "renewing kept the old certificate");
    let (_, signature) = signed_folder(keys.path(), "0.1.1");
    verify_release_manifest_against_root(MANIFEST, &signature, &root, &descriptor(MANIFEST))
        .unwrap();

    let again = script(&["release-key.mjs", "init", "--test-dir", test_dir]);
    assert!(!again.status.success(), "init replaced an existing key");
    assert!(String::from_utf8_lossy(&again.stderr).contains("refusing to replace"));
    assert_eq!(fs::read(keys.path().join("aura-production-release-root-v1.der")).unwrap(), root);
}
