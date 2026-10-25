//! Batch B: configuration-domain independence.
//!
//! Every case here uses a **real filesystem boundary**. The usual trick is to
//! replace a domain's config file with a *directory* of the same name: the path
//! still exists, so the facade believes there is a file to load, and the real
//! `File::open` fails with a genuine Windows I/O error. Nothing is simulated and
//! no in-memory snapshot is substituted for a real read.
//!
//! The point of the batch is exactly the counterexample that used to fail: with
//! domain A's load really broken, domain B must still read, modify, and project.

use std::{collections::BTreeMap, fs, sync::Arc};

use aura_config::{
    AuraPaths, ConfigModule, ConfigSource, DomainHealthState, SETTINGS_FILE_MAX_BYTES,
    SettingsFacade, SettingsFacadeError,
};
use aura_domain_common::ContentHash;
use serde_json::json;
use tempfile::TempDir;

fn paths(directory: &TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

/// One valid, switched-on Skill choice: `skills.json` refuses unknown fields.
fn skills_choice() -> serde_json::Value {
    let source = aura_skills::SkillSource::AgentGlobal;
    serde_json::to_value([aura_skills::SkillSetting {
        id: aura_skills::skill_id(source, None, "notes"),
        source,
        workspace_id: None,
        relative: "notes".into(),
        enabled: true,
        trusted_fingerprint: None,
        available_for_subagents: false,
    }])
    .expect("a Skill choice")
}

/// Makes a domain's config file genuinely unreadable by turning the path into a
/// directory. `File::open` on a directory fails on Windows with a real I/O
/// error, so the domain's load boundary is really exercised.
fn break_domain_with_a_directory(paths: &AuraPaths, module: ConfigModule) {
    let path = paths.config_path(module);
    if path.is_file() {
        fs::remove_file(&path).expect("remove config file");
    }
    fs::create_dir_all(&path).expect("replace config file with a directory");
    fs::write(path.join("blocker.txt"), b"this domain cannot be opened").expect("write blocker");
}

#[test]
fn one_domain_that_cannot_be_opened_does_not_take_down_the_others() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);

    // First open every domain normally so real valid files exist on disk.
    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    first
        .update(ConfigModule::Browser, 1, |values| {
            values.insert("home_page".to_owned(), json!("https://example.test/"));
        })
        .expect("seed Browser revision 2");
    drop(first);

    // Now genuinely break two unrelated domains: a TOML one and a JSON one.
    break_domain_with_a_directory(&paths, ConfigModule::Browser);
    break_domain_with_a_directory(&paths, ConfigModule::Commands);

    // The facade still opens. This is the whole point: formerly the first
    // broken domain aborted `open_with_validators` with `?`.
    let facade =
        SettingsFacade::open(paths.clone()).expect("facade opens despite two broken domains");

    assert_eq!(
        facade.degraded_domains(),
        vec![ConfigModule::Commands, ConfigModule::Browser],
        "exactly the broken domains are degraded, in ConfigModule::ALL order"
    );
    for module in ConfigModule::ALL {
        let expected_ready = !matches!(module, ConfigModule::Browser | ConfigModule::Commands);
        assert_eq!(
            facade.health(module).state,
            if expected_ready { DomainHealthState::Ready } else { DomainHealthState::Degraded },
            "{module:?} health"
        );
    }

    // Every unrelated domain still reads a real snapshot with the real values.
    for module in ConfigModule::ALL {
        if matches!(module, ConfigModule::Browser | ConfigModule::Commands) {
            continue;
        }
        let snapshot = facade.snapshot(module).expect("unrelated domain snapshot");
        assert_eq!(snapshot.module, module);
        assert_ne!(snapshot.document.revision, 0, "{module:?} has a real revision");
    }

    // A *degraded* domain reports the real failure, not a fabricated default.
    let error = facade.snapshot(ConfigModule::Browser).expect_err("broken domain is unavailable");
    assert!(
        matches!(
            error,
            SettingsFacadeError::DomainUnavailable { module: ConfigModule::Browser, .. }
        ),
        "the degraded domain reports its own unavailability: {error:?}"
    );
    let health = facade.health(ConfigModule::Browser);
    assert!(health.original_fault.is_some(), "the original failure is kept");
    assert!(health.original_message.is_some(), "the original message is kept");
    assert_eq!(health.revision, None, "a degraded domain claims no revision");

    // A domain that is *ready* still modifies for real, and the change is durable.
    let updated = facade
        .update(ConfigModule::Ui, 1, |values| {
            values.insert("theme".to_owned(), json!("dark"));
        })
        .expect("unrelated domain still accepts a real write");
    assert_eq!(updated.document.revision, 2);
    let reread = SettingsFacade::open(paths.clone()).expect("re-open");
    assert_eq!(
        reread.snapshot(ConfigModule::Ui).expect("ui snapshot").document.revision,
        2,
        "the unrelated write really reached the file"
    );
    // The broken domains are still broken on the second open, so nothing was
    // silently repaired into a fabricated default.
    assert_eq!(reread.degraded_domains(), vec![ConfigModule::Commands, ConfigModule::Browser]);
}

#[test]
fn a_degraded_domain_recovers_in_place_without_restarting_the_facade() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);

    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    first
        .update(ConfigModule::Mcp, 1, |values| {
            values.insert("servers".to_owned(), json!([]));
        })
        .expect("seed Mcp revision 2");
    drop(first);

    break_domain_with_a_directory(&paths, ConfigModule::Mcp);
    let facade = SettingsFacade::open(paths.clone()).expect("facade opens degraded");
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Mcp]);
    assert!(!facade.is_available(ConfigModule::Mcp));

    // Repair the real file so it is valid and strictly newer than the LKG.
    let mcp_path = paths.config_path(ConfigModule::Mcp);
    fs::remove_dir_all(&mcp_path).expect("remove blocking directory");
    let repaired = serde_json::json!({
        "schema_version": 1,
        "revision": 5,
        "servers": []
    })
    .to_string();
    fs::write(&mcp_path, repaired.as_bytes()).expect("write repaired config");

    // `reload` re-opens a domain that has no service at all.
    assert!(facade.reload(ConfigModule::Mcp).expect("re-open the degraded domain"));
    assert_eq!(facade.degraded_domains(), Vec::<ConfigModule>::new());
    assert!(facade.is_available(ConfigModule::Mcp));
    let snapshot = facade.snapshot(ConfigModule::Mcp).expect("recovered snapshot");
    assert_eq!(snapshot.document.revision, 5);

    // The recovery is recorded, and the *original* failure is still there: a
    // reader must be able to see that this domain really was unusable.
    let health = facade.health(ConfigModule::Mcp);
    assert_eq!(health.state, DomainHealthState::Ready);
    assert!(health.original_fault.is_some(), "the original failure is preserved");
    assert!(health.original_message.is_some());
    assert!(
        health.recovery_attempts.iter().any(|attempt| attempt.succeeded),
        "the successful retry is recorded: {:?}",
        health.recovery_attempts
    );
}

#[test]
fn a_real_oversized_file_recovers_from_its_own_legitimate_lkg() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    first
        .update(ConfigModule::Update, 1, |values| {
            values.insert("channel".to_owned(), json!("stable"));
        })
        .expect("seed Update revision 2");
    drop(first);

    // A genuinely oversized file: the real bounded read must refuse it. The
    // domain's own last-known-good is legitimate (it really passed validation),
    // so the existing accepted rule applies: recover from it.
    let oversized = vec![b' '; (SETTINGS_FILE_MAX_BYTES + 16) as usize];
    fs::write(paths.config_path(ConfigModule::Update), &oversized).expect("write oversized file");

    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");
    let health = facade.health(ConfigModule::Update);
    assert_eq!(health.state, DomainHealthState::Ready, "a legitimate LKG keeps the domain usable");
    assert_eq!(health.source, Some(ConfigSource::LastKnownGood));
    assert_eq!(health.revision, Some(2), "the LKG's real revision is what is served");

    // The oversized file is left untouched, never truncated or replaced.
    assert_eq!(
        fs::metadata(paths.config_path(ConfigModule::Update)).expect("stat").len(),
        SETTINGS_FILE_MAX_BYTES + 16,
        "an oversized file is not rewritten by the runtime"
    );
    // Its neighbours are unaffected.
    facade
        .update(ConfigModule::Diagnostics, 1, |values| {
            values.insert("level".to_owned(), json!("info"));
        })
        .expect("neighbouring domain still writes");
}

/// The escalation boundary: a domain with **no** legitimate LKG.
///
/// Both the live file and the last-known-good are really removed, and the live
/// path is replaced by a directory so it cannot be read. Existing accepted
/// behaviour recovers an invalid-but-present file from the LKG; with no LKG
/// there is nothing legitimate to serve, so the domain is honestly reported
/// degraded and the request fails with the real cause. Deciding what the product
/// should do here is an escalation item — this test records the *current*
/// behaviour instead of inventing a new default.
#[test]
fn a_domain_with_no_legitimate_lkg_is_reported_degraded_not_invented() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    first
        .update(ConfigModule::Skills, 1, |values| {
            values.insert("skills".to_owned(), skills_choice());
        })
        .expect("seed Skills revision 2");
    drop(first);

    // Really destroy both the live file and the legitimate LKG.
    fs::remove_file(paths.config_path(ConfigModule::Skills)).expect("remove live file");
    fs::remove_file(paths.history_path(ConfigModule::Skills).join("last-known-good.json"))
        .expect("remove LKG");
    fs::create_dir_all(paths.config_path(ConfigModule::Skills)).expect("block the live path");

    let facade = SettingsFacade::open(paths.clone()).expect("facade still opens");
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Skills]);
    let health = facade.health(ConfigModule::Skills);
    assert_eq!(health.state, DomainHealthState::Degraded);
    assert!(health.original_fault.is_some(), "the real fault is classified");
    assert!(
        health.original_message.as_deref().is_some_and(|message| !message.is_empty()),
        "the real cause is kept"
    );
    assert_eq!(health.revision, None, "no revision is claimed without a snapshot");

    // The request fails with the real cause rather than an invented empty
    // document or a fabricated revision.
    let error = facade.snapshot(ConfigModule::Skills).expect_err("no snapshot exists");
    assert!(
        matches!(
            error,
            SettingsFacadeError::DomainUnavailable { module: ConfigModule::Skills, .. }
        ),
        "{error:?}"
    );

    // Every other domain is untouched and still really usable.
    for module in ConfigModule::ALL {
        if module == ConfigModule::Skills {
            continue;
        }
        assert_eq!(facade.health(module).state, DomainHealthState::Ready, "{module:?}");
    }
    facade
        .update(ConfigModule::Commands, 1, |values| {
            values.insert("enabled".to_owned(), json!(true));
        })
        .expect("an unrelated domain still writes while Skills has no LKG");
}

#[test]
fn a_write_failure_keeps_the_domain_serving_its_previous_valid_snapshot() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    let before = facade.snapshot(ConfigModule::Memory).expect("memory snapshot");
    assert_eq!(before.document.revision, 1);

    // A rejected write: the revision in the request is stale.
    let error = facade
        .update(ConfigModule::Memory, 99, |values| {
            values.insert("enabled".to_owned(), json!(true));
        })
        .expect_err("a stale expected revision must be refused");
    assert!(matches!(error, SettingsFacadeError::RevisionConflict { .. }), "{error:?}");

    // The domain is still ready and still serving revision 1 — the rejected
    // request did not create a default, an empty document, or a rollback.
    let after = facade.snapshot(ConfigModule::Memory).expect("memory snapshot after refusal");
    assert_eq!(after.document.revision, 1);
    assert_eq!(after.content_hash, before.content_hash, "the served bytes are unchanged");
    assert_eq!(facade.health(ConfigModule::Memory).state, DomainHealthState::Ready);
    assert!(
        !facade.health(ConfigModule::Memory).recovery_attempts.is_empty(),
        "the refused write is recorded"
    );
}

#[test]
fn a_stale_external_edit_never_overwrites_a_newer_fact() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    facade
        .update(ConfigModule::Skills, 1, |values| {
            values.insert("skills".to_owned(), skills_choice());
        })
        .expect("advance to revision 2");

    // A correct, but *older*, configuration written on top: this is the
    // "always fall back to the old value" counterexample. Revision 1 is a real
    // document, but it is behind the committed revision 2. Skills is a JSON
    // domain, so the file is written as JSON; its fields sit beside the revision
    // and an empty one is valid.
    let stale = serde_json::json!({
        "schema_version": 1,
        "revision": 1
    })
    .to_string();
    fs::write(paths.config_path(ConfigModule::Skills), stale.as_bytes()).expect("write stale file");

    let error = facade.reload(ConfigModule::Skills).expect_err("a stale edit must be refused");
    assert!(matches!(error, SettingsFacadeError::NonMonotonicRevision { .. }), "{error:?}");
    let served = facade.snapshot(ConfigModule::Skills).expect("skills snapshot");
    assert_eq!(served.document.revision, 2, "the newer committed fact survives");
    assert_eq!(facade.health(ConfigModule::Skills).state, DomainHealthState::Ready);
}

#[test]
fn an_equal_revision_with_different_bytes_is_refused_rather_than_accepted() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    facade
        .update(ConfigModule::Hooks, 1, |values| {
            values.insert("hooks".to_owned(), json!([{"path": "C:/hook", "enabled": true}]));
        })
        .expect("advance to revision 2");

    // Same revision, different content: a same-revision race. Accepting it would
    // let two different facts both claim revision 2. Hooks is a TOML domain.
    let same_revision =
        "schema_version = 1\nrevision = 2\n[[hooks]]\npath = \"C:/hook\"\nenabled = false\n";
    fs::write(paths.config_path(ConfigModule::Hooks), same_revision.as_bytes()).expect("write");

    let error = facade.reload(ConfigModule::Hooks).expect_err("a same-revision race is refused");
    assert!(matches!(error, SettingsFacadeError::NonMonotonicRevision { .. }), "{error:?}");
    let served = facade.snapshot(ConfigModule::Hooks).expect("hooks snapshot");
    assert_eq!(served.document.revision, 2);
    assert_eq!(
        served.document.values.get("hooks"),
        Some(&json!([{"path": "C:/hook", "enabled": true}])),
        "the committed content is not replaced by the racing content"
    );
}

#[test]
fn a_correct_old_config_is_used_only_when_it_is_the_legitimate_lkg() {
    // A JSON domain with no schema of its own (as plugins.json once was): any well-formed
    // values are a valid revision.
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");

    // Commit a real, valid revision 2 so it becomes the legitimate LKG.
    facade
        .update(ConfigModule::Commands, 1, |values| {
            values.insert("enabled".to_owned(), json!(true));
        })
        .expect("advance to revision 2");
    drop(facade);

    // Now corrupt the live file. The domain must recover from the LKG that
    // really passed validation, not from an invented default.
    fs::write(paths.config_path(ConfigModule::Commands), b"{ this is not json").expect("corrupt");

    let facade = SettingsFacade::open(paths.clone()).expect("facade recovers");
    assert_eq!(facade.degraded_domains(), Vec::<ConfigModule>::new(), "recovery kept it usable");
    let snapshot = facade.snapshot(ConfigModule::Commands).expect("commands snapshot");
    assert_eq!(snapshot.source, ConfigSource::LastKnownGood);
    assert_eq!(snapshot.document.revision, 2);
    assert_eq!(snapshot.document.values.get("enabled"), Some(&json!(true)));

    // The invalid bytes were preserved in history, not discarded.
    let invalid_dir = paths.history_path(ConfigModule::Commands).join("invalid");
    assert!(invalid_dir.is_dir(), "the invalid file is backed up");
    assert_eq!(
        fs::read_dir(&invalid_dir).expect("read invalid dir").count(),
        1,
        "exactly the rejected input was kept"
    );
}

#[test]
fn a_secret_bearing_invalid_config_is_never_copied_into_history() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    facade
        .update(ConfigModule::Security, 1, |values| {
            values.insert("require_approval".to_owned(), json!(true));
        })
        .expect("advance to revision 2");
    drop(facade);

    // An invalid TOML document that carries a secret marker, written using the
    // real invalid-parse boundary rather than a simulated one.
    let secret_bearing = "schema_version = 1\nrevision = 9\napi_key = \"CANARY-S05-SECRET-VALUE\"\nthis line is not toml\n";
    fs::write(paths.config_path(ConfigModule::Security), secret_bearing.as_bytes())
        .expect("write secret-bearing invalid config");

    let facade = SettingsFacade::open(paths.clone()).expect("facade recovers");
    let security = paths.history_path(ConfigModule::Security);
    let invalid_dir = security.join("invalid");
    let copied = if invalid_dir.is_dir() {
        fs::read_dir(&invalid_dir).expect("read invalid dir").count()
    } else {
        0
    };
    assert_eq!(copied, 0, "a secret-bearing invalid file must not be backed up to history");

    // And the canary must not appear anywhere under the history tree at all.
    let mut haystack = Vec::new();
    collect_bytes(&security, &mut haystack);
    let text = String::from_utf8_lossy(&haystack);
    assert!(
        !text.contains("CANARY-S05-SECRET-VALUE"),
        "the secret canary leaked into the configuration history"
    );
    // The domain still recovered to a real, usable snapshot.
    facade.snapshot(ConfigModule::Security).expect("security snapshot");
}

fn collect_bytes(directory: &std::path::Path, into: &mut Vec<u8>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_bytes(&path, into);
        } else if let Ok(bytes) = fs::read(&path) {
            into.extend_from_slice(&bytes);
        }
    }
}

/// A cross-domain bundle still commits as a whole, and a bundle that fails
/// halfway is not silently reported as partial success.
#[test]
fn a_partially_published_bundle_recovers_forward_as_a_whole() {
    use aura_config::ConfigModuleBundleEntry;

    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("facade");
    let before_models = facade.snapshot(ConfigModule::Models).expect("models").document.revision;
    let before_aura = facade.snapshot(ConfigModule::Aura).expect("aura").document.revision;

    // Publish two domains but stop after the first real replace. This is an
    // interrupted cross-domain publication with a real journal on disk.
    let error = facade
        .commit_module_bundle_with_fault(
            vec![
                ConfigModuleBundleEntry {
                    module: ConfigModule::Aura,
                    expected_revision: before_aura,
                    replacement_values: Some(BTreeMap::from([(
                        "default_model_profile_id".to_owned(),
                        json!("00000000-0000-0000-0000-0000000000aa"),
                    )])),
                },
                ConfigModuleBundleEntry {
                    module: ConfigModule::Models,
                    expected_revision: before_models,
                    replacement_values: Some(BTreeMap::from([(
                        "models".to_owned(),
                        json!({ "opaque": true }),
                    )])),
                },
            ],
            Some(aura_config::ConfigBundleFaultPoint::AfterFirstReplace),
        )
        .expect_err("the injected fault must surface");
    assert!(
        matches!(error, SettingsFacadeError::BundleFaultInjected(_)),
        "the real injected fault is reported, not a partial success: {error:?}"
    );
    drop(facade);

    // On the next open the journal is converged forward: **both** domains reach
    // the bundle's after image, so the bundle is whole rather than half applied.
    let reopened = SettingsFacade::open(paths.clone()).expect("facade after interrupted bundle");
    assert_eq!(
        reopened.snapshot(ConfigModule::Aura).expect("aura").document.revision,
        before_aura + 1,
        "the journal's first domain was converged forward"
    );
    assert_eq!(
        reopened.snapshot(ConfigModule::Models).expect("models").document.revision,
        before_models + 1,
        "the journal's second domain was converged forward too"
    );
    // The journal is finished, so the work is not left pending.
    let journal_dir = paths.config_transaction_dir.join("module-bundles");
    let remaining = if journal_dir.is_dir() {
        fs::read_dir(&journal_dir).expect("read journal dir").count()
    } else {
        0
    };
    assert_eq!(remaining, 0, "the converged journal is removed");
}

/// A journal that cannot be **decoded** establishes no affected set.
///
/// # Why this test changed
///
/// The first version of this test asserted that every domain stays *available*
/// when a journal is undecodable, on the reasoning that "an unreadable journal
/// names no domain, so it cannot degrade any of them". That reasoning is the
/// exact defect R2 names: it **equates unknown with the empty set**. An
/// undecodable journal is not evidence that nothing was half-published; it is
/// evidence that the runtime cannot tell. A bundle that stopped after its first
/// real replace leaves one domain on the after image and the rest on the before
/// image, and refusing to look is how that half-published state gets reported as
/// Ready.
///
/// The user's confirmed rule for this condition (no readable cross-domain save
/// manifest, "only half updated" cannot be ruled out) is:
///
/// * keep view, stop and configuration repair;
/// * suspend new calls that depend on the suspect configuration;
/// * resume once consistent;
/// * do **not** mark the uncertain configuration Ready;
/// * do not auto-kill execution that already started.
///
/// That is what this test now asserts. The genuinely independent judgement —
/// "a journal that *does* decode and names one domain must not degrade a domain
/// it does not name" — is covered separately by
/// `a_named_bundle_failure_holds_only_the_domains_it_names`.
#[test]
fn an_undecodable_bundle_journal_is_an_unknown_scope_not_an_empty_one() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    drop(first);

    // A real journal file that cannot be decoded: the record is genuinely
    // malformed, so convergence really fails at the decode boundary.
    let journal_dir = paths.config_transaction_dir.join("module-bundles");
    fs::create_dir_all(&journal_dir).expect("journal dir");
    fs::write(journal_dir.join("broken.json"), b"{ \"transaction_id\": ").expect("write journal");

    // The facade still opens. An unreadable manifest must not stop the process
    // from starting, because "cannot read the manifest" is not "the product is
    // broken"; it is "the configuration's consistency is unknown".
    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");
    assert!(
        !facade.recovery_scope_known(),
        "an undecodable journal must be reported as an unknown scope, not as an empty one"
    );
    assert_eq!(
        facade.recovery_held_domains(),
        ConfigModule::ALL.to_vec(),
        "with the scope unknown, every domain is under protection"
    );
    for module in ConfigModule::ALL {
        assert_eq!(
            facade.health(module).state,
            DomainHealthState::Degraded,
            "{module:?} must not be marked Ready while its consistency is unknown"
        );
        assert_eq!(
            facade.health(module).original_fault,
            Some(aura_config::DomainFaultKind::BundleRecovery),
            "{module:?} records the real reason"
        );
        // The user's rule keeps view, stop and repair reachable: the domain's
        // *status* is readable through `health`, and the fault is named.
        assert!(facade.health(module).original_message.is_some());
    }

    // New calls that depend on the suspect configuration are suspended, and they
    // fail honestly rather than succeeding against unverified bytes.
    let error = facade
        .update(ConfigModule::Aura, 1, |values| {
            values.insert("theme".to_owned(), json!("system"));
        })
        .expect_err("a new write that depends on the suspect configuration is suspended");
    assert!(matches!(error, SettingsFacadeError::DomainUnavailable { .. }), "{error:?}");
    // Reading the suspect configuration is suspended for the same reason.
    assert!(facade.snapshot(ConfigModule::Aura).is_err());

    // A single-domain reload cannot bypass the unresolved manifest.
    let error = facade
        .reload(ConfigModule::Aura)
        .expect_err("a reload must not silently clear an unknown recovery scope");
    assert!(matches!(error, SettingsFacadeError::DomainUnavailable { .. }), "{error:?}");

    // The manifest is deliberately left on disk: deleting it would destroy the
    // only record of the interrupted publication, and its unresolved state is
    // exactly what the protection is about.
    assert!(
        journal_dir.join("broken.json").is_file(),
        "an unconvergeable journal is preserved, not deleted"
    );
}

/// A bundle failure that **does** name its domains holds exactly those domains.
///
/// This is the other half of R2's "unknown is not the empty set": a known scope
/// is a consistent component under control, so the domains the journal names are
/// held and every domain it does not name keeps working. The two cases must not
/// be collapsed into one another in either direction.
#[test]
fn a_named_bundle_failure_holds_only_the_domains_it_names() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let first = SettingsFacade::open(paths.clone()).expect("initial facade");
    drop(first);

    // A real, decodable journal whose entries name Aura and Models, and whose
    // frozen before-image does not match what is on disk: convergence really
    // fails at the conflict boundary with a *known* scope.
    let journal_dir = paths.config_transaction_dir.join("module-bundles");
    fs::create_dir_all(&journal_dir).expect("journal dir");
    let aura_before =
        ContentHash::blake3(&fs::read(paths.config_path(ConfigModule::Aura)).expect("aura bytes"))
            .to_string();
    let models_before = ContentHash::blake3(
        &fs::read(paths.config_path(ConfigModule::Models)).expect("models bytes"),
    )
    .to_string();
    let journal = serde_json::json!({
        "transaction_id": "ctx_s05_r2_named",
        "created_at": "2026-10-08T00:00:00Z",
        "entries": [
            {
                "module": "aura",
                "before_revision": 1,
                "before_content_hash": aura_before,
                "candidate": {
                    "schema_version": 1,
                    "revision": 2,
                    "values": { "theme": "dark" }
                }
            },
            {
                "module": "models",
                "before_revision": 1,
                "before_content_hash": models_before,
                "candidate": {
                    "schema_version": 1,
                    "revision": 2,
                    "values": { "models": [] }
                }
            }
        ]
    })
    .to_string();
    fs::write(journal_dir.join("ctx_s05_r2_named.json"), journal.as_bytes()).expect("write");

    // Move Aura's real file so it is neither the frozen before image nor the
    // candidate: a genuine recovery conflict with a known domain set.
    fs::write(
        paths.config_path(ConfigModule::Aura),
        b"schema_version = 1\nrevision = 7\n[values]\n",
    )
    .expect("conflict");

    let facade = SettingsFacade::open(paths.clone()).expect("facade opens");
    assert!(facade.recovery_scope_known(), "a decodable journal names a known scope");
    assert_eq!(
        facade.recovery_held_domains(),
        vec![ConfigModule::Aura, ConfigModule::Models],
        "exactly the named domains are held"
    );

    // A domain the bundle does not name keeps working for real.
    let updated = facade
        .update(ConfigModule::Ui, 1, |values| {
            values.insert("theme".to_owned(), json!("dark"));
        })
        .expect("an unrelated domain still writes");
    assert_eq!(updated.document.revision, 2);
    assert!(facade.is_available(ConfigModule::Ui));
    assert!(facade.snapshot(ConfigModule::Diagnostics).is_ok());

    // A named domain is honestly unavailable rather than Ready.
    assert_eq!(
        facade.health(ConfigModule::Models).state,
        DomainHealthState::Degraded,
        "a held domain is not Ready"
    );
    let error = facade.snapshot(ConfigModule::Models).expect_err("held domain");
    assert!(matches!(error, SettingsFacadeError::DomainUnavailable { .. }), "{error:?}");
}

/// Startup and retry apply the same validator. A rejected live document and
/// rejected LKG cannot be replaced by an unvalidated empty document.
#[test]
fn a_reopened_domain_uses_the_same_validator_as_startup() {
    use aura_config::{ConfigValidationError, ModuleConfigDocument, ModuleConfigValidator};

    struct RejectingValidator;
    impl ModuleConfigValidator for RejectingValidator {
        fn validate(
            &self,
            _module: ConfigModule,
            _document: &ModuleConfigDocument,
        ) -> Result<(), ConfigValidationError> {
            Err(ConfigValidationError::Semantic("S05 validator refusal".to_owned()))
        }
    }

    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    drop(SettingsFacade::open(paths.clone()).expect("seed valid files and LKG"));
    let original =
        fs::read(paths.config_path(ConfigModule::Skills)).expect("isolated validation fixture");
    let mut validators: BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>> = BTreeMap::new();
    validators.insert(ConfigModule::Skills, Arc::new(RejectingValidator));

    let facade = SettingsFacade::open_with_validators(paths.clone(), validators)
        .expect("isolated validation fixture");
    assert_eq!(facade.degraded_domains(), vec![ConfigModule::Skills]);
    assert_eq!(facade.health(ConfigModule::Skills).source, None);
    assert_eq!(
        facade.health(ConfigModule::Skills).original_fault,
        Some(aura_config::DomainFaultKind::Validation)
    );
    assert!(facade.snapshot(ConfigModule::Skills).is_err());
    assert_eq!(
        fs::read(paths.config_path(ConfigModule::Skills)).expect("isolated validation fixture"),
        original
    );

    let rejected_body = serde_json::json!({
        "schema_version": 1,
        "revision": 3,
        "values": { "S05-CANARY-DOCUMENT": true }
    })
    .to_string();
    fs::write(paths.config_path(ConfigModule::Skills), &rejected_body)
        .expect("isolated validation fixture");
    assert!(facade.reload(ConfigModule::Skills).is_err());
    assert!(facade.snapshot(ConfigModule::Skills).is_err());
    assert_eq!(
        fs::read(paths.config_path(ConfigModule::Skills)).expect("isolated validation fixture"),
        rejected_body.as_bytes()
    );
    assert!(facade.snapshot(ConfigModule::Memory).is_ok());
}

#[test]
fn corrupt_existing_config_without_lkg_keeps_its_bytes_and_is_unavailable() {
    let directory = tempfile::tempdir().expect("isolated validation fixture");
    let paths = paths(&directory);
    drop(SettingsFacade::open(paths.clone()).expect("isolated validation fixture"));
    fs::remove_file(paths.history_path(ConfigModule::Browser).join("last-known-good.toml"))
        .expect("isolated validation fixture");
    let invalid = b"invalid = [\n";
    fs::write(paths.config_path(ConfigModule::Browser), invalid)
        .expect("isolated validation fixture");
    let facade = SettingsFacade::open(paths.clone()).expect("isolated validation fixture");
    assert_eq!(
        fs::read(paths.config_path(ConfigModule::Browser)).expect("isolated validation fixture"),
        invalid
    );
    assert!(!facade.is_available(ConfigModule::Browser));
    assert_eq!(
        facade.health(ConfigModule::Browser).original_fault,
        Some(aura_config::DomainFaultKind::Codec)
    );
    assert!(!paths.history_path(ConfigModule::Browser).join("last-known-good.toml").exists());
    assert!(facade.is_available(ConfigModule::Memory));
}

#[test]
fn fresh_defaults_require_validation_before_becoming_durable_or_available() {
    use aura_config::{ConfigValidationError, ModuleConfigDocument, ModuleConfigValidator};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Validator(Arc<AtomicBool>);
    impl ModuleConfigValidator for Validator {
        fn validate(
            &self,
            _module: ConfigModule,
            _document: &ModuleConfigDocument,
        ) -> Result<(), ConfigValidationError> {
            if self.0.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(ConfigValidationError::Semantic("default rejected".to_owned()))
            }
        }
    }

    let directory = tempfile::tempdir().expect("isolated validation fixture");
    let paths = paths(&directory);
    let accept = Arc::new(AtomicBool::new(false));
    let mut validators: BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>> = BTreeMap::new();
    validators.insert(ConfigModule::Browser, Arc::new(Validator(Arc::clone(&accept))));
    let facade = SettingsFacade::open_with_validators(paths.clone(), validators)
        .expect("isolated validation fixture");
    assert!(!facade.is_available(ConfigModule::Browser));
    assert!(!paths.config_path(ConfigModule::Browser).exists());
    assert!(!paths.history_path(ConfigModule::Browser).join("last-known-good.toml").exists());
    assert!(facade.is_available(ConfigModule::Memory));

    accept.store(true, Ordering::SeqCst);
    assert!(facade.reload(ConfigModule::Browser).expect("isolated validation fixture"));
    assert!(facade.is_available(ConfigModule::Browser));
    assert_eq!(
        facade.snapshot(ConfigModule::Browser).expect("isolated validation fixture").source,
        ConfigSource::Default
    );
    assert!(paths.config_path(ConfigModule::Browser).is_file());
    assert!(paths.history_path(ConfigModule::Browser).join("last-known-good.toml").is_file());
}
