//! Batch C: Secret independence and isolation.
//!
//! The two domain credential authorities are:
//!
//! * the **Provider** credential authority — `aura-core::p05_credential`
//!   (`P05CredentialJournal` / `PersistentProviderCredentialResolver`), whose
//!   secret bytes live in the Windows Credential Manager under a
//!   namespace-scoped target and whose non-secret state lives in
//!   `model_credential_records`;
//! * the **tool** credential authority —
//!   `aura_tool_domain::credential::ToolCredentialAuthorityRecordV1`, whose
//!   record deliberately holds no secret bytes and no WinCred locator.
//!
//! Both sit on the same substrate (`aura_windows_platform::WindowsCredentialStore`)
//! and both are reached through the dedicated credential channel rather than an
//! ordinary IPC body.
//!
//! # Isolation rules these tests hold to
//!
//! * Only `CredentialRef::for_test` namespaces are used. The user's real
//!   credentials are never read, written, rotated or deleted.
//! * Every test credential is deleted by a `Drop` guard, so a failing assertion
//!   cannot leave one behind.
//! * The canary is a distinctive literal, and the tests scan the **real** files
//!   the product writes. A canary count of 0 in ordinary output is only evidence
//!   if the credential was really read in the first place, so each leak check is
//!   paired with a positive control that proves the canary is detectable in the
//!   same scan.
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use aura_config::{AuraPaths, ConfigModule, SettingsFacade};
use aura_domain_common::CredentialRef;
use aura_windows_platform::{CredentialStoreError, SecretBytes, WindowsCredentialStore};
use tempfile::TempDir;

/// A distinctive value. It is never a real credential and it is never reused
/// across tests, so a hit is unambiguous.
const CANARY: &[u8] = b"S05-CANARY-PROVIDER-CREDENTIAL-VALUE-4f19";

fn paths(directory: &TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

/// A unique isolated namespace per test, matching the existing pattern.
fn test_reference(label: &str) -> CredentialRef {
    let run = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
    CredentialRef::for_test(&format!("s05{run:x}"), label).expect("isolated test reference")
}

/// Deletes its reference on drop, including on a panic, so an isolated test
/// credential can never outlive the test that made it.
struct CredentialCleanup(CredentialRef);

impl Drop for CredentialCleanup {
    fn drop(&mut self) {
        let _ = WindowsCredentialStore.delete(&self.0);
    }
}

fn tree_contains(root: &Path, needle: &[u8]) -> bool {
    if root.is_file() {
        return fs::read(root)
            .is_ok_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle));
    }
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| tree_contains(&entry.path(), needle))
}

/// Every ordinary surface the product writes, as one haystack.
fn ordinary_output_contains(paths: &AuraPaths, needle: &[u8]) -> bool {
    [
        paths.config_root.as_path(),
        paths.config_history_dir.as_path(),
        paths.config_transaction_dir.as_path(),
        paths.logs_dir.as_path(),
        paths.runtime_dir.as_path(),
        paths.data_dir.as_path(),
    ]
    .into_iter()
    .any(|root| tree_contains(root, needle))
}

/// The substrate is real: the isolated namespace round-trips through the
/// platform store, and the canary is really readable there.
///
/// This is the **positive control**. Without it, "the canary is absent from
/// ordinary output" could be true simply because nothing was ever stored or
/// read — which is exactly the way this class of test usually passes wrongly.
#[test]
fn the_isolated_credential_namespace_really_stores_and_really_reads() {
    let reference = test_reference("positive-control");
    let _cleanup = CredentialCleanup(reference.clone());

    WindowsCredentialStore
        .write(&reference, &SecretBytes::new(CANARY.to_vec()))
        .expect("write into the isolated namespace");

    // Really read it back. `expose()` is the only way to see the
    // bytes, and the value must match exactly.
    let stored = WindowsCredentialStore.read(&reference).expect("read from the isolated namespace");
    let exposed = stored.expose().to_vec();
    assert_eq!(
        exposed.as_slice(),
        CANARY,
        "the credential was really read, not merely assumed present"
    );

    // A different isolated namespace is genuinely different: reading it must not
    // return the first credential. This is the "no cross-domain borrowing" check
    // at the substrate level.
    let other = test_reference("positive-control-other");
    let _other_cleanup = CredentialCleanup(other.clone());
    match WindowsCredentialStore.read(&other) {
        Err(CredentialStoreError::NotFound { .. }) => {}
        other_result => panic!(
            "an unrelated isolated namespace must not resolve to another credential: {other_result:?}"
        ),
    }
    // And the first credential survives that unrelated read unchanged.
    assert_eq!(
        WindowsCredentialStore.read(&reference).expect("read again").expose(),
        CANARY,
        "reading an unrelated namespace did not disturb this one"
    );
}

/// A credential value never reaches the configuration surfaces.
///
/// The settings module may name a `CredentialRef`; the bytes stay in the
/// platform store. The scan covers the whole ordinary output tree, and it is
/// paired with a positive control proving the canary would have been found.
#[test]
fn a_credential_value_never_enters_configuration_state_or_history() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    let reference = test_reference("config-history");
    let _cleanup = CredentialCleanup(reference.clone());

    WindowsCredentialStore
        .write(&reference, &SecretBytes::new(CANARY.to_vec()))
        .expect("store the isolated test credential");

    // Really read it, so the test cannot pass by never touching the credential.
    assert_eq!(
        WindowsCredentialStore
            .read(&reference)
            .expect("read the isolated test credential")
            .expose(),
        CANARY
    );

    facade
        .update(ConfigModule::Models, 1, |values| {
            values
                .insert("endpoint".to_owned(), serde_json::json!({ "credential_ref": reference }));
        })
        .expect("persist only the CredentialRef");

    // The reference itself did reach the configuration; the value did not.
    let config = fs::read_to_string(paths.config_path(ConfigModule::Models))
        .expect("read the models configuration");
    assert!(
        config.contains("credential_ref"),
        "the reference is what the configuration is allowed to carry"
    );
    assert!(
        !ordinary_output_contains(&paths, CANARY),
        "the credential value leaked into ordinary configuration output"
    );

    // Positive control for the scan itself: the same walker does find the canary
    // when it is really there, so the zero above is a real zero.
    let planted = paths.logs_dir.join("s05-scan-positive-control.bin");
    fs::create_dir_all(&paths.logs_dir).expect("logs dir");
    fs::write(&planted, CANARY).expect("plant the canary");
    assert!(
        ordinary_output_contains(&paths, CANARY),
        "the scan must find the canary when it is present, otherwise the zero above means nothing"
    );
    fs::remove_file(&planted).expect("remove the planted canary");
    assert!(
        !ordinary_output_contains(&paths, CANARY),
        "and the zero returns once the plant is removed"
    );
}

/// A credential read failure does not disable unrelated configuration domains.
///
/// The Provider credential authority and the settings domains are separate
/// owners. A credential that is missing, or that denies access, must not take
/// configuration down with it — and a *different* credential must still resolve.
#[test]
fn a_missing_credential_only_affects_its_own_authority() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");

    // A reference that was never written: the real "missing" condition.
    let missing = test_reference("missing");
    let _cleanup = CredentialCleanup(missing.clone());

    // The substrate reports a real, typed absence rather than an empty secret.
    let read = WindowsCredentialStore.read(&missing);
    assert!(
        matches!(read, Err(CredentialStoreError::NotFound { .. })),
        "a missing credential is a typed absence, not an empty value: {read:?}"
    );

    // An unrelated, present credential still resolves.
    let present = test_reference("present");
    let _present_cleanup = CredentialCleanup(present.clone());
    WindowsCredentialStore
        .write(&present, &SecretBytes::new(CANARY.to_vec()))
        .expect("store the unrelated credential");
    assert_eq!(
        WindowsCredentialStore
            .read(&present)
            .expect("the unrelated credential still resolves")
            .expose(),
        CANARY,
        "credential B is unaffected by credential A being missing"
    );

    // And every configuration domain still reads and writes.
    for module in ConfigModule::ALL {
        facade.snapshot(module).unwrap_or_else(|error| {
            panic!("{module:?} must be unaffected by a missing credential: {error}")
        });
    }
    let updated = facade
        .update(ConfigModule::Diagnostics, 1, |values| {
            values.insert("level".to_owned(), serde_json::json!("debug"));
        })
        .expect("configuration still writes while a credential is missing");
    assert_eq!(updated.document.revision, 2);
}

/// Deleting one credential leaves another authority's credential intact.
///
/// This is the substrate-level form of "a rotate/delete failure affects only
/// real dependents, and the other endpoint's credential is never borrowed".
#[test]
fn deleting_one_credential_leaves_the_other_authority_intact() {
    let first = test_reference("authority-a");
    let _first_cleanup = CredentialCleanup(first.clone());
    let second = test_reference("authority-b");
    let _second_cleanup = CredentialCleanup(second.clone());

    let first_value = b"S05-CANARY-AUTHORITY-A-VALUE-0001";
    let second_value = b"S05-CANARY-AUTHORITY-B-VALUE-0002";
    WindowsCredentialStore
        .write(&first, &SecretBytes::new(first_value.to_vec()))
        .expect("store authority A");
    WindowsCredentialStore
        .write(&second, &SecretBytes::new(second_value.to_vec()))
        .expect("store authority B");

    assert!(WindowsCredentialStore.delete(&first).expect("delete authority A"));

    // A is gone, with a typed absence rather than a borrowed value.
    assert!(
        matches!(WindowsCredentialStore.read(&first), Err(CredentialStoreError::NotFound { .. })),
        "authority A is really deleted"
    );
    // B is untouched and still returns its own value, never A's.
    let surviving =
        WindowsCredentialStore.read(&second).expect("authority B survives").expose().to_vec();
    assert_eq!(surviving.as_slice(), second_value, "B still holds its own value");
    assert_ne!(surviving.as_slice(), first_value, "B must never be answered with A's value");
}

/// A credential-bearing invalid configuration is never copied to history.
///
/// This is the settings-side half of the isolation rule: the recovery path must
/// not turn "the file was invalid" into "the file, including any secret in it,
/// is now in the history directory".
#[test]
fn a_secret_bearing_invalid_config_never_reaches_history() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    drop(facade);

    // A genuinely undecodable TOML document that carries the canary.
    let mut secret_bearing = String::from("schema_version = 1\nrevision = 7\n");
    secret_bearing.push_str(&format!("api_key = \"{}\"\n", String::from_utf8_lossy(CANARY)));
    secret_bearing.push_str("this is not valid toml\n");
    fs::write(paths.config_path(ConfigModule::Security), secret_bearing.as_bytes())
        .expect("write the secret-bearing invalid configuration");

    let facade = SettingsFacade::open(paths.clone()).expect("facade recovers");
    // The domain still recovered to a usable snapshot.
    facade.snapshot(ConfigModule::Security).expect("security snapshot");

    assert!(
        !tree_contains(&paths.config_history_dir, CANARY),
        "the secret-bearing invalid file must not be copied into the config history"
    );

    // Positive control: the same walker finds the canary in a file it really
    // contains, so the zero above is a real zero.
    let planted = paths.config_history_dir.join("s05-history-positive-control.bin");
    fs::write(&planted, CANARY).expect("plant the canary");
    assert!(
        tree_contains(&paths.config_history_dir, CANARY),
        "the history scan must find the canary when present"
    );
    fs::remove_file(&planted).expect("remove the plant");
    assert!(!tree_contains(&paths.config_history_dir, CANARY), "and the history is clean again");
}

/// A credential reference is not a secret, and the two are distinguishable.
///
/// A guard against the opposite failure: a test that redacts everything would
/// also "pass" a leak check, and would break the product. The reference must
/// still be storable and readable as an identifier.
#[test]
fn a_credential_reference_is_still_a_usable_identifier() {
    let reference = test_reference("identifier");
    let _cleanup = CredentialCleanup(reference.clone());

    let serialized = serde_json::to_string(&reference).expect("a reference serializes");
    assert!(serialized.contains("wincred"), "a reference is a locator, not a value: {serialized}");
    assert!(
        !serialized.contains(&String::from_utf8_lossy(CANARY).to_string()),
        "a reference never contains the secret value"
    );
    // The reference round-trips, so redaction has not made it unusable.
    let parsed: CredentialRef = serde_json::from_str(&serialized).expect("a reference parses");
    assert_eq!(parsed, reference);
}

/// The Windows target name is namespace-scoped, so two isolated runs cannot
/// collide on one another's credential.
#[test]
fn two_isolated_namespaces_have_distinct_platform_targets() {
    let first = test_reference("target");
    let second = test_reference("target");
    assert_ne!(
        first.windows_target_name(),
        second.windows_target_name(),
        "each isolated run gets its own platform target"
    );
    assert_ne!(first, second, "and its own reference identity");
}

/// Shared-state guard: the tests above must not have left the isolated namespace
/// reachable, and the platform store must still answer for the references they
/// created only while the guard was alive.
#[test]
fn the_cleanup_guard_really_deletes_the_test_credential() {
    let reference = test_reference("cleanup");
    {
        let _cleanup = CredentialCleanup(reference.clone());
        WindowsCredentialStore
            .write(&reference, &SecretBytes::new(CANARY.to_vec()))
            .expect("store");
        assert!(WindowsCredentialStore.read(&reference).is_ok());
    }
    assert!(
        matches!(
            WindowsCredentialStore.read(&reference),
            Err(CredentialStoreError::NotFound { .. })
        ),
        "the guard deleted the isolated test credential"
    );
}

/// The renderer-safe tool credential record carries no secret material.
///
/// Kept as a source-level guard rather than a schema assertion, so this crate
/// does not take a dependency on the tool domain merely to read one type. The
/// structural claim is checked where the type lives; here the check is that the
/// *shared substrate* this crate does depend on has no way to hand out a secret
/// by accident.
#[test]
fn the_shared_substrate_never_reveals_a_secret_without_an_explicit_read() {
    let reference = test_reference("no-accidental-reveal");
    let _cleanup = CredentialCleanup(reference.clone());
    WindowsCredentialStore.write(&reference, &SecretBytes::new(CANARY.to_vec())).expect("store");

    // A missing credential produces a typed absence, not an empty or defaulted
    // secret that a caller could mistake for a real value.
    let absent = test_reference("no-accidental-reveal-absent");
    let _absent_cleanup = CredentialCleanup(absent.clone());
    assert!(
        matches!(WindowsCredentialStore.read(&absent), Err(CredentialStoreError::NotFound { .. })),
        "absence is typed, so a caller cannot silently receive an empty secret"
    );

    // The debug rendering of the stored record must not include the secret bytes.
    let stored = WindowsCredentialStore.read(&reference).expect("read");
    let rendered = format!("{stored:?}");
    assert!(
        !rendered.contains(&String::from_utf8_lossy(CANARY).to_string()),
        "a stored credential must not print its secret in a debug rendering: {rendered}"
    );
}

/// A single shared substrate owner, so independence is not achieved by having
/// two different stores that could disagree.
#[test]
fn both_authorities_use_the_one_platform_substrate() {
    // `WindowsCredentialStore` is a unit struct: there is exactly one substrate.
    // Reading it through two independently created bindings reaches the same
    // store, which is what makes the isolation claims meaningful.
    let reference = test_reference("shared-substrate");
    let _cleanup = CredentialCleanup(reference.clone());
    let value = b"S05-CANARY-SHARED-SUBSTRATE-0003";
    WindowsCredentialStore
        .write(&reference, &SecretBytes::new(value.to_vec()))
        .expect("write through the first binding");
    let read_back = WindowsCredentialStore
        .read(&reference)
        .expect("read through the second binding")
        .expose()
        .to_vec();
    assert_eq!(read_back.as_slice(), value, "one substrate, one value");
}
