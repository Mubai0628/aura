//! Each finished maintenance run is one line in a plain text log beside the Vault's records,
//! for the owner to read in Aura or in any editor; the project Vault names the file.
#[allow(dead_code)] // Shared file fixture also supports write scenarios in other binaries.
mod support;
use aura_domain_common::{RuntimeInstanceId, SessionId, UtcTimestamp};
use aura_memory::*;
use support::Fixture;

fn run(
    id: &str,
    runtime: RuntimeInstanceId,
    state: MemoryMaintenanceState,
) -> MemoryMaintenanceRun {
    let finished =
        !matches!(state, MemoryMaintenanceState::Queued | MemoryMaintenanceState::Running);
    MemoryMaintenanceRun {
        run_id: blake3::hash(id.as_bytes()).to_hex().to_string(),
        runtime_instance_id: runtime,
        session_id: SessionId::new(),
        turn_id: None,
        state,
        reason: None,
        model_profile_id: None,
        input_tokens: 120,
        output_tokens: 30,
        usage_complete: true,
        started_at: UtcTimestamp::now(),
        finished_at: finished.then(UtcTimestamp::now),
    }
}

fn log_of(f: &Fixture) -> std::path::PathBuf {
    f.temp.path().join("cache/memory.records/maintenance.log")
}

#[test]
fn a_finished_run_is_one_line_in_the_log_and_the_vault_names_the_file() {
    let f = Fixture::project();
    let runtime = RuntimeInstanceId::new();
    let log = log_of(&f);
    assert!(f.vault.desktop_status().expect("status").maintenance_log.is_none(), "no log yet");

    let mut first = run("first", runtime, MemoryMaintenanceState::Running);
    f.vault.record_maintenance_run(&first).expect("running");
    assert!(!log.exists(), "a run in progress is not logged");
    first.state = MemoryMaintenanceState::Completed;
    first.reason = Some("新增 2 条，\n更新 1 条".into());
    first.finished_at = Some(UtcTimestamp::now());
    f.vault.record_maintenance_run(&first).expect("completed");
    f.vault.record_maintenance_run(&first).expect("the same settled record again");
    let mut skipped = run("second", runtime, MemoryMaintenanceState::Skipped);
    skipped.reason = Some("只读会话未整理".into());
    skipped.usage_complete = false;
    f.vault.record_maintenance_run(&skipped).expect("skipped");

    let text = std::fs::read_to_string(&log).expect("the log");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "one line per finished run, never repeated: {text}");
    assert!(lines[0].contains("已完成") && lines[0].contains("新增 2 条， 更新 1 条"), "{text}");
    assert!(lines[0].contains(&first.session_id.to_string()) && lines[0].contains("输入 120"));
    assert!(lines[1].contains("已跳过") && lines[1].contains("只读会话未整理"), "{text}");
    assert!(lines[1].contains("用量未确认"), "{text}");

    let named = f.vault.desktop_status().expect("status").maintenance_log.expect("named");
    assert!(!named.starts_with(r"\\?\"), "{named}");
    assert!(named.ends_with(r"memory.records\maintenance.log"), "{named}");
}

#[test]
fn a_run_an_earlier_core_left_unfinished_is_logged_as_interrupted() {
    let f = Fixture::project();
    let earlier = RuntimeInstanceId::new();
    f.vault
        .record_maintenance_run(&run("left", earlier, MemoryMaintenanceState::Running))
        .expect("running");
    f.vault.maintenance_runs(RuntimeInstanceId::new()).expect("a later Core settles it");
    let text = std::fs::read_to_string(log_of(&f)).expect("the log");
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.contains("已中断"), "{text}");
}

#[test]
fn past_one_mebibyte_the_log_starts_over_beside_its_previous_part() {
    let f = Fixture::project();
    let runtime = RuntimeInstanceId::new();
    f.vault
        .record_maintenance_run(&run("old", runtime, MemoryMaintenanceState::Skipped))
        .expect("first line");
    let log = log_of(&f);
    let filler = "x".repeat(1024 * 1024);
    std::fs::write(&log, format!("{}{filler}\n", std::fs::read_to_string(&log).expect("log")))
        .expect("a full log");
    f.vault
        .record_maintenance_run(&run("new", runtime, MemoryMaintenanceState::Completed))
        .expect("next line");
    let previous = log.with_file_name("maintenance.1.log");
    assert!(std::fs::metadata(&previous).expect("the previous part").len() > 1024 * 1024);
    let text = std::fs::read_to_string(&log).expect("the new log");
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.contains("已完成"), "{text}");
}
