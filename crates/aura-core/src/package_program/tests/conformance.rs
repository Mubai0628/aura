use super::support::*;
use crate::package_program::*;
use aura_domain_common::{Revision, ToolConformanceRunId, ToolPackageId, UtcTimestamp};
use aura_persistence_v2::{
    V2Store,
    packages::{self, conformance::*},
};
use aura_tool_domain::{
    PackageConformanceReasonV1 as Reason, PackageConformanceStateV1, PackageVersionBindingV1,
};
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[test]
fn a_real_native_package_runs_conformance_before_separate_v2_install_approval() {
    let fixture = Fixture::new(Kind::Command, |_| {});
    let mut store = V2Store::create(&fixture.root.path().join("v2")).expect("V2 only");
    let content =
        aura_tool_package::package_content_descriptor_v1(&fixture.staged).expect("fingerprint");
    let bytes = fs::read(fixture.staged.root().join(content.manifest.files[0].path.as_str()))
        .expect("payload");
    let version = store
        .database
        .unit_of_work(|tx| {
            packages::import_content(
                tx,
                ToolPackageId::new(),
                &content,
                &[packages::PackageFileBytes {
                    path: &content.manifest.files[0].path,
                    bytes: &bytes,
                }],
                UtcTimestamp::now(),
            )
        })
        .expect("import");
    let started = UtcTimestamp::now();
    let run = ToolConformanceRunId::new();
    let version = store
        .database
        .unit_of_work(|tx| begin_conformance(tx, &version, run, &[], started))
        .expect("record before execution");
    let (report, observations) = run_conformance(
        &fixture.staged,
        PackageConformanceRequest {
            version: version.clone(),
            run_id: run,
            started_at: started,
            settings: Vec::new(),
        },
        &fixture.context(),
        &AtomicBool::new(false),
        &mut |_, _, _| {},
    )
    .expect("actual Native adapter");
    assert_eq!(report.state(), PackageConformanceStateV1::Succeeded);
    assert!(observations[0].started && observations[0].stop_confirmed);
    fixture.assert_scratch_clean();
    store
        .database
        .unit_of_work(|tx| finish_conformance(tx, &report))
        .expect("durable exact report");
    let before = packages::state(store.database.connection(), version.package_id)
        .expect("state")
        .expect("package");
    assert!(!before.installed && !before.enabled && !before.trusted);
    store
        .database
        .unit_of_work(|tx| {
            packages::install(
                tx,
                &version,
                before.revision,
                &report_hash(&report)?,
                UtcTimestamp::now(),
            )
        })
        .expect("separate explicit approval");
    let after = packages::state(store.database.connection(), version.package_id)
        .expect("state")
        .expect("package");
    assert!(after.installed && after.enabled && after.trusted);
}

#[test]
fn powershell_preserves_the_manifest_directory_without_profiles_or_owner_environment() {
    for directory in [
        serde_json::json!({"type":"package_root"}),
        serde_json::json!({"type":"workspace_root"}),
        serde_json::json!({"type":"resolved_directory_claim","claim_key":"selected"}),
    ] {
        let fixture = Fixture::new(Kind::Powershell, |raw| {
            raw["tools"][0]["runtime"]["command"]["working_directory"] = directory.clone()
        });
        let key = &fixture.staged.canonical_manifest().tools[0].tool_key;
        let selected = fixture.root.path().join("selected");
        fs::create_dir(&selected).expect("temporary selected directory");
        let mut context = fixture.context();
        context.directory_claims.insert(
            aura_tool_contract::PackageResourceClaimKeyV1::new("selected").expect("claim"),
            selected.clone(),
        );
        let expected = match directory["type"].as_str().expect("declaration") {
            "package_root" => fixture.staged.root(),
            "workspace_root" => fixture.root.path(),
            _ => selected.as_path(),
        };
        let result = run_case(
            &fixture.staged,
            key,
            &serde_json::json!({}),
            &context,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("PowerShell Native");
        assert_eq!(result.reason, Reason::Completed);
        let actual = String::from_utf8(result.body).expect("UTF8 path");
        assert_eq!(
            fs::canonicalize(actual).expect("actual directory"),
            fs::canonicalize(expected).expect("declared directory")
        );
        fixture.assert_scratch_clean();
    }
}

#[test]
fn stdio_uses_the_same_job_with_identity_bound_frames_and_verified_artifacts() {
    let fixture = Fixture::new(Kind::Stdio, |_| {});
    let key = &fixture.staged.canonical_manifest().tools[0].tool_key;
    let result = run_case(
        &fixture.staged,
        key,
        &serde_json::json!({}),
        &fixture.context(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("stdio Native");
    assert_eq!(result.reason, Reason::Completed);
    assert!(result.started && result.stop_confirmed);
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].bytes, b"authorized Aura Stdio fixture output\n");
    let body: serde_json::Value = serde_json::from_slice(&result.body).expect("bound final result");
    assert_eq!(body["arguments"], serde_json::json!({}));
    fixture.assert_scratch_clean();
    let missing = Fixture::new(Kind::Stdio, |raw| {
        raw["tools"][0]["runtime"]["arguments"] =
            serde_json::json!([{"type":"literal","value":"--missing-terminal"}])
    });
    let key = &missing.staged.canonical_manifest().tools[0].tool_key;
    let result = run_case(
        &missing.staged,
        key,
        &serde_json::json!({}),
        &missing.context(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("closed protocol failure");
    assert_eq!(result.reason, Reason::InvalidProtocol);
    assert!(result.stop_confirmed && result.artifacts.is_empty());
    missing.assert_scratch_clean();
}

#[test]
fn timeouts_cancellation_exit_failure_and_missing_workspace_never_become_passes() {
    let fixture = Fixture::new(Kind::Command, |raw| {
        raw["tools"][0]["runtime"]["command"]["arguments"] =
            serde_json::json!([{"type":"literal","value":"--wait-for-cancel"}])
    });
    let key = &fixture.staged.canonical_manifest().tools[0].tool_key;
    for cancel_on_start in [false, true] {
        let mut context = fixture.context();
        context.timeout = Duration::from_millis(300);
        let cancel = AtomicBool::new(false);
        let result =
            run_case(&fixture.staged, key, &serde_json::json!({}), &context, &cancel, &mut |_| {
                if cancel_on_start {
                    cancel.store(true, Ordering::Release);
                }
            })
            .expect("bounded run");
        assert_eq!(
            result.reason,
            if cancel_on_start { Reason::Cancelled } else { Reason::TimedOut }
        );
        assert!(result.stop_confirmed);
        fixture.assert_scratch_clean();
    }
    let failed = Fixture::new(Kind::Command, |raw| {
        raw["tools"][0]["runtime"]["command"]["arguments"] =
            serde_json::json!([{"type":"literal","value":"--panic"}])
    });
    let result = run_case(
        &failed.staged,
        &failed.staged.canonical_manifest().tools[0].tool_key,
        &serde_json::json!({}),
        &failed.context(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("observed exit failure");
    assert_eq!(result.reason, Reason::ProcessFailed);
    let missing = Fixture::new(Kind::Command, |raw| {
        raw["tools"][0]["runtime"]["command"]["working_directory"] =
            serde_json::json!({"type":"workspace_root"})
    });
    let mut context = missing.context();
    context.workspace_root = None;
    let mut started = false;
    assert!(
        run_case(
            &missing.staged,
            &missing.staged.canonical_manifest().tools[0].tool_key,
            &serde_json::json!({}),
            &context,
            &AtomicBool::new(false),
            &mut |_| started = true
        )
        .is_err()
    );
    assert!(!started);
}

#[test]
fn http_cases_run_on_the_fixture_and_program_credentials_are_refused_without_launch() {
    for (kind, credential, expected, state) in [
        (Kind::Http, false, Reason::Completed, PackageConformanceStateV1::Succeeded),
        (
            Kind::Command,
            true,
            Reason::ProgramCredentialUnsupported,
            PackageConformanceStateV1::Failed,
        ),
    ] {
        let fixture = Fixture::new(kind, |raw| {
            if credential {
                raw["tools"][0]["runtime"]["command"]["credential_slot"] = serde_json::json!({"slot_key":"synthetic","environment_variable":"AURA_SYNTHETIC_INPUT"});
            }
        });
        let content =
            aura_tool_package::package_content_descriptor_v1(&fixture.staged).expect("descriptor");
        let binding = PackageVersionBindingV1 {
            package_id: ToolPackageId::new(),
            package_version: content.manifest.package_version,
            package_fingerprint: content.package_fingerprint,
            revision: Revision::INITIAL,
        };
        let (report, observed) = run_conformance(
            &fixture.staged,
            PackageConformanceRequest {
                version: binding,
                run_id: ToolConformanceRunId::new(),
                started_at: UtcTimestamp::now(),
                settings: Vec::new(),
            },
            &fixture.context(),
            &AtomicBool::new(false),
            &mut |_, _, _| {},
        )
        .expect("truthful result");
        assert!(observed.is_empty(), "neither case launches a program");
        assert_eq!(report.cases[0].reason, expected);
        assert_eq!(report.state(), state);
        fixture.assert_scratch_clean();
    }
}

#[test]
fn invalid_results_and_output_limit_cannot_be_reported_as_completed() {
    let fixture = Fixture::new(Kind::Command, |raw| {
        raw["tools"][0]["model"]["result_contract"] =
            serde_json::json!({"type":"structured_json","result_schema":{"type":"object"}})
    });
    let key = &fixture.staged.canonical_manifest().tools[0].tool_key;
    let result = run_case(
        &fixture.staged,
        key,
        &serde_json::json!({}),
        &fixture.context(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("closed invalid result");
    assert_eq!(result.reason, Reason::InvalidResult);
    assert!(result.stop_confirmed);
    let mut context = fixture.context();
    context.max_output_bytes = 8;
    let result = run_case(
        &fixture.staged,
        key,
        &serde_json::json!({}),
        &context,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("bounded result");
    assert_eq!(result.reason, Reason::OutputLimit);
    assert!(result.stop_confirmed);
    fixture.assert_scratch_clean();
}

#[test]
fn cleanup_failure_is_reported_separately_after_confirmed_process_stop() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new(Kind::Command, |_| {});
    let key = &fixture.staged.canonical_manifest().tools[0].tool_key;
    let mut held = None;
    let mut owned_root = None;
    let result = run_case(
        &fixture.staged,
        key,
        &serde_json::json!({}),
        &fixture.context(),
        &AtomicBool::new(false),
        &mut |_| {
            let created = fs::read_dir(fixture.root.path())
                .expect("temporary root")
                .map(|entry| entry.expect("entry").path())
                .find(|path| {
                    path.file_name().expect("name").to_string_lossy().starts_with("package-call-")
                })
                .expect("this invocation root");
            held = Some(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .share_mode(0)
                    .open(created.join("held.fixture"))
                    .expect("owned temporary cleanup fault"),
            );
            owned_root = Some(created);
        },
    )
    .expect("process result with cleanup observation");
    assert!(result.started && result.stop_confirmed);
    assert_eq!(result.reason, Reason::CleanupUnconfirmed);
    drop(held);
    let owned = fs::canonicalize(owned_root.expect("owned call root")).expect("still present");
    assert!(
        owned.starts_with(fs::canonicalize(fixture.root.path()).expect("temporary parent"))
            && owned.file_name().expect("name").to_string_lossy().starts_with("package-call-")
    );
    fs::remove_dir_all(owned).expect("only verified owned temporary tree");
    fixture.assert_scratch_clean();
}

#[test]
fn an_unsettled_case_stops_later_conformance_processes() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new(Kind::Command, |raw| {
        raw["tools"][0]["conformance_cases"] =
            serde_json::json!([{"name":"first","arguments":{}},{"name":"second","arguments":{}}])
    });
    let content =
        aura_tool_package::package_content_descriptor_v1(&fixture.staged).expect("descriptor");
    let version = PackageVersionBindingV1 {
        package_id: ToolPackageId::new(),
        package_version: content.manifest.package_version,
        package_fingerprint: content.package_fingerprint,
        revision: Revision::INITIAL,
    };
    let mut started = 0;
    let mut held = None;
    let mut owned_root = None;
    let (report, runs) = run_conformance(
        &fixture.staged,
        PackageConformanceRequest {
            version,
            run_id: ToolConformanceRunId::new(),
            started_at: UtcTimestamp::now(),
            settings: Vec::new(),
        },
        &fixture.context(),
        &AtomicBool::new(false),
        &mut |_, _, _| {
            started += 1;
            if started == 1 {
                let created = fs::read_dir(fixture.root.path())
                    .expect("temporary root")
                    .map(|entry| entry.expect("entry").path())
                    .find(|path| {
                        path.file_name()
                            .expect("name")
                            .to_string_lossy()
                            .starts_with("package-call-")
                    })
                    .expect("this invocation root");
                held = Some(
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .share_mode(0)
                        .open(created.join("held.fixture"))
                        .expect("owned temporary cleanup fault"),
                );
                owned_root = Some(created);
            }
        },
    )
    .expect("interrupted report");
    assert_eq!(started, 1);
    assert_eq!(runs.len(), 1);
    assert_eq!(report.state(), PackageConformanceStateV1::Interrupted);
    assert!(report.cases.iter().all(|case| case.reason == Reason::CleanupUnconfirmed));
    drop(held);
    let owned = fs::canonicalize(owned_root.expect("owned root")).expect("still present");
    assert!(
        owned.starts_with(fs::canonicalize(fixture.root.path()).expect("temporary parent"))
            && owned.file_name().expect("name").to_string_lossy().starts_with("package-call-")
    );
    fs::remove_dir_all(owned).expect("only verified owned temporary tree");
    fixture.assert_scratch_clean();
}
