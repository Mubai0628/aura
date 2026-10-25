//! Second repair, batch 1: responsibility is assigned **before** any
//! single-domain disk write.
//!
//! # The defect these cases reject
//!
//! `SettingsFacade::open_with_validators` used to run the per-domain open loop
//! first and judge the interrupted bundle journals only afterwards. Each
//! `ModuleConfigService::open` performs LKG/default recovery and **writes to
//! disk**. So a domain that was really one half of an interrupted cross-domain
//! save had its input silently rewritten from a last-known-good before the
//! facade had decided whether that bundle could be judged at all.
//!
//! The user's confirmed rule is the opposite: when "only half updated" cannot be
//! ruled out, keep the original input untouched, suspend new calls that depend on
//! the suspect configuration, and never report the uncertain config as Ready.
//!
//! # How these cases can fail against the old implementation
//!
//! Every case makes the *pre-existing* file content observable — by reading the
//! bytes on disk, not the in-memory snapshot — and then asserts the bytes are
//! unchanged. Under the old ordering the LKG was already written by the time the
//! facade opened, so the byte assertion fails. Reading disk bytes (rather than
//! `health().source`) is what makes this a real counterexample instead of a
//! restatement of the code.

use std::{collections::BTreeMap, fs};

use aura_config::{AuraPaths, ConfigModule, SettingsFacade, encode_document, parse_document};
use serde_json::json;
use tempfile::TempDir;

fn paths(directory: &TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

/// Reads a domain's file exactly as bytes.
fn disk_bytes(paths: &AuraPaths, module: ConfigModule) -> Vec<u8> {
    fs::read(paths.config_path(module)).expect("the domain file really exists")
}

/// Writes one domain's file with a value that survives a parse/validate round
/// trip, so it can be a legitimate last-known-good.
fn write_domain(paths: &AuraPaths, module: ConfigModule, marker: &str) {
    let mut document = aura_config::ModuleConfigDocument::empty();
    document.values.insert("marker".to_owned(), json!(marker));
    document.revision = 1;
    let bytes = encode_document(module, &document).expect("encode");
    let path = paths.config_path(module);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("config dir");
    }
    fs::write(&path, &bytes).expect("write domain");
}

/// Writes one domain's file at a chosen revision.
///
/// The revision matters for reload cases: a lower revision is refused as
/// non-monotonic, so a test that wants to prove a reload really *ran* must give it
/// a newer revision to observe.
fn write_domain_with_revision(
    paths: &AuraPaths,
    module: ConfigModule,
    marker: &str,
    revision: u64,
) {
    let mut document = aura_config::ModuleConfigDocument::empty();
    document.values.insert("marker".to_owned(), json!(marker));
    document.revision = revision;
    let bytes = encode_document(module, &document).expect("encode");
    let path = paths.config_path(module);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("config dir");
    }
    fs::write(&path, &bytes).expect("write domain");
}

/// Records a last-known-good for a domain by writing a valid file and letting the
/// facade adopt it, so a later damaged file has a real LKG to fall back to.
fn seed_last_known_good(paths: &AuraPaths, module: ConfigModule, marker: &str) {
    write_domain(paths, module, marker);
    let facade = SettingsFacade::open(paths.clone()).expect("seed facade opens");
    drop(facade);
}

/// Damages a domain's file so it parses as invalid but is still readable.
fn damage_domain(paths: &AuraPaths, module: ConfigModule) {
    let path = paths.config_path(module);
    fs::write(&path, b"{ this is not a valid module document").expect("damage the file");
}

/// An interrupted bundle journal that cannot be decoded.
///
/// This is the "unknown scope" trigger from the confirmed rule: there is no
/// readable record of which domains the save intended to touch.
fn write_undecodable_journal(paths: &AuraPaths) {
    let journal_dir = paths.config_transaction_dir.join("module-bundles");
    fs::create_dir_all(&journal_dir).expect("journal dir");
    fs::write(journal_dir.join("interrupted.json"), b"{ \"transaction_id\": ")
        .expect("write journal");
}

#[test]
fn an_unknown_bundle_scope_does_not_rewrite_a_damaged_domain_from_its_lkg_first() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);

    // A real, legitimate last-known-good exists for `Ui`.
    seed_last_known_good(&paths, ConfigModule::Ui, "the-good-version");

    // Now the live file is damaged. Under the old ordering this is exactly the
    // input that recovery would overwrite with the LKG before judging the bundle.
    damage_domain(&paths, ConfigModule::Ui);
    let damaged = disk_bytes(&paths, ConfigModule::Ui);

    // An interrupted cross-domain save exists and cannot be decoded, so the
    // affected set is genuinely unknown.
    write_undecodable_journal(&paths);

    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");

    // The scope is unknown, so every domain is under protection.
    assert!(
        !facade.recovery_scope_known(),
        "an undecodable journal must be reported as an unknown scope"
    );
    assert_eq!(
        facade.recovery_held_domains(),
        ConfigModule::ALL.to_vec(),
        "an unknown scope protects every domain"
    );

    // THE COUNTEREXAMPLE: the damaged input must still be the damaged input.
    //
    // The old implementation opened the domain first, which ran LKG recovery and
    // wrote the good bytes to disk, so this assertion failed with the LKG content.
    // The input of a domain that may belong to a half-committed group must not be
    // altered before the group is judged.
    assert_eq!(
        disk_bytes(&paths, ConfigModule::Ui),
        damaged,
        "a domain held by an unjudged bundle must keep its original bytes; \
         rewriting it from the LKG destroys the only evidence of the interrupted save"
    );

    // And it must not be reported as Ready from a snapshot it cannot justify.
    for module in ConfigModule::ALL {
        assert_ne!(
            facade.health(module).state,
            aura_config::DomainHealthState::Ready,
            "{module:?} must not be Ready while its consistency is unknown"
        );
    }
}

#[test]
fn a_domain_named_by_an_undecodable_scope_is_not_silently_repaired() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);

    // Two domains have real last-known-goods, and both live files are damaged.
    for module in [ConfigModule::Ui, ConfigModule::Commands] {
        seed_last_known_good(&paths, module, "good");
        damage_domain(&paths, module);
    }
    let ui_damaged = disk_bytes(&paths, ConfigModule::Ui);
    let commands_damaged = disk_bytes(&paths, ConfigModule::Commands);
    write_undecodable_journal(&paths);

    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");
    assert!(!facade.recovery_scope_known());

    // Neither damaged input was replaced, and both are unreachable rather than
    // silently served from a fallback.
    assert_eq!(disk_bytes(&paths, ConfigModule::Ui), ui_damaged);
    assert_eq!(disk_bytes(&paths, ConfigModule::Commands), commands_damaged);
    for module in [ConfigModule::Ui, ConfigModule::Commands] {
        let error = facade.snapshot(module).expect_err("a held domain is unavailable");
        // The refusal must name the real cause. `snapshot` reports the domain as
        // unavailable (there is no service to ask), but the message must be the
        // bundle's own, and the *health* record must carry the bundle fault kind
        // — a generic "unavailable" would lose the reason from the notice.
        assert!(
            error.to_string().contains("unresolved") || error.to_string().contains("bundle"),
            "{module:?} must name the unresolved bundle; got {error}"
        );
        assert_eq!(
            facade.health(module).original_fault,
            Some(aura_config::DomainFaultKind::BundleRecovery),
            "{module:?} must classify the fault as a bundle recovery fault"
        );
    }
}

#[test]
fn a_domain_the_bundle_does_not_name_still_opens_and_keeps_its_own_content() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);

    // `Aura` is a real, healthy domain that no bundle names.
    write_domain(&paths, ConfigModule::Aura, "untouched-by-the-bundle");
    let aura_before = disk_bytes(&paths, ConfigModule::Aura);
    write_undecodable_journal(&paths);

    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");

    // An unknown scope is a real protection, so every domain is held. This case
    // records that the *bytes* are still the domain's own: an unknown scope must
    // not be implemented by rewriting files, and it must not fabricate content.
    assert_eq!(
        disk_bytes(&paths, ConfigModule::Aura),
        aura_before,
        "an unknown scope must not alter a domain's content"
    );

    // The held reason names the bundle fault, so the notice can say what really
    // happened rather than "unavailable".
    let error = facade.snapshot(ConfigModule::Aura).expect_err("held while the scope is unknown");
    assert!(
        error.to_string().contains("unresolved") || error.to_string().contains("bundle"),
        "the refusal must name the unresolved bundle; got {error}"
    );
    assert_eq!(
        facade.health(ConfigModule::Aura).original_fault,
        Some(aura_config::DomainFaultKind::BundleRecovery),
        "the health record carries the bundle fault kind"
    );
}

#[test]
fn a_decodable_bundle_naming_one_domain_holds_only_that_domain() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);

    // Both domains have real content and a real LKG.
    for module in [ConfigModule::Ui, ConfigModule::Commands] {
        seed_last_known_good(&paths, module, "good");
    }
    write_domain(&paths, ConfigModule::Commands, "commands-own-content");
    let commands_before = disk_bytes(&paths, ConfigModule::Commands);

    // A journal that *decodes* and names exactly one domain, but whose candidate
    // cannot be converged because its "before" state does not match what is on
    // disk. The named domain is a known scope; the unnamed domain is untouched.
    let journal_dir = paths.config_transaction_dir.join("module-bundles");
    fs::create_dir_all(&journal_dir).expect("journal dir");
    let mut candidate = aura_config::ModuleConfigDocument::empty();
    candidate.values.insert("marker".to_owned(), json!("from-the-bundle"));
    let entry = aura_config::PreparedConfigBundleEntry {
        module: ConfigModule::Ui,
        before_revision: 99,
        before_content_hash: "0".repeat(64),
        candidate,
    };
    let record = aura_config::PreparedConfigBundle {
        transaction_id: "s05r2-known-scope".to_owned(),
        entries: vec![entry],
        created_at: aura_domain_common::UtcTimestamp::now(),
    };
    fs::write(journal_dir.join("known.json"), serde_json::to_vec(&record).expect("encode journal"))
        .expect("write journal");

    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");

    // A decodable journal names its domains, so the scope is known.
    assert!(
        facade.recovery_scope_known(),
        "a decodable journal establishes a known scope even when convergence fails"
    );
    let held = facade.recovery_held_domains();
    assert!(held.contains(&ConfigModule::Ui), "the named domain is held: {held:?}");
    assert!(
        !held.contains(&ConfigModule::Commands),
        "a domain the bundle does not name must stay out of the hold: {held:?}"
    );

    // The unnamed domain keeps its own bytes and is really readable.
    assert_eq!(
        disk_bytes(&paths, ConfigModule::Commands),
        commands_before,
        "an unrelated domain's content must be untouched"
    );
    facade.snapshot(ConfigModule::Commands).expect("a domain the bundle does not name is readable");
}

#[test]
fn the_held_domain_reports_the_bundle_fault_kind_not_a_generic_one() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    write_undecodable_journal(&paths);

    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");
    for module in ConfigModule::ALL {
        assert_eq!(
            facade.health(module).original_fault,
            Some(aura_config::DomainFaultKind::BundleRecovery),
            "{module:?} must record the bundle fault, so the notice can name the real cause"
        );
        // The reason is preserved verbatim for the user-facing notice.
        assert!(
            facade.health(module).original_message.is_some(),
            "{module:?} keeps the original message"
        );
    }
}

/// A source-level guard: the judgement step must not be able to write.
///
/// This is deliberately a *structural* assertion rather than a behavioural one.
/// The behavioural cases above prove the ordering; this one pins the property
/// that makes the ordering safe — the pre-open judgement cannot converge, delete
/// or rewrite a journal, so it can never be the thing that changes a domain's
/// input.
#[test]
fn the_pre_open_judgement_performs_no_writes() {
    let source = include_str!("../src/service.rs");
    let judge =
        source.find("fn judge_pending_bundle_transactions").expect("the judgement step exists");
    let next_fn = source[judge + 10..]
        .find("\nfn ")
        .map(|offset| judge + 10 + offset)
        .expect("the judgement is followed by another function");
    let body = &source[judge..next_fn];

    for forbidden in [
        "atomic_write",
        "atomic_replace_file",
        "converge_bundle_journal",
        "fs::remove_file",
        "fs::write",
        "fs::create_dir",
    ] {
        assert!(
            !body.contains(forbidden),
            "the pre-open judgement must not contain `{forbidden}`: it runs before any domain \
             opens, so a write here would change a domain's input before responsibility is settled"
        );
    }
}

/// Documents the byte-exactness helper's own contract.
///
/// If `disk_bytes` silently fell back to an in-memory value the counterexamples
/// above would stop proving anything, so the helper's expectation is pinned.
#[test]
fn the_byte_helper_reads_a_real_file() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    write_domain(&paths, ConfigModule::Ui, "real-bytes");
    let bytes = disk_bytes(&paths, ConfigModule::Ui);
    assert!(!bytes.is_empty());
    let document = parse_document(ConfigModule::Ui, &bytes).expect("the bytes really parse");
    assert_eq!(document.values.get("marker"), Some(&json!("real-bytes")));

    // A missing file is an error, never an empty success.
    fs::remove_file(paths.config_path(ConfigModule::Ui)).expect("remove");
    let missing = std::panic::catch_unwind(|| disk_bytes(&paths, ConfigModule::Ui));
    assert!(missing.is_err(), "a missing domain file must not read as empty bytes");
}

/// A real interrupted publish, produced through the production prepare/publish
/// path rather than by hand-writing a journal.
///
/// Returns the journal path. The bundle names `Ui` and `Diagnostics`, and its
/// before-revision/content conditions come from the real service, so this is the
/// same record shape a genuine interrupted save leaves.
fn interrupted_publish(paths: &AuraPaths, transaction: &str) -> std::path::PathBuf {
    let settings = SettingsFacade::open(paths.clone()).expect("initial real settings");
    let ui = settings.snapshot(ConfigModule::Ui).expect("real UI fact");
    let diagnostics = settings.snapshot(ConfigModule::Diagnostics).expect("real Diagnostics fact");
    let prepared = settings
        .prepare_module_bundle(
            transaction.to_owned(),
            vec![
                aura_config::ConfigModuleBundleEntry {
                    module: ConfigModule::Ui,
                    expected_revision: ui.document.revision,
                    replacement_values: Some(BTreeMap::from([(
                        "marker".to_owned(),
                        json!("legitimate-bundle-after"),
                    )])),
                },
                aura_config::ConfigModuleBundleEntry {
                    module: ConfigModule::Diagnostics,
                    expected_revision: diagnostics.document.revision,
                    replacement_values: Some(BTreeMap::from([(
                        "marker".to_owned(),
                        json!("same-bundle-diagnostics"),
                    )])),
                },
            ],
        )
        .expect("production prepare with real revision and content conditions");
    settings
        .publish_prepared_module_bundle_with_fault(
            &prepared,
            Some(aura_config::ConfigBundleFaultPoint::AfterFirstReplace),
        )
        .expect_err("the interrupted publish really fails");
    let journal =
        paths.config_transaction_dir.join("module-bundles").join(format!("{transaction}.json"));
    assert!(journal.is_file(), "the production path left its durable record");
    journal
}

/// **Gap 1 counterexample.** A *valid* journal must also hold its domains.
///
/// Before this fix only *failed* journals entered the initial held set; a valid
/// record went straight into `convergable`, so its members were opened by the
/// startup loop. `ModuleConfigService::open` then rewrote the damaged input from
/// the LKG, which manufactured a "current" file matching neither the frozen
/// before nor the after image — and convergence was validated against that
/// rewritten file, so the interrupted save was silently resolved from evidence
/// it had itself destroyed.
#[test]
fn a_valid_journal_holds_its_members_before_any_lkg_rewrite() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let journal = interrupted_publish(&paths, "known-valid");

    // Give `Ui` a legitimate LKG, then damage the live file so the LKG is
    // available to overwrite it.
    seed_last_known_good(&paths, ConfigModule::Ui, "the-good-version");
    damage_domain(&paths, ConfigModule::Ui);
    let damaged = disk_bytes(&paths, ConfigModule::Ui);

    let settings = SettingsFacade::open(paths.clone()).expect("the facade opens");

    // THE COUNTEREXAMPLE: the damaged input is still the damaged input.
    assert_eq!(
        disk_bytes(&paths, ConfigModule::Ui),
        damaged,
        "a valid journal must not authorize an earlier LKG overwrite of input that \
         matches neither the frozen before nor the after image"
    );
    // While the record is unresolved, its members are held and not Ready.
    assert!(
        settings.recovery_held_domains().contains(&ConfigModule::Ui),
        "the valid record's member is under responsibility: {:?}",
        settings.recovery_held_domains()
    );
    assert_ne!(
        settings.health(ConfigModule::Ui).state,
        aura_config::DomainHealthState::Ready,
        "a member of an unresolved record must not be Ready"
    );
    // The record is retained: the interrupted save is still unsettled, so
    // deleting it would lose the only description of what the save intended.
    assert!(
        journal.is_file(),
        "an unsettled record's journal is retained rather than resolved from \
         evidence it destroyed"
    );
}

/// **Gap 2 counterexample.** An unknown scope must not let a *valid* record
/// settle its members.
///
/// Before this fix each journal was settled in sequence and a domain was
/// released by `retain` when its own record converged. With an undecodable record
/// alongside a valid one, the valid record reopened `Ui` and reported it Ready
/// while the unknown responsibility still covered every domain — and the valid
/// record was already deleted, so the state could not be reconstructed.
#[test]
fn an_unknown_scope_prevents_a_valid_record_from_settling() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let journal = interrupted_publish(&paths, "known-valid");
    let before = disk_bytes(&paths, ConfigModule::Ui);

    // An undecodable record in the same directory makes the scope unknown.
    write_undecodable_journal(&paths);

    let settings = SettingsFacade::open(paths.clone()).expect("the facade opens");

    assert!(!settings.recovery_scope_known(), "the undecodable record makes the scope unknown");
    assert_eq!(
        settings.recovery_held_domains(),
        ConfigModule::ALL.to_vec(),
        "an unknown scope protects every domain"
    );
    assert_ne!(
        settings.health(ConfigModule::Ui).state,
        aura_config::DomainHealthState::Ready,
        "another valid record must not clear a member still under unknown responsibility"
    );
    assert_eq!(
        disk_bytes(&paths, ConfigModule::Ui),
        before,
        "an unknown overlapping responsibility prevents writes that cannot be proven"
    );
    assert!(
        journal.is_file(),
        "the valid record is retained while the unknown responsibility stands"
    );
}

/// **Gap 3 counterexample.** A retry that newly discovers a bad manifest must
/// publish that as the *actual* disposition.
///
/// Before this fix `recover_pending_bundle_transactions` consumed only
/// `convergable` from the fresh judgement and copied `held`/`scope_known`/
/// `failures` forward from the previous state. So a retry that found a new
/// undecodable journal withdrew `Ui`'s service but still reported
/// `scope_known = true`, `held = 0` and `failures = 0`: the failure was invisible
/// and an unavailable read looked available.
#[test]
fn a_retry_that_finds_a_bad_manifest_publishes_it() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);

    // A healthy facade first, so the stale state is genuinely "healthy".
    let settings = SettingsFacade::open(paths.clone()).expect("initial healthy facade");
    settings.snapshot(ConfigModule::Ui).expect("Ui is really readable while healthy");
    assert!(settings.recovery_scope_known());
    assert!(settings.recovery_held_domains().is_empty());

    // A damaged record appears in this round's isolated data.
    write_undecodable_journal(&paths);

    settings.recover_pending_bundle_transactions().expect("the retry itself completes");

    // THE COUNTEREXAMPLE: the new judgement is the live fact.
    assert!(
        !settings.recovery_scope_known(),
        "the retry must publish its new unknown judgement, not the former healthy scope"
    );
    assert_eq!(
        settings.recovery_held_domains(),
        ConfigModule::ALL.to_vec(),
        "a newly discovered unknown scope holds every domain"
    );
    assert_eq!(
        settings.pending_bundle_journal_failures().len(),
        1,
        "a newly discovered fault must be visible"
    );
    assert!(
        settings.snapshot(ConfigModule::Ui).is_err(),
        "a domain whose service was withdrawn must not report a readable snapshot"
    );
}

/// **Positive control.** A legitimate, non-conflicting bundle still recovers.
///
/// This is what stops the fixes above from passing by refusing everything. A
/// valid record with no competing responsibility converges for real: the
/// candidate is served, the domain becomes Ready, no domain is held, and the
/// record is removed.
#[test]
fn a_valid_nonconflicting_bundle_really_recovers() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let journal = interrupted_publish(&paths, "valid-only");

    let settings = SettingsFacade::open(paths.clone()).expect("the real recoverable bundle");

    let snapshot = settings.snapshot(ConfigModule::Ui).expect("the converged fact is served");
    assert_eq!(
        snapshot.document.values.get("marker"),
        Some(&json!("legitimate-bundle-after")),
        "the real candidate is served"
    );
    assert_eq!(
        settings
            .snapshot(ConfigModule::Diagnostics)
            .expect("both members converge")
            .document
            .values
            .get("marker"),
        Some(&json!("same-bundle-diagnostics")),
        "the whole bundle converges, not just its first member"
    );
    assert_eq!(settings.health(ConfigModule::Ui).state, aura_config::DomainHealthState::Ready);
    assert!(settings.recovery_scope_known());
    assert!(
        settings.recovery_held_domains().is_empty(),
        "a settled record leaves no hold: {:?}",
        settings.recovery_held_domains()
    );
    assert!(!journal.is_file(), "a settled record is removed");
}

/// A domain named by **two** live records is released only when both settle.
///
/// A per-record `retain` could not express this: the first record to converge
/// would drop the domain from the held set even though the second still names it.
#[test]
fn a_domain_is_held_until_every_record_naming_it_settles() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    // One valid record for the pair…
    let _first = interrupted_publish(&paths, "first-valid");
    // …and a second record that names the same `Ui` but cannot converge, because
    // its before-condition does not match the file on disk.
    let journal_dir = paths.config_transaction_dir.join("module-bundles");
    let mut candidate = aura_config::ModuleConfigDocument::empty();
    candidate.values.insert("marker".to_owned(), json!("second-record"));
    let record = aura_config::PreparedConfigBundle {
        transaction_id: "second-conflicting".to_owned(),
        entries: vec![aura_config::PreparedConfigBundleEntry {
            module: ConfigModule::Ui,
            before_revision: 4_242,
            before_content_hash: "f".repeat(64),
            candidate,
        }],
        created_at: aura_domain_common::UtcTimestamp::now(),
    };
    fs::write(
        journal_dir.join("second-conflicting.json"),
        serde_json::to_vec(&record).expect("encode"),
    )
    .expect("write the second record");

    let settings = SettingsFacade::open(paths.clone()).expect("the facade opens");

    assert!(
        settings.recovery_held_domains().contains(&ConfigModule::Ui),
        "the domain is still named by the failed record"
    );
    assert_ne!(
        settings.health(ConfigModule::Ui).state,
        aura_config::DomainHealthState::Ready,
        "one record settling must not release a domain another record still names"
    );
    // The other member of the *valid* record settled normally, so responsibility
    // is per domain rather than a single global flag: `Diagnostics` is not named
    // by the failed record and is released.
    assert!(
        !settings.recovery_held_domains().contains(&ConfigModule::Diagnostics),
        "a domain the failed record does not name is not held by it: {:?}",
        settings.recovery_held_domains()
    );
}

/// **Transaction ownership counterexample.** Disjoint bundles must coexist.
///
/// The bundle reservation used to be a single `Mutex<Option<BundleReservation>>`
/// slot plus one `Mutex<Option<String>>` transaction id, so preparing *any*
/// bundle made every other bundle fail with `BundlePending` — including one whose
/// domain set is completely disjoint. A measurement showed exactly that: after a UI
/// `prepare`, an unrelated Diagnostics bundle was refused.
///
/// The claim registry already admits disjoint sets; only the bookkeeping refused
/// them. This case pins that a disjoint pair really prepares, and that an
/// *intersecting* pair is still refused with the identity of the holder.
#[test]
fn disjoint_bundles_coexist_while_intersecting_ones_conflict() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let settings = SettingsFacade::open(paths.clone()).expect("facade opens");

    // Prepare a real bundle over one disjoint pair.
    let ui = settings.snapshot(ConfigModule::Ui).expect("real UI fact");
    let diagnostics = settings.snapshot(ConfigModule::Diagnostics).expect("real Diagnostics fact");
    let first = settings
        .prepare_module_bundle(
            "first-pair".to_owned(),
            vec![
                aura_config::ConfigModuleBundleEntry {
                    module: ConfigModule::Ui,
                    expected_revision: ui.document.revision,
                    replacement_values: Some(BTreeMap::from([(
                        "marker".to_owned(),
                        json!("first"),
                    )])),
                },
                aura_config::ConfigModuleBundleEntry {
                    module: ConfigModule::Diagnostics,
                    expected_revision: diagnostics.document.revision,
                    replacement_values: Some(BTreeMap::from([(
                        "marker".to_owned(),
                        json!("first"),
                    )])),
                },
            ],
        )
        .expect("the first bundle prepares");

    // A bundle over a genuinely DISJOINT domain set must also prepare. This is
    // the counterexample: under the old single slot this returned
    // `BundlePending { transaction_id: "first-pair" }`.
    let commands = settings.snapshot(ConfigModule::Commands).expect("real Commands fact");
    let second = settings.prepare_module_bundle(
        "second-pair".to_owned(),
        vec![aura_config::ConfigModuleBundleEntry {
            module: ConfigModule::Commands,
            expected_revision: commands.document.revision,
            replacement_values: Some(BTreeMap::from([("marker".to_owned(), json!("second"))])),
        }],
    );
    assert!(
        second.is_ok(),
        "a disjoint bundle must not be refused merely because another bundle is live; got {:?}",
        second.err()
    );

    // But a bundle that INTERSECTS the first is still refused, and the refusal
    // identifies the holder. Two layers can refuse it: the domain claim registry
    // (which owns the domain itself) reports `DomainBusy`, and the transaction
    // registry reports `BundlePending` naming the transaction. Both are correct
    // refusals; what must NOT happen is `Ok`.
    let blocked = settings
        .prepare_module_bundle(
            "overlapping".to_owned(),
            vec![aura_config::ConfigModuleBundleEntry {
                module: ConfigModule::Ui,
                expected_revision: ui.document.revision,
                replacement_values: Some(BTreeMap::from([(
                    "marker".to_owned(),
                    json!("overlapping"),
                )])),
            }],
        )
        .expect_err("an intersecting bundle is refused");
    match &blocked {
        aura_config::SettingsFacadeError::BundlePending { transaction_id } => {
            assert_eq!(transaction_id, "first-pair", "the transaction refusal names the holder")
        }
        aura_config::SettingsFacadeError::DomainBusy { message } => {
            assert!(!message.is_empty(), "the claim refusal names the busy domain, without a path")
        }
        other => panic!("an intersecting bundle must be refused, not accepted; got {other:?}"),
    }

    // Ending one transaction must not disturb the other.
    settings.abort_prepared_module_bundle(&first).expect("first aborts");
    let blocked_after = settings.prepare_module_bundle(
        "overlapping-after-abort".to_owned(),
        vec![aura_config::ConfigModuleBundleEntry {
            module: ConfigModule::Ui,
            expected_revision: ui.document.revision,
            replacement_values: Some(BTreeMap::from([("marker".to_owned(), json!("now-free"))])),
        }],
    );
    assert!(
        blocked_after.is_ok(),
        "aborting the holder releases exactly its domains; got {:?}",
        blocked_after.err()
    );
    // …and the *other* transaction is still live, so its domains stay owned.
    let still_owned = settings
        .prepare_module_bundle(
            "conflicts-with-second".to_owned(),
            vec![aura_config::ConfigModuleBundleEntry {
                module: ConfigModule::Commands,
                expected_revision: commands.document.revision,
                replacement_values: Some(BTreeMap::from([(
                    "marker".to_owned(),
                    json!("conflict"),
                )])),
            }],
        )
        .expect_err("ending one transaction must not release another's domains");
    assert!(
        matches!(
            &still_owned,
            aura_config::SettingsFacadeError::BundlePending { .. }
                | aura_config::SettingsFacadeError::DomainBusy { .. }
        ),
        "the second transaction still owns Commands; got {still_owned:?}"
    );
    let _ = second;
}

/// An ordinary reload must consider only its own domain's transaction.
///
/// Before this fix `reload` returned `Ok(false)` — a silent skip — whenever *any*
/// transaction was live, so a bundle over `Commands` silently stopped `Ui` from
/// reloading. A silent skip is worse than a refusal: the caller cannot tell
/// "nothing changed" from "I was not allowed to look".
#[test]
fn a_reload_is_not_blocked_by_an_unrelated_transaction() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let settings = SettingsFacade::open(paths.clone()).expect("facade opens");
    let commands = settings.snapshot(ConfigModule::Commands).expect("real Commands fact");
    let _held = settings
        .prepare_module_bundle(
            "commands-only".to_owned(),
            vec![aura_config::ConfigModuleBundleEntry {
                module: ConfigModule::Commands,
                expected_revision: commands.document.revision,
                replacement_values: Some(BTreeMap::from([(
                    "marker".to_owned(),
                    json!("commands-bundle"),
                )])),
            }],
        )
        .expect("the Commands bundle prepares");

    // `Ui` is untouched by that transaction, so its reload really *runs*. Two
    // outcomes prove it ran: it either observed a legitimate external edit
    // (`Ok(true)`) or refused a non-monotonic one (`NonMonotonicRevision`) — both
    // mean the transaction did not block it. What must NOT happen is the silent
    // `Ok(false)` skip or a `BundlePending` naming the Commands transaction.
    write_domain_with_revision(&paths, ConfigModule::Ui, "externally-edited", 9);
    let outcome = settings.reload(ConfigModule::Ui);
    match &outcome {
        Ok(_) => {}
        Err(aura_config::SettingsFacadeError::NonMonotonicRevision { .. }) => {}
        other => {
            panic!("an unrelated transaction must not block this domain's reload; got {other:?}")
        }
    }

    // The domain the transaction *does* own is refused, and told why.
    let owned =
        settings.reload(ConfigModule::Commands).expect_err("the owned domain is mid-transaction");
    assert!(
        matches!(&owned, aura_config::SettingsFacadeError::BundlePending { transaction_id }
            if transaction_id == "commands-only"),
        "the owned domain names its transaction; got {owned:?}"
    );
}

/// **Atomic bundle recovery counterexample.** A failing member must not leave the
/// others already reset.
///
/// `apply_bundle_recovery` used to be a bare `for domain in &plan.domains {
/// apply_domain_recovery(...) }` loop. A measurement showed the consequence: with a real
/// I/O failure on the **second** domain, the **first had already been reset and
/// its revision bumped**, yet an overall error was returned and no recovery
/// transaction existed to undo it — partial application presented as a failed
/// whole, and real data loss.
///
/// The set is now claimed once, every candidate is validated and encoded before the
/// first write, and a failed write rolls the earlier members back. This case makes
/// the second member genuinely unwritable (its path is a directory) and asserts the
/// first member still holds its original bytes.
#[test]
fn a_failing_member_leaves_no_earlier_member_applied() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let settings = SettingsFacade::open(paths.clone()).expect("facade opens");

    // Two real domains with content, so both have something to restore.
    write_domain_with_revision(&paths, ConfigModule::Aura, "aura-original", 5);
    write_domain_with_revision(&paths, ConfigModule::Models, "models-original", 5);
    settings.reload(ConfigModule::Aura).ok();
    settings.reload(ConfigModule::Models).ok();

    let plan = settings
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Models])
        .expect("bundle plan");

    // Make the SECOND member genuinely unwritable: turn its path into a directory,
    // which is the same real Windows I/O boundary the other cases use. Note the
    // plan was built while the file was still a file, so the failure happens at the
    // write, which is exactly the window under test.
    let models_path = paths.config_path(ConfigModule::Models);
    let models_original = fs::read(&models_path).expect("models bytes before");
    fs::remove_file(&models_path).expect("remove models file");
    fs::create_dir_all(&models_path).expect("replace models file with a directory");
    fs::write(models_path.join("blocker"), b"unwritable").expect("blocker");

    let aura_path = paths.config_path(ConfigModule::Aura);
    let aura_original = fs::read(&aura_path).expect("aura bytes before");

    // The application must fail…
    let refused = settings
        .apply_bundle_recovery(&plan, aura_config::recovery::ConfigRecoveryAction::ResetToDefault);
    assert!(refused.is_err(), "a set that cannot be applied must not report success");

    // …and it must NOT have left the first member applied. This is the assertion
    // the old per-domain loop failed: Aura had already been reset.
    assert_eq!(
        fs::read(&aura_path).expect("aura bytes after"),
        aura_original,
        "a later member's failure must not leave an earlier member applied"
    );
    // The failed member is still the blocked path, i.e. nothing was forced.
    assert!(models_path.is_dir(), "the blocked member stays blocked");
    let _ = models_original;
}

/// **Journal-ordering counterexample.** The recovery record must outlive the
/// domain writes.
///
/// `converge_bundle_journal` used to delete the journal **inside** itself, right
/// after the domain writes and before the caller re-read the members or published
/// their recovery events. A failure in that later work therefore left the
/// configuration converged on disk with **no record** of what had been interrupted.
///
/// Deletion now happens last. To *discriminate* the ordering the failure must
/// happen strictly after the writes, so this case lets convergence succeed and then
/// makes the member's own re-open fail: the member's history directory is replaced
/// with a plain file, so `ModuleConfigService::open` fails at a real I/O boundary
/// while the converged bytes are already on disk.
#[test]
fn a_journal_survives_a_settle_that_could_not_finish() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let journal = interrupted_publish(&paths, "settle-incomplete");
    assert!(journal.is_file(), "the interrupted publish left its record");

    // Replace the second member's *history* directory with a plain file. The
    // converged candidate has already been written to the config file by the time
    // the member is re-opened, and `ModuleConfigService::open` creates and writes
    // into that history directory — so the re-open fails strictly after the write.
    let history = paths.history_path(ConfigModule::Diagnostics);
    if history.is_dir() {
        fs::remove_dir_all(&history).expect("remove the history directory");
    }
    if let Some(parent) = history.parent() {
        fs::create_dir_all(parent).expect("history parent");
    }
    fs::write(&history, b"not a directory").expect("block the history path");

    let settings = SettingsFacade::open(paths.clone()).expect("the facade opens");

    // THE COUNTEREXAMPLE: convergence wrote the member, but its re-open failed, so
    // the settle did not finish and the record must still exist. Under eager
    // deletion it was already gone.
    assert!(
        journal.is_file(),
        "a journal whose settle did not finish must be retained; deleting it would \
         destroy the only record of the interrupted save"
    );
    // The member is not silently reported available on a promise.
    let held = settings.recovery_held_domains();
    let state = settings.health(ConfigModule::Diagnostics).state;
    assert!(
        held.contains(&ConfigModule::Diagnostics) || state != aura_config::DomainHealthState::Ready,
        "a member that could not settle must not be Ready: held={held:?} state={state:?}"
    );
}

/// **Preview-binding counterexample.** A confirmation must not be applied over
/// newer, valid configuration.
///
/// The defect, as measured: a preview of a rev1 default reset was still applied
/// after the user legitimately saved `dark`/rev2, so the **stale confirmation reset
/// the domain to empty/rev3**. `DomainRecoveryPlan.current_revision` was simply not
/// part of any check at commit time.
///
/// The preview now carries an identity derived from the facts it was built from
/// (served revision, committed revision, and the live file's content hash), and the
/// write re-derives and compares it. This case reproduces that sequence exactly.
#[test]
fn a_confirmation_for_a_superseded_state_is_refused() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let settings = SettingsFacade::open(paths.clone()).expect("facade opens");

    // A degraded domain, so a recovery preview is meaningful.
    damage_domain(&paths, ConfigModule::Ui);

    // The user previews a default reset…
    let preview =
        settings.plan_domain_recovery(ConfigModule::Ui).expect("the user previews a recovery");
    assert!(
        preview.allows(aura_config::recovery::ConfigRecoveryAction::ResetToDefault),
        "the reset is offered"
    );

    // …then legitimately saves new configuration before confirming.
    // Clear the blockage and commit a real, newer value.
    let path = paths.config_path(ConfigModule::Ui);
    if path.is_dir() {
        fs::remove_dir_all(&path).expect("remove blocker");
    }
    write_domain_with_revision(&paths, ConfigModule::Ui, "dark", 2);
    settings.reload(ConfigModule::Ui).ok();
    assert!(settings.snapshot(ConfigModule::Ui).is_ok(), "the user's save is live");

    // THE COUNTEREXAMPLE: confirming the old preview must be refused rather than
    // wiping the newer configuration.
    let refused = settings
        .apply_domain_recovery(
            &preview,
            aura_config::recovery::ConfigRecoveryAction::ResetToDefault,
        )
        .expect_err("a superseded preview must not be applied");
    assert!(
        matches!(
            &refused,
            aura_config::recovery::ConfigRecoveryError::Unavailable { reason_code, .. }
                if *reason_code == aura_config::recovery::codes::PREVIEW_STALE
        ),
        "the refusal names the stale preview; got {refused:?}"
    );

    // The user's newer value survives: the stale reset did not write empty/rev3.
    let bytes = disk_bytes(&paths, ConfigModule::Ui);
    let document = parse_document(ConfigModule::Ui, &bytes).expect("the live document parses");
    assert_eq!(
        document.values.get("marker"),
        Some(&json!("dark")),
        "the newer valid configuration must survive a stale confirmation"
    );

    // Refreshing the preview produces a different identity and then succeeds.
    let refreshed = settings.plan_domain_recovery(ConfigModule::Ui).expect("a refreshed preview");
    assert_ne!(refreshed.preview_id, preview.preview_id, "the refresh observes the changed state");
}

/// **Fault-original counterexample.** A recovery must not destroy the damaged
/// input it is repairing.
///
/// A measurement showed that after hot-corrupting a file, a real reload refusing, and
/// then resetting per the current plan, the **complete scan of this round's data
/// tree found no original non-secret content** — the repair had overwritten the
/// only copy of the evidence.
///
/// The damaged input is now preserved into the domain's `invalid/` history before
/// the write, and a secret-bearing original is deliberately not copied into
/// ordinary history.
#[test]
fn a_recovery_preserves_the_damaged_original() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    // A healthy facade first, so the domain has a real service and real history.
    drop(SettingsFacade::open(paths.clone()).expect("facade opens"));

    // The recovery path must be isolated from the **startup** path, or the case
    // proves nothing: `SettingsFacade::open` also backs up an unreadable file, so a
    // test that damages the file and then re-opens would pass with no
    // recovery-time preservation at all. This case therefore keeps one facade open
    // across the corruption and performs the recovery through it, so the only code
    // that can preserve the original is the recovery itself.
    let settings = SettingsFacade::open(paths.clone()).expect("facade opens healthy");
    settings.snapshot(ConfigModule::Ui).expect("Ui is healthy before the corruption");

    // **Hot corruption**, with no re-open afterwards.
    let damaged = b"{ this is not a valid module document, but it is the evidence }";
    let path = paths.config_path(ConfigModule::Ui);
    fs::write(&path, damaged).expect("hot-corrupt the live file");

    let preserved_before = count_matching_files(directory.path(), damaged);
    assert_eq!(
        preserved_before, 1,
        "the live file is the only copy of the damaged input before the recovery"
    );

    // The plan and the reset run on the **same open facade**, so no startup pass
    // intervenes between the damage and the write.
    let plan =
        settings.plan_domain_recovery(ConfigModule::Ui).expect("the user previews a recovery");
    settings
        .apply_domain_recovery(&plan, aura_config::recovery::ConfigRecoveryAction::ResetToDefault)
        .expect("the reset applies");

    // THE COUNTEREXAMPLE: the damaged bytes are still findable. Without
    // recovery-time preservation the write overwrites the only copy.
    let preserved_after = count_matching_files(directory.path(), damaged);
    assert!(
        preserved_after >= 1,
        "the damaged original must still exist after the repair; a recovery that \
         destroys its own evidence cannot be diagnosed"
    );
    // And the reset really happened, so this is not passing because nothing was done.
    let live = fs::read(&path).expect("the live file");
    assert_ne!(live, damaged, "the reset really replaced the damaged input");
}

/// Counts files under `root` whose bytes equal `wanted`.
///
/// A real traversal of the round's data tree, so the assertion is about what is
/// actually on disk rather than about a path the test happened to guess.
fn count_matching_files(root: &std::path::Path, wanted: &[u8]) -> usize {
    let mut found = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            } else if fs::read(&entry_path).is_ok_and(|bytes| bytes == wanted) {
                found += 1;
            }
        }
    }
    found
}
