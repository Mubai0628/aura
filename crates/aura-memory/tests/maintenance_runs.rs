#[allow(dead_code)] // Shared file fixture also supports write scenarios in other binaries.
mod support;
use aura_domain_common::{RuntimeInstanceId, SessionId, UtcTimestamp};
use aura_memory::*;
use support::Fixture;
#[test]
fn skipped_and_interrupted_runs_survive_cache_rebuild_without_creating_vault() {
    let f = Fixture::project();
    let current = RuntimeInstanceId::new();
    let mut run = MemoryMaintenanceRun {
        run_id: blake3::hash(b"run").to_hex().to_string(),
        runtime_instance_id: current,
        session_id: SessionId::new(),
        turn_id: None,
        state: MemoryMaintenanceState::Skipped,
        reason: Some("只读会话未整理".into()),
        model_profile_id: None,
        input_tokens: 0,
        output_tokens: 0,
        usage_complete: true,
        started_at: UtcTimestamp::now(),
        finished_at: Some(UtcTimestamp::now()),
    };
    f.vault.record_maintenance_run(&run).expect("metadata only");
    assert!(!f.root.exists());
    f.vault.rebuild().expect("rebuild");
    let rows = f.vault.maintenance_runs(current).expect("derived cache");
    assert_eq!(rows[0].reason, run.reason);
    let first_id = run.run_id.clone();
    run.state = MemoryMaintenanceState::Queued;
    assert!(f.vault.record_maintenance_run(&run).is_err(), "settled work cannot be queued again");
    run.run_id = blake3::hash(b"unfinished").to_hex().to_string();
    run.state = MemoryMaintenanceState::Running;
    run.usage_complete = false;
    run.finished_at = None;
    run.input_tokens = 43;
    f.vault.record_maintenance_run(&run).expect("running");
    let rows = f.vault.maintenance_runs(RuntimeInstanceId::new()).expect("restart");
    assert!(rows.iter().any(|r| r.state == MemoryMaintenanceState::Interrupted
        && r.input_tokens == 43
        && !r.usage_complete));
    assert!(!f.root.exists(), "neither a skip nor recovery publishes a note");
    run.input_tokens = u64::MAX;
    assert!(
        f.vault.record_maintenance_run(&run).is_err(),
        "overflow rejected before metadata publication"
    );
    let metadata = std::fs::read_to_string(
        f.temp.path().join("cache/memory.records/maintenance").join(format!("{}.json", run.run_id)),
    )
    .expect("source run metadata");
    assert!(!metadata.contains("body") && !metadata.contains("candidates"));
    assert!(f.vault.maintenance_run(&first_id).expect("stable identity").is_some());
    for i in 0..101 {
        let mut later = run.clone();
        later.run_id = blake3::hash(format!("later-{i}").as_bytes()).to_hex().to_string();
        later.input_tokens = 0;
        later.state = MemoryMaintenanceState::Skipped;
        later.started_at = UtcTimestamp::now();
        later.finished_at = Some(later.started_at);
        f.vault.record_maintenance_run(&later).expect("later metadata");
    }
    let recent = f.vault.maintenance_runs(current).expect("bounded display");
    assert_eq!(recent.len(), 100);
    assert!(!recent.iter().any(|r| r.run_id == first_id));
    assert!(
        f.vault.maintenance_run(&first_id).expect("durable dedup outside display page").is_some()
    );
}

#[test]
fn only_a_bounded_number_of_finished_runs_stays_on_disk() {
    let f = Fixture::project();
    let runtime = RuntimeInstanceId::new();
    let base = UtcTimestamp::now().value();
    let mut ids = vec![];
    for i in 0..260i64 {
        let at = UtcTimestamp::from_value(base + time::Duration::seconds(i));
        let run = MemoryMaintenanceRun {
            run_id: blake3::hash(format!("bounded-{i}").as_bytes()).to_hex().to_string(),
            runtime_instance_id: runtime,
            session_id: SessionId::new(),
            turn_id: None,
            state: MemoryMaintenanceState::Completed,
            reason: None,
            model_profile_id: None,
            input_tokens: 1,
            output_tokens: 1,
            usage_complete: true,
            started_at: at,
            finished_at: Some(at),
        };
        f.vault.record_maintenance_run(&run).expect("record");
        ids.push(run.run_id);
    }
    let files = std::fs::read_dir(f.temp.path().join("cache/memory.records/maintenance"))
        .expect("run records")
        .count();
    // The 251st run pruned the store back to 200; nine more followed.
    assert_eq!(files, 209, "older finished runs are pruned instead of reaching the read bound");
    assert!(f.vault.maintenance_run(&ids[259]).expect("newest").is_some());
    assert_eq!(f.vault.maintenance_runs(runtime).expect("list").len(), 100);
    assert!(f.vault.maintenance_run(&ids[0]).expect("oldest").is_none());
    f.vault.rebuild().expect("rebuild still succeeds");
    assert_eq!(f.vault.maintenance_runs(runtime).expect("rebuilt list")[0].run_id, ids[259]);
}
