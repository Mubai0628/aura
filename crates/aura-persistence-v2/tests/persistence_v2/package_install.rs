//! Persistence authorization/CAS tests. These synthesize records, never claim to
//! be runtime Conformance evidence; the actual backend has separate real tests.
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{Revision, ToolConformanceRunId, ToolPackageId, UtcTimestamp};
use aura_persistence_v2::{
    V2Store,
    packages::{self, conformance::*},
};
use aura_tool_domain::*;

fn imported() -> (tempfile::TempDir, V2Store, PackageVersionBindingV1) {
    imported_with(|_| {})
}

/// Persistence trusts Core's strict stager for the fingerprint, so a test may shape the
/// manifest it stores; only Core-produced descriptors reach this API in production.
fn imported_with(
    edit: impl FnOnce(&mut CanonicalToolPackageManifestV1),
) -> (tempfile::TempDir, V2Store, PackageVersionBindingV1) {
    let root = tempfile::tempdir().expect("temporary store");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let fixture = aura_contract_schema::canonical_fixture().expect("staged fixture").tools;
    let mut descriptor = PackageContentDescriptorV1 {
        manifest: fixture.canonical_package_manifest.expect("manifest"),
        package_fingerprint: fixture.package_fingerprint.expect("fingerprint"),
        security_fingerprint: fixture.package_security_fingerprint.expect("security"),
        runtime_closure_hash: fixture.package_runtime_closure_hash.expect("closure"),
    };
    edit(&mut descriptor.manifest);
    let binding = store
        .database
        .unit_of_work(|tx| {
            packages::import_content(
                tx,
                ToolPackageId::new(),
                &descriptor,
                &[packages::PackageFileBytes {
                    path: &descriptor.manifest.files[0].path,
                    bytes: b"stdio fixture runtime",
                }],
                UtcTimestamp::now(),
            )
        })
        .expect("import");
    (root, store, binding)
}
fn report(store: &mut V2Store, binding: &PackageVersionBindingV1) -> PackageConformanceReportV1 {
    report_with(store, binding, &[])
}
fn report_with(
    store: &mut V2Store,
    binding: &PackageVersionBindingV1,
    settings: &[PackageToolSettingsV1],
) -> PackageConformanceReportV1 {
    let at = UtcTimestamp::now();
    let run_id = ToolConformanceRunId::new();
    let version = store
        .database
        .unit_of_work(|tx| begin_conformance(tx, binding, run_id, settings, at))
        .expect("start");
    let content = packages::load_content(store.database.connection(), &version).expect("content");
    let cases = content
        .descriptor
        .manifest
        .tools
        .iter()
        .flat_map(|tool| {
            tool.conformance_cases.iter().map(|case| PackageConformanceCaseResultV1 {
                tool_key: tool.tool_key.clone(),
                case_name: case.name.clone(),
                execution_location: PackageExecutionLocationV1::UserJob,
                reason: PackageConformanceReasonV1::Completed,
                runtime_evidence_hash: CanonicalSha256HashV1::sha256(
                    b"synthetic metadata unit evidence",
                ),
            })
        })
        .collect();
    PackageConformanceReportV1 {
        run_id,
        version,
        started_at: at,
        completed_at: UtcTimestamp::now(),
        cases,
        cleanup_verified: true,
        settings_hash: package_settings_hash_v1(settings).expect("settings hash"),
    }
}
fn trust_count(store: &V2Store) -> i64 {
    store
        .database
        .connection()
        .query_row("SELECT count(*) FROM tool_package_trust", [], |row| row.get(0))
        .expect("count")
}

#[test]
fn unconfirmed_cleanup_keeps_a_strong_execution_reference_until_startup_recovery() {
    let (_root, mut store, binding) = imported();
    let mut run = report(&mut store, &binding);
    run.cleanup_verified = false;
    run.cases[0].reason = PackageConformanceReasonV1::CleanupUnconfirmed;
    store
        .database
        .unit_of_work(|tx| finish_conformance(tx, &run))
        .expect("truthful unknown cleanup");
    let count = |store: &V2Store| {
        store
            .database
            .connection()
            .query_row(
                "SELECT count(*) FROM tool_package_references WHERE owner_kind='execution'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("strong reference")
    };
    assert_eq!(count(&store), 1, "GC must not collect a possibly still used payload");
    assert!(
        store
            .database
            .unit_of_work(|tx| begin_conformance(
                tx,
                &run.version,
                ToolConformanceRunId::new(),
                &[],
                UtcTimestamp::now(),
            ))
            .is_err(),
        "an unsettled owner cannot be replaced inside the live Core"
    );
    store
        .database
        .unit_of_work(|tx| interrupt_conformance(tx, UtcTimestamp::now()))
        .expect("startup after the previous Core and Jobs have gone away");
    assert_eq!(count(&store), 0);
    assert_eq!(trust_count(&store), 0);
    assert!(
        store
            .database
            .unit_of_work(|tx| begin_conformance(
                tx,
                &run.version,
                ToolConformanceRunId::new(),
                &[],
                UtcTimestamp::now(),
            ))
            .is_ok(),
        "a fresh explicit verification can run after recovery"
    );
}

#[test]
fn import_and_successful_conformance_do_not_install_without_an_exact_explicit_approval() {
    let (_root, mut store, binding) = imported();
    let wrong = CanonicalSha256HashV1::sha256(b"not a report");
    assert!(
        store
            .database
            .unit_of_work(|tx| packages::install(
                tx,
                &binding,
                Revision::INITIAL,
                &wrong,
                UtcTimestamp::now()
            ))
            .is_err()
    );
    let report = report(&mut store, &binding);
    assert_eq!(trust_count(&store), 0);
    store.database.unit_of_work(|tx| finish_conformance(tx, &report)).expect("finish");
    let state = packages::state(store.database.connection(), binding.package_id)
        .expect("state")
        .expect("package");
    assert!(!state.installed && !state.trusted && !state.enabled);
    assert!(
        store
            .database
            .unit_of_work(|tx| packages::install(
                tx,
                &report.version,
                state.revision,
                &wrong,
                UtcTimestamp::now()
            ))
            .is_err()
    );
    let next = store
        .database
        .unit_of_work(|tx| {
            packages::install(
                tx,
                &report.version,
                state.revision,
                &report_hash(&report)?,
                UtcTimestamp::now(),
            )
        })
        .expect("explicit exact approval");
    let installed = packages::state(store.database.connection(), binding.package_id)
        .expect("state")
        .expect("package");
    assert!(installed.installed && installed.trusted && installed.enabled);
    assert_eq!(installed.revision, next);
    assert!(next > state.revision);
    assert!(
        store
            .database
            .unit_of_work(|tx| packages::install(
                tx,
                &report.version,
                state.revision,
                &report_hash(&report)?,
                UtcTimestamp::now()
            ))
            .is_err(),
        "old approval is not reusable"
    );
}

#[test]
fn wrong_cases_backend_identity_or_run_cannot_be_accepted_and_failure_cannot_install() {
    let (_root, mut store, binding) = imported();
    let good = report(&mut store, &binding);
    for mutation in 0..4 {
        let mut bad = good.clone();
        match mutation {
            0 => bad.run_id = ToolConformanceRunId::new(),
            1 => bad.cases.clear(),
            2 => bad.cases.push(bad.cases[0].clone()),
            _ => bad.cases[0].execution_location = PackageExecutionLocationV1::ControlledHttp,
        }
        assert!(store.database.unit_of_work(|tx| finish_conformance(tx, &bad)).is_err());
    }
    let mut failed = good;
    failed.cases[0].reason = PackageConformanceReasonV1::InvalidResult;
    store.database.unit_of_work(|tx| finish_conformance(tx, &failed)).expect("a truthful failure");
    assert!(
        store
            .database
            .unit_of_work(|tx| packages::install(
                tx,
                &failed.version,
                Revision::INITIAL,
                &report_hash(&failed)?,
                UtcTimestamp::now()
            ))
            .is_err()
    );
    assert_eq!(trust_count(&store), 0);
}

#[test]
fn cancelled_and_interrupted_runs_release_leases_without_trust_or_automatic_replay() {
    for interruption in [false, true] {
        let (root, mut store, binding) = imported();
        let mut run = report(&mut store, &binding);
        let count = |store: &V2Store| {
            store
                .database
                .connection()
                .query_row(
                    "SELECT count(*) FROM tool_package_references WHERE owner_kind='execution'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("lease count")
        };
        assert_eq!(count(&store), 1);
        if interruption {
            drop(store);
            store = V2Store::open(&root.path().join("v2")).expect("restart");
            assert_eq!(
                store
                    .database
                    .unit_of_work(|tx| interrupt_conformance(tx, UtcTimestamp::now()))
                    .expect("recovery"),
                1
            );
            assert!(
                store.database.unit_of_work(|tx| finish_conformance(tx, &run)).is_err(),
                "a late pre-crash success has lost authority"
            );
            assert_eq!(
                store
                    .database
                    .unit_of_work(|tx| interrupt_conformance(tx, UtcTimestamp::now()))
                    .expect("idempotent recovery"),
                0
            );
        } else {
            run.cases[0].reason = PackageConformanceReasonV1::Cancelled;
            store
                .database
                .unit_of_work(|tx| finish_conformance(tx, &run))
                .expect("cancel terminal");
            assert_eq!(run.state(), PackageConformanceStateV1::Cancelled);
        }
        assert_eq!(count(&store), 0);
        assert_eq!(trust_count(&store), 0);
    }
}

#[test]
fn failed_install_audit_rolls_back_current_version_enable_and_trust_together() {
    let (_root, mut store, binding) = imported();
    let report = report(&mut store, &binding);
    store.database.unit_of_work(|tx| finish_conformance(tx, &report)).expect("finish");
    store.database.connection().execute_batch("CREATE TEMP TRIGGER fail_package_install BEFORE INSERT ON events WHEN NEW.kind='package.installed' BEGIN SELECT RAISE(ABORT,'synthetic install audit failure'); END;").expect("temporary fault injection");
    let result = store.database.unit_of_work(|tx| {
        packages::install(
            tx,
            &report.version,
            Revision::INITIAL,
            &report_hash(&report)?,
            UtcTimestamp::now(),
        )
    });
    assert!(result.is_err());
    assert_eq!(trust_count(&store), 0);
    let after = packages::state(store.database.connection(), binding.package_id)
        .expect("state")
        .expect("package");
    assert!(!after.installed && !after.enabled && !after.trusted);
    assert_eq!(after.revision, Revision::INITIAL);
}

#[test]
fn contradictory_run_json_and_sql_state_are_rejected_before_installation() {
    let (_root, mut store, binding) = imported();
    let report = report(&mut store, &binding);
    store.database.unit_of_work(|tx| finish_conformance(tx, &report)).expect("finish");
    store
        .database
        .connection()
        .execute("UPDATE tool_package_versions SET conformance_state='interrupted'", [])
        .expect("temporary corruption");
    assert!(conformance(store.database.connection(), &report.version).is_err());
    assert!(
        store
            .database
            .unit_of_work(|tx| packages::install(
                tx,
                &report.version,
                Revision::INITIAL,
                &report_hash(&report)?,
                UtcTimestamp::now()
            ))
            .is_err()
    );
    assert_eq!(trust_count(&store), 0);
}

#[test]
fn an_unchecked_result_shape_still_passes_but_an_unsupported_program_credential_fails() {
    let (_root, mut store, binding) = imported();
    let mut report = report(&mut store, &binding);
    report.cases[0].reason = PackageConformanceReasonV1::CompletedResultUnchecked;
    assert_eq!(report.state(), PackageConformanceStateV1::Succeeded);
    report.cases[0].reason = PackageConformanceReasonV1::ProgramCredentialUnsupported;
    assert_eq!(report.state(), PackageConformanceStateV1::Failed);
}

fn configured(manifest: &mut CanonicalToolPackageManifestV1) {
    manifest.tools[0].instance_configuration =
        CanonicalToolPackageInstanceConfigurationSchemaV1(Some(
            aura_tool_contract::ToolInputSchemaV1::new(serde_json::json!({
                "type":"object","properties":{"region":{"type":"string"}},
                "required":["region"],"additionalProperties":false
            }))
            .expect("schema"),
        ));
}

#[test]
fn settings_are_checked_at_start_bound_by_the_report_and_copied_only_by_install() {
    let (_root, mut store, binding) = imported_with(configured);
    let tool = {
        let content =
            packages::load_content(store.database.connection(), &binding).expect("content");
        content.descriptor.manifest.tools[0].tool_key.clone()
    };
    let settings = vec![PackageToolSettingsV1 {
        tool_key: tool.clone(),
        configuration: Some(serde_json::json!({"region":"eu"})),
        endpoints: vec![],
    }];
    for invalid in [
        vec![],
        vec![PackageToolSettingsV1 {
            configuration: Some(serde_json::json!({"region":1})),
            ..settings[0].clone()
        }],
    ] {
        assert!(
            store
                .database
                .unit_of_work(|tx| begin_conformance(
                    tx,
                    &binding,
                    ToolConformanceRunId::new(),
                    &invalid,
                    UtcTimestamp::now()
                ))
                .is_err(),
            "missing or invalid configuration cannot start a run"
        );
    }
    let mut report = report_with(&mut store, &binding, &settings);
    let mut forged = report.clone();
    forged.settings_hash = package_settings_hash_v1(&[]).expect("hash");
    assert!(store.database.unit_of_work(|tx| finish_conformance(tx, &forged)).is_err());
    report.completed_at = UtcTimestamp::now();
    store.database.unit_of_work(|tx| finish_conformance(tx, &report)).expect("finish");
    assert!(
        packages::installed_settings(store.database.connection(), &report.version)
            .expect("read")
            .is_empty(),
        "a successful run is not installation"
    );
    store
        .database
        .unit_of_work(|tx| {
            packages::install(
                tx,
                &report.version,
                Revision::INITIAL,
                &report_hash(&report)?,
                UtcTimestamp::now(),
            )
        })
        .expect("install");
    assert_eq!(
        packages::installed_settings(store.database.connection(), &report.version).expect("read"),
        settings
    );
    let entry = packages::catalog::package(store.database.connection(), binding.package_id)
        .expect("catalog")
        .expect("package");
    assert!(entry.eligible() && entry.installed.as_ref() == Some(&report.version));
}
