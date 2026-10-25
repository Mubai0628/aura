use super::*;
use crate::package_program::tests::support::{Fixture, Kind};
use aura_domain_common::RuntimeInstanceId;
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_tool_domain::*;

struct Harness {
    fixture: Fixture,
    services: PackageServices,
    store: Arc<Mutex<V2Store>>,
    persistence: Arc<PersistenceCoordinator>,
    _persistence: tempfile::TempDir,
}

fn harness(kind: Kind) -> Harness {
    let fixture = Fixture::new(kind, |_| {});
    let data = tempfile::tempdir().expect("temporary V1 data");
    let PersistenceStartup::Ready(persistence) =
        PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("data/aura.db"),
            data.path().join("data/artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence")
    else {
        panic!("fresh persistence requires no recovery")
    };
    let store = Arc::new(Mutex::new(V2Store::create(&fixture.root.path().join("v2")).expect("V2")));
    let persistence = Arc::new(persistence);
    let services = PackageServices::open(
        store.clone(),
        persistence.clone(),
        &fixture.root.path().join("packages"),
    )
    .expect("one package owner");
    Harness { fixture, services, store, persistence, _persistence: data }
}

fn payloads(store: &Arc<Mutex<V2Store>>) -> i64 {
    store
        .lock()
        .expect("store")
        .database
        .connection()
        .query_row("SELECT count(*) FROM tool_package_payloads", [], |row| row.get(0))
        .expect("payload count")
}

fn source(harness: &Harness) -> std::path::PathBuf {
    harness.fixture.root.path().join("source")
}

fn imported(harness: &Harness) -> PackageVersionViewV1 {
    match harness.services.import(ImportSource::Directory, &source(harness)).expect("import") {
        PackageImportOutcomeV1::Imported { view } => *view,
        other => panic!("fixture must import: {other:?}"),
    }
}

fn reference(view: &PackageVersionViewV1) -> PackageVersionRefV1 {
    PackageVersionRefV1 {
        package_id: view.preview.version.package_id,
        package_version: view.preview.version.package_version.clone(),
        package_fingerprint: view.preview.version.package_fingerprint.clone(),
    }
}

fn environment(harness: &Harness) -> RunEnvironment {
    RunEnvironment {
        powershell: None,
        user_environment: harness.fixture.context().user_environment,
    }
}

fn finished(harness: &Harness, view: &PackageVersionViewV1) -> PackageConformanceViewV1 {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let current = harness.services.conformance(&reference(view)).expect("read run");
        if current.state != PackageConformanceStateV1::Running {
            return current;
        }
        assert!(Instant::now() < until, "Conformance must settle");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn approved_run(harness: &Harness, view: &PackageVersionViewV1) -> PackageConformanceViewV1 {
    let prepared = harness
        .services
        .prepare(
            &PackageConformancePrepareRequestV1 {
                version: view.preview.version.clone(),
                settings: vec![],
            },
            environment(harness),
        )
        .expect("concrete preview");
    harness
        .services
        .start(&PackageConformanceStartRequestV1 {
            preparation_id: prepared.preparation_id,
            preview_hash: prepared.preview_hash,
        })
        .expect("approved start");
    finished(harness, view)
}

#[test]
fn a_rejected_source_reports_diagnostics_and_stores_nothing() {
    let harness = harness(Kind::Http);
    std::fs::write(source(&harness).join(aura_tool_contract::TOOL_PACKAGE_MANIFEST_NAME), b"{")
        .expect("break the manifest");
    let outcome =
        harness.services.import(ImportSource::Directory, &source(&harness)).expect("import");
    let PackageImportOutcomeV1::Rejected { diagnostics } = outcome else {
        panic!("broken manifest must be rejected")
    };
    assert_eq!(diagnostics[0].code, PackageImportDiagnosticCodeV1::InvalidManifest);
    assert!(harness.services.list().expect("list").packages.is_empty());
}

#[test]
fn only_the_exact_preview_hash_starts_a_run_and_only_once() {
    let harness = harness(Kind::Http);
    let view = imported(&harness);
    assert_eq!(view.conformance.state, PackageConformanceStateV1::NotRun);
    let prepared = harness
        .services
        .prepare(
            &PackageConformancePrepareRequestV1 {
                version: view.preview.version.clone(),
                settings: vec![],
            },
            environment(&harness),
        )
        .expect("prepare");
    let wrong = PackageConformanceStartRequestV1 {
        preparation_id: prepared.preparation_id.clone(),
        preview_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"another preview"),
    };
    assert!(harness.services.start(&wrong).is_err());
    let replay = PackageConformanceStartRequestV1 {
        preparation_id: prepared.preparation_id,
        preview_hash: prepared.preview_hash,
    };
    assert!(
        harness.services.start(&replay).is_err(),
        "a refused approval consumed the preparation"
    );
    assert_eq!(
        harness.services.conformance(&reference(&view)).expect("run").state,
        PackageConformanceStateV1::NotRun
    );
}

#[test]
fn http_package_installs_only_with_the_exact_report_and_lifecycle_changes_take_effect() {
    let harness = harness(Kind::Http);
    let view = imported(&harness);
    let run = approved_run(&harness, &view);
    assert_eq!(run.state, PackageConformanceStateV1::Succeeded, "{:?}", run.report);
    let listed = harness.services.list().expect("list");
    assert!(listed.packages[0].installed.is_none(), "a passing run is not an install");
    let revision = listed.packages[0].revision;
    let install = PackageInstallRequestV1 {
        version: run.version.clone(),
        expected_package_revision: revision,
        report_hash: run.report_hash.clone().expect("finished run hash"),
    };
    let summary = harness.services.install(&install).expect("explicit install");
    let installed = summary.installed.clone().expect("installed");
    assert!(installed.enabled && installed.trusted);
    let tool = &installed.tools[0];
    assert_eq!(tool.execution_location, PackageExecutionLocationV1::ControlledHttp);
    let slot = tool.credential.as_ref().expect("fixture declares a bearer slot");
    assert_eq!(slot.origin, "https://example.invalid");
    assert!(slot.binding.is_none());
    assert!(harness.services.install(&install).is_err(), "the revision the user saw has moved on");
    let disabled = harness
        .services
        .set_enabled(&PackageEnableRequestV1 {
            package_id: summary.package_id,
            expected_package_revision: summary.revision,
            enabled: false,
        })
        .expect("disable");
    assert!(!disabled.installed.as_ref().expect("still installed").enabled);
    let revoked = harness
        .services
        .revoke_trust(&PackageChangeRequestV1 {
            package_id: summary.package_id,
            expected_package_revision: disabled.revision,
        })
        .expect("revoke");
    assert!(!revoked.installed.as_ref().expect("still installed").trusted);
    assert!(
        harness
            .services
            .set_enabled(&PackageEnableRequestV1 {
                package_id: summary.package_id,
                expected_package_revision: revoked.revision,
                enabled: true,
            })
            .is_err(),
        "enabling never restores trust"
    );
    let removed = harness
        .services
        .uninstall(&PackageChangeRequestV1 {
            package_id: summary.package_id,
            expected_package_revision: revoked.revision,
        })
        .expect("uninstall");
    assert!(removed.installed.is_none());
    assert_eq!(payloads(&harness.store), 0, "no running use: content is collected at once");
}

#[test]
fn a_restart_releases_a_dead_calls_reference_and_collects_uninstalled_content() {
    let harness = harness(Kind::Http);
    let view = imported(&harness);
    let run = approved_run(&harness, &view);
    let revision = harness.services.list().expect("list").packages[0].revision;
    let summary = harness
        .services
        .install(&PackageInstallRequestV1 {
            version: run.version.clone(),
            expected_package_revision: revision,
            report_hash: run.report_hash.clone().expect("finished run hash"),
        })
        .expect("install");
    // A call was running when the previous Core ended: its strong reference is left behind.
    harness
        .store
        .lock()
        .expect("store")
        .database
        .unit_of_work(|tx| {
            let fingerprint: String =
                tx.query_row("SELECT fingerprint FROM tool_package_payloads", [], |row| {
                    row.get(0)
                })?;
            aura_persistence_v2::packages::references::retain_for_call(
                tx,
                &fingerprint,
                "sop_crashed_call",
            )
        })
        .expect("a running call's reference");
    harness
        .services
        .uninstall(&PackageChangeRequestV1 {
            package_id: summary.package_id,
            expected_package_revision: summary.revision,
        })
        .expect("uninstall");
    assert_eq!(payloads(&harness.store), 1, "referenced content is never collected");

    let Harness { fixture, services, store, persistence, _persistence } = harness;
    drop(services);
    let restarted =
        PackageServices::open(store.clone(), persistence, &fixture.root.path().join("packages"))
            .expect("the next Core takes the package owner");
    restarted.collect().expect("startup collection");
    assert_eq!(
        payloads(&store),
        0,
        "the dead call's reference is released and its content collected"
    );
}

#[test]
fn program_conformance_runs_in_its_own_scratch_workspace() {
    let harness = harness(Kind::Command);
    let view = imported(&harness);
    let prepared = harness
        .services
        .prepare(
            &PackageConformancePrepareRequestV1 {
                version: view.preview.version.clone(),
                settings: vec![],
            },
            environment(&harness),
        )
        .expect("prepare");
    let PackageWorkingDirectoryPreviewV1::Resolved { path, .. } =
        &prepared.preview.runtimes[0].working_directory
    else {
        panic!("the actual run directory is shown")
    };
    assert!(std::path::Path::new(path).starts_with(harness.fixture.root.path().join("packages")));
    harness
        .services
        .start(&PackageConformanceStartRequestV1 {
            preparation_id: prepared.preparation_id,
            preview_hash: prepared.preview_hash,
        })
        .expect("start");
    let run = finished(&harness, &view);
    assert_eq!(run.state, PackageConformanceStateV1::Succeeded, "{:?}", run.report);
}
