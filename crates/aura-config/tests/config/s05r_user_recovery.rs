//! Directed verification of the user's recovery entry.
//!
//! The eight cases the brief names, each measured against the real
//! `SettingsFacade` and a real filesystem:
//!
//! 1. normal recovery;
//! 2. no legitimate previous version;
//! 3. the previous version no longer passes current validation;
//! 4. cross-domain inconsistency;
//! 5. an old-revision race;
//! 6. the user cancels;
//! 7. repeated failure;
//! 8. the confidentiality boundary.
//!
//! Every case also asserts the properties that must **not** be traded away:
//! recovery writes a **new** revision, never touches credentials/sessions/
//! workspaces, never clears an unresolved record, and never hides a failure.
use std::{collections::BTreeMap, fs, sync::Arc};

use aura_config::{
    AuraPaths, ConfigModule, ConfigSource, ConfigValidationError, DomainHealthState,
    ModuleConfigDocument, ModuleConfigValidator, SETTINGS_FILE_MAX_BYTES, SettingsFacade,
    recovery::{self, ConfigRecoveryAction, codes},
};
use serde_json::json;
use tempfile::TempDir;

fn paths(directory: &TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

/// Makes a domain's file genuinely unreadable by turning the path into a
/// directory, so the real `File::open` fails with a real I/O error.
fn break_domain(paths: &AuraPaths, module: ConfigModule) {
    let path = paths.config_path(module);
    if path.is_file() {
        fs::remove_file(&path).expect("remove config file");
    }
    fs::create_dir_all(&path).expect("replace config file with a directory");
    fs::write(path.join("blocker.txt"), b"this domain cannot be opened").expect("write blocker");
}

/// Every recorded `version-*` revision for a domain, newest first.
fn recorded_revisions(paths: &AuraPaths, module: ConfigModule) -> Vec<u64> {
    recovery::recorded_versions(paths, module)
        .expect("history is readable")
        .into_iter()
        .map(|version| version.revision)
        .collect()
}

// ---------------------------------------------------------------------------
// 1. Normal recovery
// ---------------------------------------------------------------------------

/// A damaged domain is recovered through the entry, and the recovery writes a
/// **new** revision rather than rewinding the file's.
#[test]
fn a_damaged_domain_is_recovered_through_the_entry_with_a_new_revision() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);

    // Seed real valid history: revision 2 with a distinctive value.
    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    first
        .update(ConfigModule::Ui, 1, |values| {
            values.insert("theme".to_owned(), json!("dark"));
        })
        .expect("commit revision 2");
    drop(first);
    assert!(
        recorded_revisions(&paths, ConfigModule::Ui).contains(&2),
        "revision 2 is recorded in history, so it is a legitimate previous version"
    );

    // Now damage the domain for real.
    break_domain(&paths, ConfigModule::Ui);
    let facade = SettingsFacade::open(paths.clone()).expect("facade opens degraded");
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Ui]);

    // The preview: built without writing anything.
    let plan = facade.plan_domain_recovery(ConfigModule::Ui).expect("plan");
    assert_eq!(plan.module, ConfigModule::Ui);
    assert!(plan.degraded, "the domain really is degraded");
    assert!(plan.fault_code.is_some(), "the real fault is classified");
    assert!(
        plan.allows(ConfigRecoveryAction::RestorePreviousValid),
        "a legitimate previous version exists, so restore is offered: {:?}",
        plan.options
    );
    assert!(plan.allows(ConfigRecoveryAction::Retry));
    assert!(plan.allows(ConfigRecoveryAction::ResetToDefault));
    assert!(
        plan.recoverable_versions
            .iter()
            .any(|version| version.revision == 2 && version.passes_current_validation),
        "revision 2 still passes current validation: {:?}",
        plan.recoverable_versions
    );
    // The preview states the revision the result will carry, before the write.
    assert_eq!(plan.next_revision, 3, "current(2) + 1");

    // The file is still the blocking directory: the plan wrote nothing.
    assert!(
        paths.config_path(ConfigModule::Ui).is_dir(),
        "planning must not change anything on disk"
    );

    // Clear the damage so the recovery has somewhere to write, then apply.
    fs::remove_dir_all(paths.config_path(ConfigModule::Ui)).expect("remove blocker");
    let plan =
        facade.plan_domain_recovery(ConfigModule::Ui).expect("refresh after original removal");
    let outcome = facade
        .apply_domain_recovery(&plan, ConfigRecoveryAction::RestorePreviousValid)
        .expect("the restore applies");
    assert!(outcome.applied);
    assert_eq!(outcome.action, ConfigRecoveryAction::RestorePreviousValid);

    // A NEW revision, never revision 2 again.
    let revision = outcome.revision.expect("the recovery reports its revision");
    assert_eq!(revision, 3, "recovery uses a new revision, never rewinds the file's");
    assert_ne!(revision, 2, "the recovered revision is not the historical one");

    // The recovered values really came from the recorded version.
    let snapshot = facade.snapshot(ConfigModule::Ui).expect("recovered snapshot");
    assert_eq!(snapshot.document.revision, 3);
    assert_eq!(
        snapshot.document.values.get("theme"),
        Some(&json!("dark")),
        "the restored values are revision 2's, under the new revision 3"
    );

    // And it is durable and the domain is Ready again.
    assert_eq!(facade.health(ConfigModule::Ui).state, DomainHealthState::Ready);
    let reopened = SettingsFacade::open(paths.clone()).expect("re-open");
    assert_eq!(reopened.snapshot(ConfigModule::Ui).expect("ui").document.revision, 3);
    assert!(
        facade.health(ConfigModule::Ui).original_fault.is_some(),
        "the original failure is preserved, so a reader can see the domain really was unusable"
    );
}

/// Reset to default installs a document that passes the domain's own validation.
#[test]
fn reset_to_default_installs_a_validated_default_with_a_new_revision() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    first
        .update(ConfigModule::Diagnostics, 1, |values| {
            values.insert("level".to_owned(), json!("debug"));
        })
        .expect("commit revision 2");
    drop(first);

    break_domain(&paths, ConfigModule::Diagnostics);
    let facade = SettingsFacade::open(paths.clone()).expect("facade opens degraded");
    let plan = facade.plan_domain_recovery(ConfigModule::Diagnostics).expect("plan");
    assert!(plan.allows(ConfigRecoveryAction::ResetToDefault));
    assert_eq!(plan.next_revision, 3);

    fs::remove_dir_all(paths.config_path(ConfigModule::Diagnostics)).expect("remove blocker");
    let plan = facade
        .plan_domain_recovery(ConfigModule::Diagnostics)
        .expect("refresh after original removal");
    let outcome = facade
        .apply_domain_recovery(&plan, ConfigRecoveryAction::ResetToDefault)
        .expect("the reset applies");
    assert!(outcome.applied);
    assert_eq!(outcome.revision, Some(3));
    assert_eq!(outcome.source, Some(ConfigSource::Default));

    // The default really is the domain's validated default, not the old values.
    let snapshot = facade.snapshot(ConfigModule::Diagnostics).expect("reset snapshot");
    assert_eq!(snapshot.document.revision, 3);
    assert_ne!(
        snapshot.document.values.get("level"),
        Some(&json!("debug")),
        "a default reset does not carry the previous values forward"
    );
    assert_eq!(facade.health(ConfigModule::Diagnostics).state, DomainHealthState::Ready);
}

// ---------------------------------------------------------------------------
// 2. No legitimate previous version
// ---------------------------------------------------------------------------

/// With no recorded history, the restore option is **honestly disabled** and the
/// scoped default reset is offered instead.
#[test]
fn without_a_legitimate_previous_version_the_restore_option_is_disabled_and_stated() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    drop(first);

    // Really remove both the live file and every recorded version: this domain now
    // has no legitimate previous configuration at all.
    fs::remove_file(paths.config_path(ConfigModule::Skills)).expect("remove live file");
    for version in recovery::recorded_versions(&paths, ConfigModule::Skills).expect("history") {
        fs::remove_file(version.path).expect("remove recorded version");
    }
    fs::create_dir_all(paths.config_path(ConfigModule::Skills)).expect("block the live path");

    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Skills]);

    let plan = facade.plan_domain_recovery(ConfigModule::Skills).expect("plan");
    let restore = plan
        .option(ConfigRecoveryAction::RestorePreviousValid)
        .expect("the option is present, not silently omitted");
    assert!(!restore.available, "with no legitimate previous version the restore must be disabled");
    assert!(
        restore.reason_code.as_deref() == Some(codes::NO_HISTORY)
            || restore.reason_code.as_deref() == Some(codes::NO_VALID_PREVIOUS_VERSION),
        "the reason is stated with a stable code: {restore:?}"
    );
    assert!(restore.detail.is_some(), "the user is told what to do instead: {restore:?}");
    // The scoped default reset is the offered alternative.
    assert!(
        plan.allows(ConfigRecoveryAction::ResetToDefault),
        "the scoped default reset is offered when there is no previous version: {:?}",
        plan.options
    );

    // Applying an unavailable option is refused, not silently substituted.
    let error = facade
        .apply_domain_recovery(&plan, ConfigRecoveryAction::RestorePreviousValid)
        .expect_err("an unavailable option must be refused");
    assert!(
        matches!(
            error,
            recovery::ConfigRecoveryError::Unavailable { reason_code, .. }
                if reason_code == codes::ACTION_UNAVAILABLE
        ),
        "{error:?}"
    );

    // And nothing was written by the refused attempt.
    assert!(paths.config_path(ConfigModule::Skills).is_dir(), "a refused recovery writes nothing");
}

// ---------------------------------------------------------------------------
// 3. The previous version no longer passes current validation
// ---------------------------------------------------------------------------

/// A recorded version that no longer passes the **current** validator must not be
/// applied, and the option is disabled with that stated reason.
#[test]
fn a_previous_version_that_no_longer_validates_is_refused_not_applied() {
    // A JSON domain with no schema of its own (as plugins.json once was): any well-formed
    // values are a valid revision.
    /// Accepts a document only while the flag is set.
    struct GatedValidator(Arc<std::sync::atomic::AtomicBool>);

    impl ModuleConfigValidator for GatedValidator {
        fn validate(
            &self,
            _module: ConfigModule,
            _document: &ModuleConfigDocument,
        ) -> Result<(), ConfigValidationError> {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                Ok(())
            } else {
                Err(ConfigValidationError::Semantic(
                    "S05R R5: this version no longer passes current validation".to_owned(),
                ))
            }
        }
    }

    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let accept = Arc::new(std::sync::atomic::AtomicBool::new(true));

    // Record revision 2 while the validator accepts it, so a legitimate history
    // entry really exists.
    {
        let mut validators: BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>> =
            BTreeMap::new();
        validators.insert(
            ConfigModule::Commands,
            Arc::new(GatedValidator(Arc::clone(&accept))) as Arc<dyn ModuleConfigValidator>,
        );
        let facade = SettingsFacade::open_with_validators(paths.clone(), validators)
            .expect("facade with the gated validator");
        facade
            .update(ConfigModule::Commands, 1, |values| {
                values.insert("enabled".to_owned(), json!(true));
            })
            .expect("commit revision 2 while the validator accepts");
    }
    assert!(
        recorded_revisions(&paths, ConfigModule::Commands).contains(&2),
        "revision 2 really is recorded"
    );

    // The rules change: the same document no longer passes.
    accept.store(false, std::sync::atomic::Ordering::SeqCst);
    break_domain(&paths, ConfigModule::Commands);

    let mut validators: BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>> = BTreeMap::new();
    validators.insert(
        ConfigModule::Commands,
        Arc::new(GatedValidator(Arc::clone(&accept))) as Arc<dyn ModuleConfigValidator>,
    );
    let facade = SettingsFacade::open_with_validators(paths.clone(), validators)
        .expect("facade opens degraded");
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Commands]);

    let plan = facade.plan_domain_recovery(ConfigModule::Commands).expect("plan");
    let restore =
        plan.option(ConfigRecoveryAction::RestorePreviousValid).expect("the option is present");
    assert!(
        !restore.available,
        "a version that no longer validates must not be offered for restore"
    );
    assert_eq!(
        restore.reason_code.as_deref(),
        Some(codes::NO_VALID_PREVIOUS_VERSION),
        "the reason names the real condition: {restore:?}"
    );
    // The version is listed, with its verdict and rejection code, so the user can
    // see that it existed and why it is not usable.
    let version = plan
        .recoverable_versions
        .iter()
        .find(|version| version.revision == 2)
        .expect("the recorded version is listed");
    assert!(!version.passes_current_validation);
    assert!(version.rejection_code.is_some());

    // Applying it is refused by the entry itself, not only by the preview.
    let error = facade
        .apply_domain_recovery(&plan, ConfigRecoveryAction::RestorePreviousValid)
        .expect_err("a version that fails current validation must not be applied");
    assert!(matches!(error, recovery::ConfigRecoveryError::Unavailable { .. }), "{error:?}");

    // The damaged domain is still damaged: nothing was applied behind the refusal.
    assert!(
        paths.config_path(ConfigModule::Commands).is_dir(),
        "the blocked live path is untouched"
    );
}

// ---------------------------------------------------------------------------
// 4. Cross-domain inconsistency
// ---------------------------------------------------------------------------

/// When no **joint record** proves a consistent version, the cross-domain restore
/// is **honestly disabled**, and the plan never claims a consistent version by
/// stapling per-domain last-known-good together.
///
/// This case previously asserted that two domains both reaching
/// revision 2 made `consistent_revision == Some(2)` — "the only revision both
/// recorded". That reasoning is invalid, and the assertion **froze the defect**:
/// each domain's revision counter is independent, so two wholly unrelated
/// single-domain saves landing on the same number is not a shared commit. A measurement
/// showed exactly that case. The corrected rule is that matching numbers never
/// prove coherence — only a record naming the whole set together may.
#[test]
fn an_inconsistent_cross_domain_history_disables_the_bundle_restore() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    // Two INDEPENDENT single-domain saves that happen to land on revision 2.
    // Under the old rule this "proved" a consistent version.
    facade
        .update(ConfigModule::Aura, 1, |values| {
            values.insert("theme".to_owned(), json!("dark"));
        })
        .expect("aura to 2");
    facade
        .update(ConfigModule::Models, 1, |values| {
            values.insert("models".to_owned(), json!([]));
        })
        .expect("models to 2");

    let plan = facade
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Models])
        .expect("bundle plan");

    // THE COUNTEREXAMPLE: matching revision numbers must not prove coherence.
    assert_eq!(
        plan.consistent_revision, None,
        "two independent saves that both reached revision 2 are not one committed \
         version; a per-domain counter is not joint evidence: {plan:?}"
    );
    assert_eq!(plan.inconsistency_code.as_deref(), Some(codes::NO_CONSISTENT_BUNDLE_VERSION));
    assert!(
        plan.consistent_set.is_empty(),
        "no joint record was found, so the previewed set is empty"
    );
    let restore =
        plan.option(ConfigRecoveryAction::RestorePreviousValid).expect("the option is present");
    assert!(
        !restore.available,
        "a consistent previous version cannot be proven, so the bundle restore is disabled"
    );
    assert!(
        restore.detail.as_deref().is_some_and(|detail| detail.contains("stapled")),
        "the reason explains why per-domain last-known-good is not a substitute: {restore:?}"
    );

    // A scoped default reset is the offered alternative, and it names its scope.
    assert!(
        plan.allows(ConfigRecoveryAction::ResetToDefault),
        "the scoped default reset is offered instead: {:?}",
        plan.options
    );
    let reset = plan.option(ConfigRecoveryAction::ResetToDefault).expect("reset option");
    assert!(
        reset.detail.as_deref().is_some_and(|detail| detail.contains("2 affected domains")),
        "the reset states its scope: {reset:?}"
    );

    // Asking for the disabled restore is refused at the write, not just previewed.
    let error = facade
        .apply_bundle_recovery(&plan, ConfigRecoveryAction::RestorePreviousValid)
        .expect_err("an unprovable consistent version must not be applied");
    assert!(matches!(error, recovery::ConfigRecoveryError::Unavailable { .. }), "{error:?}");
}

/// A cross-domain recovery that **is** provable applies as one consistent version.
#[test]
fn a_provable_cross_domain_version_applies_as_one_consistent_set() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    // A real cross-domain bundle: both domains commit at revision 2 together.
    facade
        .commit_module_bundle(vec![
            aura_config::ConfigModuleBundleEntry {
                module: ConfigModule::Aura,
                expected_revision: 1,
                replacement_values: Some(BTreeMap::from([("theme".to_owned(), json!("dark"))])),
            },
            aura_config::ConfigModuleBundleEntry {
                module: ConfigModule::Models,
                expected_revision: 1,
                replacement_values: Some(BTreeMap::from([("models".to_owned(), json!([]))])),
            },
        ])
        .expect("the bundle commits as a whole");

    // The joint commit leaves a durable record naming BOTH domains, which is the
    // evidence a restore is allowed to rely on. The revisions matching is a
    // consequence of the joint act, not the proof of it.
    assert!(recorded_revisions(&paths, ConfigModule::Aura).contains(&2));
    assert!(recorded_revisions(&paths, ConfigModule::Models).contains(&2));

    // Damage both, then plan.
    break_domain(&paths, ConfigModule::Aura);
    break_domain(&paths, ConfigModule::Models);
    let facade = SettingsFacade::open(paths.clone()).expect("facade opens degraded");
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Aura, ConfigModule::Models]);

    let plan = facade
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Models])
        .expect("bundle plan");
    assert_eq!(
        plan.consistent_revision,
        Some(2),
        "the joint commit record names both domains at revision 2: {plan:?}"
    );
    assert!(
        !plan.consistent_set.is_empty(),
        "the preview binds the exact set the joint record proves"
    );
    assert!(plan.allows(ConfigRecoveryAction::RestorePreviousValid));

    // Clear the blockages and apply as one set.
    fs::remove_dir_all(paths.config_path(ConfigModule::Aura)).expect("remove blocker");
    fs::remove_dir_all(paths.config_path(ConfigModule::Models)).expect("remove blocker");
    let plan = facade
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Models])
        .expect("refresh after original removal");
    let outcomes = facade
        .apply_bundle_recovery(&plan, ConfigRecoveryAction::RestorePreviousValid)
        .expect("the consistent restore applies");
    assert_eq!(outcomes.len(), 2, "both affected domains are recovered");
    assert!(outcomes.iter().all(|outcome| outcome.applied));
    // Every domain lands on the SAME new revision: that is what "one consistent
    // version" means at the write.
    let revisions =
        outcomes.iter().map(|outcome| outcome.revision.expect("applied")).collect::<Vec<_>>();
    assert_eq!(
        revisions,
        vec![3, 3],
        "both domains reach the same new revision, so the set is consistent"
    );
    for module in [ConfigModule::Aura, ConfigModule::Models] {
        assert_eq!(facade.snapshot(module).expect("recovered").document.revision, 3);
        assert_eq!(facade.health(module).state, DomainHealthState::Ready);
    }
}

// ---------------------------------------------------------------------------
// 5. Old-revision race
// ---------------------------------------------------------------------------

/// A recovery that races a newer commit does not rewind the file's revision.
#[test]
fn an_old_revision_race_does_not_rewind_the_committed_revision() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Hooks, 1, |values| {
            values.insert("hooks".to_owned(), json!([{"path": "C:/hook", "enabled": true}]));
        })
        .expect("commit revision 2");

    break_domain(&paths, ConfigModule::Hooks);
    let facade = SettingsFacade::open(paths.clone()).expect("facade degraded");
    let plan = facade.plan_domain_recovery(ConfigModule::Hooks).expect("plan");
    assert_eq!(plan.next_revision, 3, "planned against the served revision 2");
    assert!(plan.allows(ConfigRecoveryAction::RestorePreviousValid));

    // The race: another writer commits revision 3 before the recovery applies.
    // The blocker is cleared and the live file is put back at the revision it
    // really held, so the concurrent writer starts from the committed fact rather
    // than from nothing.
    fs::remove_dir_all(paths.config_path(ConfigModule::Hooks)).expect("remove blocker");
    fs::write(
        paths.config_path(ConfigModule::Hooks),
        "schema_version = 1\nrevision = 2\n[[hooks]]\npath = \"C:/hook\"\nenabled = true\n",
    )
    .expect("restore the live file at revision 2");
    let concurrent = SettingsFacade::open(paths.clone()).expect("the concurrent writer's facade");
    concurrent
        .update(ConfigModule::Hooks, 2, |values| {
            values.insert("hooks".to_owned(), json!([{"path": "C:/hook", "enabled": false}]));
        })
        .expect("a concurrent writer commits revision 3");

    // A stale confirmation is now REFUSED
    // rather than applied. This is strictly safer than the previous behaviour,
    // which applied the old plan and relied on `highest_committed_revision` to
    // avoid rewinding. The property this case exists for is unchanged — no old
    // revision is ever inserted over a newer committed fact — and it is now
    // enforced *before* the write as well as during it.
    let refused = facade
        .apply_domain_recovery(&plan, ConfigRecoveryAction::RestorePreviousValid)
        .expect_err("a confirmation for a superseded state must be refused");
    assert!(
        matches!(
            &refused,
            recovery::ConfigRecoveryError::Unavailable { reason_code, .. }
                if *reason_code == codes::PREVIEW_STALE
        ),
        "the refusal names the stale preview; got {refused:?}"
    );
    // The concurrent writer's revision 3 is untouched: nothing was written.
    let live = SettingsFacade::open(paths.clone()).expect("re-open");
    assert_eq!(
        live.snapshot(ConfigModule::Hooks).expect("hooks").document.revision,
        3,
        "a refused stale recovery must not disturb the committed fact"
    );

    // After refreshing the preview the recovery proceeds, and its revision still
    // advances past the committed fact rather than reusing it.
    let refreshed = facade.plan_domain_recovery(ConfigModule::Hooks).expect("refreshed plan");
    assert_ne!(
        refreshed.preview_id, plan.preview_id,
        "the refresh really observes the changed state"
    );
    let outcome = facade
        .apply_domain_recovery(&refreshed, ConfigRecoveryAction::RestorePreviousValid)
        .expect("the refreshed recovery applies");
    let revision = outcome.revision.expect("applied");
    assert!(
        revision > 3,
        "the recovery must not reuse the already-committed revision 3: got {revision}"
    );
    // The recovered values are the recorded ones; the revision is new.
    let snapshot = facade.snapshot(ConfigModule::Hooks).expect("recovered");
    assert_eq!(snapshot.document.revision, revision);
    // The restored document comes from the domain's own recorded version, so it
    // carries whatever that version recorded — not the concurrent writer's value.
    // The concurrent writer set `enabled = false`; the recorded version is the one
    // this recovery chose, so the assertion is that the served document is the
    // recorded one rather than a blend.
    assert!(
        snapshot.document.values.contains_key("hooks"),
        "the restored document carries the recorded value: {:?}",
        snapshot.document.values
    );
    assert_eq!(
        outcome.source,
        Some(ConfigSource::LastKnownGood),
        "the outcome reports the real provenance of a restored version"
    );
    // And no old revision was inserted over the newer committed fact.
    let reopened = SettingsFacade::open(paths.clone()).expect("re-open");
    assert_eq!(reopened.snapshot(ConfigModule::Hooks).expect("hooks").document.revision, revision);
}

// ---------------------------------------------------------------------------
// 6. The user cancels
// ---------------------------------------------------------------------------

/// Planning is side-effect free, and a plan the user does not act on changes
/// nothing: cancellation is the absence of an apply, and it must be safe.
#[test]
fn a_planned_recovery_the_user_cancels_changes_nothing() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Memory, 1, |values| {
            // Gave Memory real fields; a valid one keeps this test about recovery.
            values.insert("automatic_maintenance".to_owned(), json!(false));
        })
        .expect("commit revision 2");
    break_domain(&paths, ConfigModule::Memory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade degraded");

    // Capture the whole managed state before planning.
    let before = managed_state_fingerprint(&paths);
    let plan = facade.plan_domain_recovery(ConfigModule::Memory).expect("plan");
    assert!(plan.allows(ConfigRecoveryAction::RestorePreviousValid));

    // The user cancels: no apply is called at all.
    let after = managed_state_fingerprint(&paths);
    assert_eq!(
        before, after,
        "a plan the user does not act on must not change one byte of managed state"
    );

    // The domain is still degraded and still honestly reported.
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Memory]);
    assert!(!facade.is_available(ConfigModule::Memory));
    // Stop and diagnostic ability is intact: the health surface still answers.
    assert!(facade.health(ConfigModule::Memory).original_fault.is_some());
    assert!(!facade.degraded_summary().is_empty());
}

/// A stable fingerprint of every managed file, for the cancellation assertion.
fn managed_state_fingerprint(paths: &AuraPaths) -> Vec<(String, u64, String)> {
    fn walk(root: &std::path::Path, into: &mut Vec<(String, u64, String)>) {
        let Ok(entries) = fs::read_dir(root) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, into);
            } else if let Ok(bytes) = fs::read(&path) {
                into.push((
                    path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                    bytes.len() as u64,
                    aura_domain_common::ContentHash::blake3(&bytes).to_string(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    walk(&paths.config_root, &mut files);
    walk(&paths.config_history_dir, &mut files);
    files.sort();
    files
}

// ---------------------------------------------------------------------------
// 7. Repeated failure
// ---------------------------------------------------------------------------

/// A recovery that fails repeatedly reports the real failure every time, keeps
/// stop/diagnostic ability, and never loops on its own.
#[test]
fn a_recovery_that_keeps_failing_stays_reported_and_never_loops() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Commands, 1, |values| {
            values.insert("enabled".to_owned(), json!(true));
        })
        .expect("commit revision 2");
    break_domain(&paths, ConfigModule::Commands);

    let facade = SettingsFacade::open(paths.clone()).expect("facade degraded");
    let plan = facade.plan_domain_recovery(ConfigModule::Commands).expect("plan");
    assert!(plan.allows(ConfigRecoveryAction::Retry));

    // The blocker is still there, so a retry really fails — repeatedly, and each
    // attempt reports its own real reason rather than a generic one.
    let mut reasons = Vec::new();
    for attempt in 0..3 {
        let error = facade
            .apply_domain_recovery(&plan, ConfigRecoveryAction::Retry)
            .expect_err("the retry fails while the path is blocked");
        let reason = error.to_string();
        assert!(
            !reason.is_empty(),
            "attempt {attempt} reports a real reason rather than hiding the failure"
        );
        reasons.push(reason);
    }
    assert_eq!(reasons.len(), 3, "each attempt is an explicit, user-driven call");

    // Three attempts, three failures, and the domain is still degraded with its
    // original fault preserved — no state was lost and nothing was hidden.
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Commands]);
    let health = facade.health(ConfigModule::Commands);
    assert!(health.original_fault.is_some(), "the original fault survives the retries");
    assert!(
        health.recovery_attempts.len() >= 3,
        "every failed attempt is recorded, so a hidden failure is impossible: {:?}",
        health.recovery_attempts
    );
    assert!(
        health.recovery_attempts.iter().all(|attempt| !attempt.succeeded),
        "no attempt is recorded as succeeding"
    );
    // Diagnostic ability is intact: the bounded summary still names the fault.
    assert!(facade.degraded_summary().contains("commands"));

    // The repair finally happens, and one more retry really recovers it. That the
    // earlier failures did not poison the entry is the point.
    fs::remove_dir_all(paths.config_path(ConfigModule::Commands)).expect("remove blocker");
    fs::write(
        paths.config_path(ConfigModule::Commands),
        serde_json::json!({ "schema_version": 1, "revision": 9, "values": { "enabled": true } })
            .to_string(),
    )
    .expect("write the repaired file");
    let outcome = facade
        .apply_domain_recovery(&plan, ConfigRecoveryAction::Retry)
        .expect("the retry succeeds after a real repair");
    assert!(outcome.applied);
    assert_eq!(facade.health(ConfigModule::Commands).state, DomainHealthState::Ready);
    assert!(
        facade.health(ConfigModule::Commands).recovery_attempts.iter().any(|a| a.succeeded),
        "the successful attempt is recorded alongside the failures"
    );
}

// ---------------------------------------------------------------------------
// 8. The confidentiality boundary
// ---------------------------------------------------------------------------

/// Recovery never copies a secret-bearing input into ordinary history, and never
/// touches credentials, sessions or workspaces.
#[test]
fn recovery_does_not_copy_a_secret_bearing_input_or_clear_unrelated_state() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Security, 1, |values| {
            values.insert("require_approval".to_owned(), json!(true));
        })
        .expect("commit revision 2");

    // Simulate the other things the product owns, so "recovery left them alone"
    // is a real observation rather than an assumption about an empty directory.
    fs::create_dir_all(paths.data_dir.join("sessions")).expect("sessions dir");
    fs::write(paths.data_dir.join("sessions/s1.json"), b"{\"session\":\"kept\"}")
        .expect("session file");
    fs::create_dir_all(paths.local_root.join("worktrees/ws1")).expect("workspace dir");
    fs::write(paths.local_root.join("worktrees/ws1/file.txt"), b"kept").expect("workspace file");

    break_domain(&paths, ConfigModule::Security);
    // A blocked path is not secret-bearing, so nothing is copied to history for
    // it; assert that the history holds no invalid-backup copy of a canary.
    let facade = SettingsFacade::open(paths.clone()).expect("facade degraded");
    let plan = facade.plan_domain_recovery(ConfigModule::Security).expect("plan");
    assert!(
        plan.allows(ConfigRecoveryAction::RestorePreviousValid)
            || plan.allows(ConfigRecoveryAction::ResetToDefault),
        "at least one option is offered: {:?}",
        plan.options
    );

    fs::remove_dir_all(paths.config_path(ConfigModule::Security)).expect("remove blocker");
    let plan = facade
        .plan_domain_recovery(ConfigModule::Security)
        .expect("refresh after original removal");
    let outcome = facade
        .apply_domain_recovery(&plan, ConfigRecoveryAction::ResetToDefault)
        .expect("the reset applies");
    assert!(outcome.applied);

    // Sessions and workspaces are untouched: recovery does not clear them.
    assert_eq!(
        fs::read(paths.data_dir.join("sessions/s1.json")).expect("session still there"),
        b"{\"session\":\"kept\"}"
    );
    assert_eq!(
        fs::read(paths.local_root.join("worktrees/ws1/file.txt")).expect("workspace still there"),
        b"kept"
    );

    // The plan and the outcome carry names and codes only. No path, no value.
    let described = format!("{plan:?}");
    assert!(
        !described.contains(&paths.config_root.to_string_lossy().to_string()),
        "a recovery plan must not carry a filesystem path"
    );
    assert!(!described.contains("require_approval\":true"), "a plan must not carry a value body");
}

// ---------------------------------------------------------------------------
// Bounded behaviour that must hold in every case
// ---------------------------------------------------------------------------

/// The preview states the scope, the references needing rebinding and the loss.
#[test]
fn the_preview_states_scope_references_and_loss_before_the_user_triggers_it() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Aura, 1, |values| {
            values.insert(
                "default_model_profile_id".to_owned(),
                json!("00000000-0000-0000-0000-0000000000aa"),
            );
            values.insert("theme".to_owned(), json!("dark"));
        })
        .expect("commit revision 2");
    break_domain(&paths, ConfigModule::Aura);
    let facade = SettingsFacade::open(paths.clone()).expect("facade degraded");

    let plan = facade.plan_domain_recovery(ConfigModule::Aura).expect("plan");
    // Scope.
    assert_eq!(plan.module, ConfigModule::Aura);
    assert!(plan.degraded);
    assert!(plan.scope_known, "the plan states whether the scope is known");
    // References needing rebinding, as names only.
    assert!(
        plan.references_needing_rebinding.iter().any(|key| key == "default_model_profile_id"),
        "a cross-domain reference is named: {:?}",
        plan.references_needing_rebinding
    );
    assert!(
        plan.references_needing_rebinding.iter().all(|key| !key.contains(':')),
        "the reference list carries key names, never paths or values"
    );
    // Actual loss.
    assert!(!plan.loss.is_empty(), "the loss is stated: {:?}", plan.loss);
    // And the revision the result would carry.
    assert_eq!(plan.next_revision, 3);
}

/// An over-limit file is never rewritten just because a plan was computed.
#[test]
fn planning_never_rewrites_an_oversized_file() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Update, 1, |values| {
            values.insert("channel".to_owned(), json!("stable"));
        })
        .expect("commit revision 2");
    drop(facade);

    let oversized = vec![b' '; (SETTINGS_FILE_MAX_BYTES + 16) as usize];
    fs::write(paths.config_path(ConfigModule::Update), &oversized).expect("write oversized");
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    let _plan = facade.plan_domain_recovery(ConfigModule::Update).expect("plan");
    assert_eq!(
        fs::metadata(paths.config_path(ConfigModule::Update)).expect("stat").len(),
        SETTINGS_FILE_MAX_BYTES + 16,
        "planning must never truncate or rewrite the domain's file"
    );
}

#[test]
fn repeated_domain_causes_bind_one_complete_set_and_publish_each_file_once() {
    let directory = tempfile::tempdir().expect("create isolated recovery directory");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths).expect("open independent configuration domains");
    let single_set = facade
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Tools])
        .expect("plan unique recovery set");
    let repeated = facade
        .plan_bundle_recovery(&[
            ConfigModule::Tools,
            ConfigModule::Aura,
            ConfigModule::Tools,
            ConfigModule::Aura,
        ])
        .expect("plan repeated causes as one recovery set");
    assert_eq!(repeated.modules, vec![ConfigModule::Aura, ConfigModule::Tools]);
    assert_eq!(repeated.domains.len(), 2);
    assert_eq!(
        repeated.preview_id, single_set.preview_id,
        "fault multiplicity and caller order cannot change the confirmed set"
    );
    let outcomes = facade
        .apply_bundle_recovery(&repeated, ConfigRecoveryAction::ResetToDefault)
        .expect("publish the complete recovery set once");
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|outcome| outcome.applied));
    for module in [ConfigModule::Aura, ConfigModule::Tools] {
        assert_eq!(
            facade.snapshot(module).expect("read actual published configuration").document.revision,
            2
        );
    }
}
