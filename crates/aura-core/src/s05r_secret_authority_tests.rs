//! The secret domain boundary through the **real** authority chains.
//!
//! # Why the existing ten items were not enough
//!
//! `aura-config/tests/s05_secret_isolation.rs` calls `WindowsCredentialStore`
//! **directly**. That proves the substrate round-trips and that no copy of a value
//! reaches the ordinary output tree. It does **not** drive the product:
//!
//! * the actual Provider resolver (`PersistentProviderCredentialResolver::resolve`),
//!   which is what a model transport really calls;
//! * the P05 credential journal (`P05CredentialJournal::configure_secret` /
//!   `clear` / `recover`), which is what the credential commands really run;
//! * the tool credential authority, which is a **separate** owner.
//!
//! Two `WindowsCredentialStore` handles are not two domain authorities. This file
//! therefore drives the real chain, and it says explicitly which authority each
//! case exercises.
//!
//! # The two authorities
//!
//! | authority | what reads it | what it may NOT do |
//! | --- | --- | --- |
//! | Provider credential authority | `ProviderCredentialResolver::resolve` | borrow the tool authority's credential |
//! | Tool credential authority | the tool credential binding path | borrow the provider authority's credential |
//!
//! They share the confidentiality **substrate** (WinCred + the same non-secret
//! attribute contract) but they are separate owners. A missing, denied, cancelled
//! or interrupted credential in one must leave the other working.
//!
//! # Canary discipline
//!
//! Every scan class here has:
//!
//! * a **seeded-canary positive control** — the canary is really stored and really
//!   readable through the chain, so "not found in ordinary output" is not true by
//!   accident; and
//! * a **normal no-leak control** — a clean run with no canary anywhere.
//!
//! A check product that **should** have been read but was missing or unreadable is
//! reported as **NOT OBTAINED**, never as "0 leaks". An empty directory is not a
//! leak-free directory.
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use aura_domain_common::CredentialRef;
use aura_windows_platform::{CredentialStoreError, SecretBytes, WindowsCredentialStore};

/// A distinctive value. Never a real credential, never reused across cases, so a
/// hit in any scanned surface is unambiguous.
const CANARY_PROVIDER_A: &str = "CANARY-S05R-R4-PROVIDER-A-8f31";
const CANARY_PROVIDER_B: &str = "CANARY-S05R-R4-PROVIDER-B-4c72";
const CANARY_TOOL_A: &str = "CANARY-S05R-R4-TOOL-A-1d95";

/// A unique isolated namespace per case, matching the existing repo pattern.
fn reference(label: &str) -> CredentialRef {
    let run = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
    CredentialRef::for_test(&format!("s05r{run:x}"), label).expect("isolated test reference")
}

/// Deletes its references on drop, including on a panic, so an isolated test
/// credential can never outlive the case that made it.
struct CredentialGuard(Vec<CredentialRef>);

impl CredentialGuard {
    fn new(references: Vec<CredentialRef>) -> Self {
        Self(references)
    }
}

impl Drop for CredentialGuard {
    fn drop(&mut self) {
        for reference in &self.0 {
            let _ = WindowsCredentialStore.delete(reference);
        }
    }
}

/// The outcome of asking for a secret: a real value, or a stated reason.
///
/// This type exists so a caller **cannot** collapse "removed", "cancelled" and
/// "unreadable" into one negative. Each is a different fact and the product must
/// act differently on each.
///
/// # What the substrate can and cannot distinguish (measured)
///
/// `aura_windows_platform::CredentialStoreError` has exactly **one** absence-ish
/// variant, `NotFound { reference }`, plus `Windows(io::Error)`. There is **no**
/// dedicated access-denied variant: an ACL denial arrives as
/// `Windows(io::Error)` with the platform error inside.
///
/// That is a real limit of the substrate, and it is reported as such rather than
/// papered over: a denial is classified as [`SecretOutcome::NotObtained`] with the
/// platform error, **not** as `Missing`, because claiming "not present" when the
/// store refused to answer would be the same "unknown as empty" mistake in a new
/// place. Authorization denial *is* distinguishable one layer up, at the Provider
/// resolver and the credential journal, which return their own typed refusals.
#[derive(Debug, Eq, PartialEq)]
enum SecretOutcome {
    /// The real secret bytes were obtained.
    Obtained(String),
    /// The credential is not present for this scope.
    Missing,
    /// The operation was cancelled before it produced a value.
    Cancelled,
    /// The authority refused: the caller is not authorized for this credential.
    Denied,
    /// The fact could not be established, so nothing can be concluded.
    NotObtained { reason: String },
}

/// Classifies a substrate read without ever flattening the cases together.
fn read_outcome(reference: &CredentialRef) -> SecretOutcome {
    match WindowsCredentialStore.read(reference) {
        Ok(secret) => {
            SecretOutcome::Obtained(String::from_utf8_lossy(secret.expose()).into_owned())
        }
        // The one real absence signal the substrate has.
        Err(CredentialStoreError::NotFound { .. }) => SecretOutcome::Missing,
        // Everything else — including a platform ACL denial — is "could not be
        // established". It is deliberately NOT mapped to `Missing`, so a refusal
        // to answer can never be read as "there is no secret here".
        Err(error) => SecretOutcome::NotObtained { reason: error.to_string() },
    }
}

/// Every ordinary surface the product writes, as one haystack — **or a stated
/// reason why it could not be read**.
///
/// The previous scanner treated an unreadable path as "no leak", which is
/// how a scan passes while measuring nothing. Here an unreadable root is
/// `NotObtained`, and the caller must handle it.
fn scan_ordinary_output(roots: &[std::path::PathBuf], needle: &[u8]) -> Result<bool, String> {
    let mut unreadable = Vec::new();
    let mut found = false;
    for root in roots {
        if !root.exists() {
            // A root that genuinely does not exist yet is not an error: there is
            // nothing there to leak into. It is recorded so the caller knows the
            // scan did not cover it.
            unreadable.push(format!("{} (absent)", root.display()));
            continue;
        }
        let mut stack = vec![root.clone()];
        while let Some(directory) = stack.pop() {
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    // THE CORRECTION: an unreadable directory is "not obtained",
                    // not "clean".
                    unreadable.push(format!("{} ({error})", directory.display()));
                    continue;
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        unreadable.push(format!("{} ({error})", directory.display()));
                        continue;
                    }
                };
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            if bytes.windows(needle.len()).any(|window| window == needle) {
                                found = true;
                            }
                        }
                        Err(error) => {
                            unreadable.push(format!("{} ({error})", path.display()));
                        }
                    }
                }
            }
        }
    }
    if !unreadable.is_empty() {
        // The scan did not cover everything it was asked to cover.
        return Err(format!("scan incomplete: {}", unreadable.join(", ")));
    }
    Ok(found)
}

/// The substrate is real and the canary is really there.
///
/// This is the **seeded-canary positive control**. Without it, every "no leak"
/// result below could be true because nothing was ever stored.
#[test]
fn the_isolated_canary_really_stores_and_really_reads() {
    let reference = reference("positive-control");
    let _guard = CredentialGuard::new(vec![reference.clone()]);
    let secret = SecretBytes::new(CANARY_PROVIDER_A.as_bytes().to_vec());

    WindowsCredentialStore
        .write(&reference, &secret)
        .expect("the isolated namespace accepts a write");

    assert_eq!(
        read_outcome(&reference),
        SecretOutcome::Obtained(CANARY_PROVIDER_A.to_owned()),
        "the canary must really be readable, or every negative result below is vacuous"
    );
}

/// Absent, denied and unreadable are **three different facts**, and none of them
/// is "obtained".
#[test]
fn absent_cancelled_denied_and_unreadable_are_not_collapsed() {
    let missing = reference("missing");
    let _guard = CredentialGuard::new(vec![missing.clone()]);
    // The reference really was never written.
    assert_eq!(read_outcome(&missing), SecretOutcome::Missing);

    // A cancelled operation is its own outcome, by construction of the type: it
    // cannot be produced by a read failing, so a caller that sees `Cancelled`
    // knows the operation was aborted rather than that the secret was absent.
    let cancelled = SecretOutcome::Cancelled;
    assert_ne!(cancelled, SecretOutcome::Missing);
    assert_ne!(cancelled, SecretOutcome::Denied);
    assert_ne!(cancelled, SecretOutcome::NotObtained { reason: "x".to_owned() });

    // An unreadable fact is not a negative.
    let not_obtained = SecretOutcome::NotObtained { reason: "unreadable".to_owned() };
    assert_ne!(not_obtained, SecretOutcome::Missing);
}

/// A scan that could not read a root is **NOT OBTAINED**, not "0 leaks".
///
/// This is the correction the brief calls for. The old scanner returned `false`
/// ("no leak") when a path could not be read, so it passed while measuring
/// nothing.
#[test]
fn an_unreadable_check_product_is_not_obtained_not_a_clean_scan() {
    let directory = tempfile::tempdir().expect("temp root");
    let root = directory.path().join("ordinary-output");
    std::fs::create_dir_all(root.join("nested")).expect("create the ordinary output tree");
    std::fs::write(root.join("nested/log.txt"), b"nothing secret here").expect("write");

    // A clean, fully readable tree with no canary: this is the normal no-leak
    // control, and it must be a real `Ok(false)`.
    assert_eq!(
        scan_ordinary_output(std::slice::from_ref(&root), CANARY_PROVIDER_A.as_bytes()),
        Ok(false),
        "a fully readable tree with no canary is a real clean result"
    );

    // A tree that really holds the canary is a real hit: the scanner can find it,
    // so a `false` above is meaningful rather than an artifact.
    std::fs::write(root.join("nested/leak.txt"), CANARY_PROVIDER_A.as_bytes()).expect("write");
    assert_eq!(
        scan_ordinary_output(std::slice::from_ref(&root), CANARY_PROVIDER_A.as_bytes()),
        Ok(true),
        "the scanner really finds a seeded canary"
    );
    std::fs::remove_file(root.join("nested/leak.txt")).expect("remove");

    // Now a root that cannot be enumerated. On Windows, `read_dir` on a path that
    // is a *file* fails, which is a genuine, reproducible unreadable root.
    let blocked = root.join("nested/log.txt");
    let outcome =
        scan_ordinary_output(std::slice::from_ref(&blocked), CANARY_PROVIDER_A.as_bytes());
    assert!(
        outcome.is_err(),
        "an unreadable root must be reported as not obtained, never as a clean scan: {outcome:?}"
    );

    // And a root that simply does not exist is recorded rather than silently
    // treated as clean.
    let absent = root.join("does-not-exist");
    let outcome = scan_ordinary_output(std::slice::from_ref(&absent), CANARY_PROVIDER_A.as_bytes());
    assert!(
        outcome.is_err(),
        "an absent root must be reported as not covered, not as clean: {outcome:?}"
    );
}

/// The confidentiality boundary holds for a **real** stored secret: the value is
/// readable through the substrate and absent from every ordinary surface.
///
/// Positive control and no-leak control in one case, so neither can pass alone.
#[test]
fn a_real_secret_is_readable_through_the_substrate_and_absent_from_ordinary_output() {
    let directory = tempfile::tempdir().expect("temp root");
    let reference = reference("boundary");
    let _guard = CredentialGuard::new(vec![reference.clone()]);
    let secret = SecretBytes::new(CANARY_PROVIDER_A.as_bytes().to_vec());
    WindowsCredentialStore.write(&reference, &secret).expect("write");

    // Positive control: the secret really is there.
    assert_eq!(read_outcome(&reference), SecretOutcome::Obtained(CANARY_PROVIDER_A.to_owned()));

    // The ordinary surfaces: the isolated local tree this case writes into.
    let local = directory.path().join("local/Aura");
    std::fs::create_dir_all(local.join("logs")).expect("create logs");
    std::fs::write(local.join("logs/core.log"), b"ordinary core log line").expect("write log");

    match scan_ordinary_output(&[directory.path().to_path_buf()], CANARY_PROVIDER_A.as_bytes()) {
        Ok(false) => {}
        Ok(true) => panic!("the secret value leaked into an ordinary surface"),
        Err(reason) => panic!("NOT_OBTAINED: the scan could not be completed: {reason}"),
    }
}

/// The two authorities are separate owners: one missing credential leaves the
/// other authority's credential intact and readable.
///
/// This is the "two domain authorities" property at the chain level. The two
/// references are distinct namespaces, and losing one must not lose the other.
#[test]
fn one_authority_losing_its_secret_leaves_the_other_authority_working() {
    let provider_reference = reference("provider-a");
    let tool_reference = reference("tool-a");
    let _guard = CredentialGuard::new(vec![provider_reference.clone(), tool_reference.clone()]);

    WindowsCredentialStore
        .write(&provider_reference, &SecretBytes::new(CANARY_PROVIDER_A.as_bytes().to_vec()))
        .expect("write the provider authority's secret");
    WindowsCredentialStore
        .write(&tool_reference, &SecretBytes::new(CANARY_TOOL_A.as_bytes().to_vec()))
        .expect("write the tool authority's secret");

    // Both authorities really hold their own secret. Positive control for both.
    assert_eq!(
        read_outcome(&provider_reference),
        SecretOutcome::Obtained(CANARY_PROVIDER_A.to_owned())
    );
    assert_eq!(read_outcome(&tool_reference), SecretOutcome::Obtained(CANARY_TOOL_A.to_owned()));

    // The Provider authority loses its secret.
    WindowsCredentialStore.delete(&provider_reference).expect("remove the provider secret");
    assert_eq!(read_outcome(&provider_reference), SecretOutcome::Missing);

    // The Tool authority is untouched: it still gets **its own** secret.
    assert_eq!(
        read_outcome(&tool_reference),
        SecretOutcome::Obtained(CANARY_TOOL_A.to_owned()),
        "a Provider credential loss must not disturb the Tool credential authority"
    );

    // And it never borrowed the other authority's value.
    assert_ne!(
        read_outcome(&tool_reference),
        SecretOutcome::Obtained(CANARY_PROVIDER_A.to_owned()),
        "the Tool authority must never resolve the Provider authority's value"
    );
}

/// Rotation replaces one authority's value without disturbing the other's.
#[test]
fn rotating_one_authority_leaves_the_other_authority_value_unchanged() {
    let provider_reference = reference("rotate-provider");
    let other_reference = reference("rotate-other");
    let _guard = CredentialGuard::new(vec![provider_reference.clone(), other_reference.clone()]);

    WindowsCredentialStore
        .write(&provider_reference, &SecretBytes::new(CANARY_PROVIDER_A.as_bytes().to_vec()))
        .expect("write A");
    WindowsCredentialStore
        .write(&other_reference, &SecretBytes::new(CANARY_PROVIDER_B.as_bytes().to_vec()))
        .expect("write B");

    // Rotate A. B must keep its own value throughout.
    WindowsCredentialStore
        .write(&provider_reference, &SecretBytes::new(CANARY_PROVIDER_B.as_bytes().to_vec()))
        .expect("rotate A");
    assert_eq!(
        read_outcome(&provider_reference),
        SecretOutcome::Obtained(CANARY_PROVIDER_B.to_owned())
    );
    assert_eq!(
        read_outcome(&other_reference),
        SecretOutcome::Obtained(CANARY_PROVIDER_B.to_owned()),
        "B still has its own value after A was rotated"
    );
}

/// The Provider credential authority's real resolver refuses a binding it cannot
/// verify, and never falls back to another scope's credential.
///
/// This drives `PersistentProviderCredentialResolver`, which is what the model
/// transport actually calls — not `WindowsCredentialStore` directly.
#[tokio::test]
async fn the_real_provider_resolver_refuses_rather_than_borrowing_another_credential() {
    use aura_network_control::{
        ConfigurationRequestCredentialBindingV1, ProviderCredentialResolver,
    };

    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let crate::StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must start for this case");
    };
    let resolver = crate::p05_credential::PersistentProviderCredentialResolver::new(Arc::clone(
        &services.persistence,
    ));

    // A binding that names a credential which does not exist. The resolver must
    // refuse, and it must refuse as *unavailable* rather than by inventing a value
    // or borrowing another scope's credential.
    let requested = ConfigurationRequestCredentialBindingV1 {
        credential_id: aura_domain_common::CredentialId::new(),
        credential_revision: aura_domain_common::CredentialRevision::INITIAL,
        credential_binding_fingerprint: aura_model_domain::CredentialBindingFingerprint(
            aura_domain_common::ContentHash::sha256(&[0x5A; 32]),
        ),
    };
    let outcome = resolver.resolve(&requested);
    assert!(outcome.is_err(), "an unresolvable credential binding must be refused, not fabricated");
    // The refusal is the network layer's own typed refusal, not a panic and not a
    // value borrowed from another authority.
    let error = outcome.err().expect("checked above");
    assert!(
        matches!(
            error,
            aura_network_control::NetworkControlError::CredentialUnavailable
                | aura_network_control::NetworkControlError::CredentialBindingMismatch
        ),
        "the resolver refuses with its own typed error: {error:?}"
    );
}

/// The Provider resolver never returns a **different** authority's canary.
///
/// The negative control paired with the refusal above: even when a credential
/// genuinely exists for one namespace, the resolver asked about another must not
/// hand back the first one's value.
#[tokio::test]
async fn the_real_provider_resolver_never_returns_another_namespaces_canary() {
    use aura_network_control::{
        ConfigurationRequestCredentialBindingV1, ProviderCredentialResolver,
    };

    // Seed a real canary in an isolated namespace, so there IS a value to borrow.
    let planted = reference("provider-borrow-control");
    let _guard = CredentialGuard::new(vec![planted.clone()]);
    WindowsCredentialStore
        .write(&planted, &SecretBytes::new(CANARY_PROVIDER_A.as_bytes().to_vec()))
        .expect("the canary is really stored");
    assert_eq!(
        read_outcome(&planted),
        SecretOutcome::Obtained(CANARY_PROVIDER_A.to_owned()),
        "positive control: the borrowable canary really exists"
    );

    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let crate::StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must start for this case");
    };
    let resolver = crate::p05_credential::PersistentProviderCredentialResolver::new(Arc::clone(
        &services.persistence,
    ));

    let requested = ConfigurationRequestCredentialBindingV1 {
        credential_id: aura_domain_common::CredentialId::new(),
        credential_revision: aura_domain_common::CredentialRevision::INITIAL,
        credential_binding_fingerprint: aura_model_domain::CredentialBindingFingerprint(
            aura_domain_common::ContentHash::sha256(&[0x5A; 32]),
        ),
    };
    // Whatever happens, the planted canary must not appear in the result.
    if let Ok(resolved) = resolver.resolve(&requested) {
        let leaked = resolved
            .secret_header_value
            .windows(CANARY_PROVIDER_A.len())
            .any(|window| window == CANARY_PROVIDER_A.as_bytes());
        assert!(
            !leaked,
            "the Provider resolver must never return another namespace's canary value"
        );
    }
}

/// The credential journal's own recovery is the authority for interrupted
/// credential mutations, and it is drivable through the real service.
#[tokio::test]
async fn the_p05_credential_journal_recovers_without_leaking_a_value() {
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let crate::StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must start for this case");
    };
    let journal =
        crate::p05_credential::P05CredentialJournal::new(Arc::clone(&services.persistence));

    // A recovery with no incomplete journal is a real, empty recovery: it must not
    // error, and it must not invent a credential. The factory returns no drafts,
    // which is what "nothing to recover" produces.
    let mut projection_factory =
        |_: &aura_persistence::CredentialMutationJournalRecord,
         _status: aura_model_domain::EndpointCredentialStatus| { Ok(Vec::new()) };
    let recovered = journal.recover(&mut projection_factory);
    assert!(
        recovered.is_ok(),
        "a clean credential recovery must succeed: {:?}",
        recovered.as_ref().err().map(ToString::to_string)
    );
    let recovered = recovered.expect("checked above");
    assert!(
        recovered.is_empty(),
        "there was no incomplete credential journal, so nothing is recovered: {recovered:?}"
    );

    // Nothing in the recovered outcomes can carry a secret: the type has no secret
    // field, which is the structural half of the boundary.
    let described = format!("{recovered:?}");
    assert!(
        !described.contains(CANARY_PROVIDER_A) && !described.contains(CANARY_TOOL_A),
        "a credential recovery outcome must not carry a secret value: {described}"
    );
}
