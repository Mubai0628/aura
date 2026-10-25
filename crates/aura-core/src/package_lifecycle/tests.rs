use super::*;
use crate::package_program::tests::support;
use aura_domain_common::{Revision, ToolConformanceRunId, UtcTimestamp};
use aura_persistence_v2::{V2Store, packages};
use aura_tool_domain::PackageConformanceStateV1;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use support::{Fixture, Kind};

fn service(fixture: &Fixture) -> (PackageConformanceOwner, Arc<Mutex<V2Store>>) {
    let store = Arc::new(Mutex::new(V2Store::create(&fixture.root.path().join("v2")).expect("V2")));
    let service = PackageConformanceOwner::open(store.clone(), &fixture.root.path().join("owner"))
        .expect("one private owner");
    (service, store)
}

fn options(fixture: &Fixture) -> PackageRunOptions {
    let context = fixture.context();
    PackageRunOptions {
        workspace_root: context.workspace_root.map(std::path::Path::to_owned),
        directory_claims: context.directory_claims,
        settings: Vec::new(),
        powershell: None,
        user_environment: context.user_environment,
        timeout: context.timeout,
        max_output_bytes: context.max_output_bytes,
    }
}

fn wait(handle: &PackageConformanceHandle) {
    let until = Instant::now() + Duration::from_secs(8);
    while handle.status().expect("status").running {
        assert!(Instant::now() < until, "background operation must settle");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(handle.status().expect("status").failure_code.is_none());
}

#[test]
fn background_native_conformance_survives_the_start_call_and_does_not_install() {
    let fixture = Fixture::new(Kind::Command, |_| {});
    let (owner, store) = service(&fixture);
    let version = owner.import_staged(&fixture.staged).expect("persist source");
    let prepared = owner.prepare(&version, options(&fixture)).expect("concrete preview");
    assert_eq!(owner.active_count().expect("not running before approval"), 0);
    let preview = prepared.preview().expect("resolved preview");
    let aura_tool_domain::PackageWorkingDirectoryPreviewV1::Resolved { path, .. } =
        &preview.runtimes[0].working_directory
    else {
        panic!("actual package root must be shown")
    };
    assert!(std::path::Path::new(path).starts_with(fixture.root.path().join("owner")));
    assert_ne!(std::path::Path::new(path), fixture.staged.root());
    assert_eq!(
        prepared.preview_hash().expect("approval hash"),
        prepared.preview_hash().expect("stable while retained")
    );
    let handle = owner.start(prepared).expect("explicit user start");
    assert_ne!(handle.version.revision, version.revision);
    wait(&handle);
    let run = owner.report(&handle.version).expect("retained terminal").expect("run");
    let report = run.report.expect("real process report");
    assert_eq!(report.state(), PackageConformanceStateV1::Succeeded);
    let current =
        packages::state(store.lock().expect("store").database.connection(), version.package_id)
            .expect("package")
            .expect("imported");
    assert!(!current.installed && !current.trusted && !current.enabled);
    assert_eq!(owner.active_count().expect("owner"), 0);
    assert_eq!(owner.scratch_count().expect("owned roots"), 0);
}

#[test]
fn dropping_the_owner_cancels_the_actual_job_and_keeps_its_durable_result() {
    let fixture = Fixture::new(Kind::Command, |raw| {
        raw["tools"][0]["runtime"]["command"]["arguments"] =
            serde_json::json!([{"type":"literal","value":"--wait-for-cancel"}]);
    });
    let (owner, store) = service(&fixture);
    let version = owner.import_staged(&fixture.staged).expect("import");
    let handle =
        owner.start(owner.prepare(&version, options(&fixture)).expect("prepare")).expect("start");
    let until = Instant::now() + Duration::from_secs(5);
    while handle.status().expect("status").process_id.is_none() {
        assert!(Instant::now() < until, "real process must start");
        thread::sleep(Duration::from_millis(10));
    }
    drop(owner);
    wait(&handle);
    let run = packages::conformance::conformance(
        store.lock().expect("store").database.connection(),
        &handle.version,
    )
    .expect("persisted run")
    .expect("record");
    assert_eq!(run.report.expect("confirmed stop").state(), PackageConformanceStateV1::Cancelled);
    let reopened = PackageConformanceOwner::open(store, &fixture.root.path().join("owner"))
        .expect("old operation released its owner lease");
    assert_eq!(reopened.recovery().interrupted_runs, 0);
    assert_eq!(reopened.scratch_count().expect("scratch"), 0);
}

#[test]
fn startup_interrupts_unfinished_records_and_never_replays_them_or_installs_them() {
    let fixture = Fixture::new(Kind::Command, |_| {});
    let (owner, store) = service(&fixture);
    let version = owner.import_staged(&fixture.staged).expect("import");
    let version = store
        .lock()
        .expect("store")
        .database
        .unit_of_work(|tx| {
            packages::conformance::begin_conformance(
                tx,
                &version,
                ToolConformanceRunId::new(),
                &[],
                UtcTimestamp::now(),
            )
        })
        .expect("simulate the durable pre-launch boundary");
    drop(owner);
    let owner = PackageConformanceOwner::open(store.clone(), &fixture.root.path().join("owner"))
        .expect("startup recovery after old Core and Jobs are gone");
    assert_eq!(owner.recovery().interrupted_runs, 1);
    assert_eq!(owner.active_count().expect("no replay"), 0);
    let recovered = aura_tool_domain::PackageVersionBindingV1 {
        revision: Revision::new(version.revision.value() + 1),
        ..version
    };
    let run = owner.report(&recovered).expect("interrupted record").expect("run");
    assert!(run.report.is_none());
    let handle = owner
        .start(owner.prepare(&run.version, options(&fixture)).expect("new explicit preview"))
        .expect("only an explicit new start can launch another run");
    wait(&handle);
}

#[test]
fn a_second_owner_cannot_interrupt_a_live_run_or_take_over_its_scratch() {
    let fixture = Fixture::new(Kind::Command, |_| {});
    let (owner, store) = service(&fixture);
    assert!(PackageConformanceOwner::open(store, &fixture.root.path().join("owner")).is_err());
    assert_eq!(owner.recovery().interrupted_runs, 0);
}

#[test]
fn cancelling_a_concrete_preview_starts_nothing_and_cleans_only_its_owned_root() {
    let fixture = Fixture::new(Kind::Command, |_| {});
    let (owner, _) = service(&fixture);
    let version = owner.import_staged(&fixture.staged).expect("import");
    let prepared = owner.prepare(&version, options(&fixture)).expect("prepare");
    assert_eq!(owner.scratch_count().expect("one retained preview"), 1);
    prepared.cancel().expect("observed pre-launch cleanup");
    assert_eq!(owner.scratch_count().expect("no leftovers"), 0);
    assert_eq!(owner.active_count().expect("no job"), 0);
    assert!(owner.report(&version).expect("no mutation").is_none());
}

#[test]
fn a_concrete_preparation_cannot_be_started_by_another_owner() {
    let fixture = Fixture::new(Kind::Command, |_| {});
    let (owner, store) = service(&fixture);
    let version = owner.import_staged(&fixture.staged).expect("import");
    let prepared = owner.prepare(&version, options(&fixture)).expect("prepare");
    let other_store = Arc::new(Mutex::new(
        V2Store::create(&fixture.root.path().join("other-v2")).expect("other private store"),
    ));
    let other =
        PackageConformanceOwner::open(other_store, &fixture.root.path().join("other-owner"))
            .expect("another namespace");
    assert!(other.start(prepared).is_err());
    assert_eq!(other.active_count().expect("no job"), 0);
    assert!(
        packages::conformance::conformance(
            store.lock().expect("store").database.connection(),
            &version
        )
        .expect("no start")
        .is_none()
    );
}

#[test]
fn startup_removes_owned_scratch_without_following_inner_junctions_and_rejects_a_link_root() {
    let fixture = Fixture::new(Kind::Command, |_| {});
    let (owner, store) = service(&fixture);
    drop(owner);
    let private = fixture.root.path().join("owner");
    let outside = fixture.root.path().join("outside");
    std::fs::create_dir(&outside).expect("temporary outside fixture");
    std::fs::write(outside.join("keep"), b"synthetic outside data").expect("fixture");
    let owned = private.join(format!("package-run-{}", "a".repeat(32)));
    std::fs::create_dir(&owned).expect("only owned scratch");
    junction::create(&outside, owned.join("link")).expect("temporary junction");
    let reopened =
        PackageConformanceOwner::open(store.clone(), &private).expect("safe scratch recovery");
    assert_eq!(reopened.recovery().removed_scratch_roots, 1);
    assert_eq!(
        std::fs::read(outside.join("keep")).expect("must survive"),
        b"synthetic outside data"
    );
    drop(reopened);
    junction::create(&outside, &owned).expect("temporary bad root");
    assert!(
        PackageConformanceOwner::open(store, &private).is_err(),
        "a root link is never adopted or recursively cleaned"
    );
    assert_eq!(
        std::fs::read(outside.join("keep")).expect("still survives"),
        b"synthetic outside data"
    );
    junction::delete(&owned).expect("remove only the link created by this test");
}
