use crate::{
    engine::apply_device_observation,
    journal_stage::{
        JournalStageDecision, decide_journal_stage, load_journal_bytes, staged_continues_committed,
    },
    migrate::{
        MIGRATION_BINDING_VERSION, PreGenerationMigrationBinding, RestoreAction,
        bind_original_journal, error_retained, lock_migration_file, plan_restore,
    },
    policy,
};
use aura_domain_common::{ContentHash, UtcTimestamp};
use aura_windows_platform::ExecutionDevice;
use aura_windows_platform::execution_maintenance::{
    migration_service_quiesced, process_query_confirms_absence,
};
use aura_windows_user_contract::*;

const ID: &str = "4b486eb4-dd7f-4681-b8ac-7eece6ce8d25";
const BOOT: &str = "6b675559-aa09-11f1-8d48-00155d006a00";
const GEN: &str = "driver:null.sys:fffff806725b0000";
const BEFORE: &str = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)";
const LIVE: &str = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)(A;;0x1201bf;;;AC)(A;;0x1201bf;;;S-1-15-2-2)";

fn config() -> MaintenanceConfig {
    MaintenanceConfig {
        schema: MAINTENANCE_VERSION.into(),
        installation_id: ID.into(),
        owner_sid: "S-1-5-21-1-2-3-1001".into(),
        offline_sid: "S-1-5-21-1-2-3-1002".into(),
        online_sid: Some("S-1-5-21-1-2-3-1003".into()),
        initial_resources: vec![],
    }
}

fn drifted_journal(after: String) -> MaintenanceJournal {
    MaintenanceJournal {
        schema: MAINTENANCE_VERSION.into(),
        config_sha256: ContentHash::sha256(b"cfg"),
        resources: vec![
            MaintenanceResourceReceipt {
                boot_id: BOOT.into(),
                target: MaintenanceTarget::Null,
                parent_dacl: None,
                before: BEFORE.into(),
                after,
                state: MaintenanceResourceState::Applied,
                object_generation: None,
            },
            MaintenanceResourceReceipt {
                boot_id: BOOT.into(),
                target: MaintenanceTarget::Ksec,
                parent_dacl: None,
                before: BEFORE.into(),
                after: ExecutionDevice::Ksec.plan(ID, BEFORE, true).expect("ksec"),
                state: MaintenanceResourceState::Applied,
                object_generation: None,
            },
        ],
        observed_at: UtcTimestamp::now(),
        error: Some("maintenance.acl_drift".into()),
    }
}

fn binding(journal: &MaintenanceJournal, observed: &str) -> PreGenerationMigrationBinding {
    let bytes = serde_json::to_vec(journal).expect("bytes");
    PreGenerationMigrationBinding {
        schema: MIGRATION_BINDING_VERSION.into(),
        installation_id: ID.into(),
        config_sha256: journal.config_sha256.clone(),
        journal_sha256: ContentHash::sha256(&bytes),
        boot_id: BOOT.into(),
        target: MaintenanceTarget::Null,
        observed_dacl: observed.into(),
        live_generation: GEN.into(),
    }
}

fn restore(
    journal: MaintenanceJournal,
    original: &MaintenanceJournal,
    token: &PreGenerationMigrationBinding,
    live: &str,
) -> anyhow::Result<crate::migrate::RestorePlan> {
    plan_restore(&config(), journal, original, token, live, GEN, BOOT)
}

#[test]
fn restore_adds_only_the_specified_aura_ace_and_keeps_bound_live_aces() {
    let recorded_after = ExecutionDevice::Null.plan(ID, BEFORE, true).expect("recorded");
    let journal = drifted_journal(recorded_after);
    let token = binding(&journal, LIVE);
    let plan = restore(journal.clone(), &journal, &token, LIVE).expect("plan");
    assert_eq!(plan.action, RestoreAction::PersistIntentAndApply);
    assert!(error_retained(&plan.journal));
    assert_eq!(plan.journal.resources[0].state, MaintenanceResourceState::Retired);
    assert_eq!(plan.journal.resources[0].object_generation, None);
    assert_eq!(plan.journal.resources[0].before, BEFORE);
    assert_eq!(plan.journal.resources[1].target, MaintenanceTarget::Ksec);
    assert_eq!(plan.journal.resources[1], journal.resources[1]);
    let successor = &plan.journal.resources[2];
    assert_eq!(successor.state, MaintenanceResourceState::Pending);
    assert_eq!(successor.object_generation.as_deref(), Some(GEN));
    assert_eq!(successor.after, plan.after);
    assert_ne!(successor.before, BEFORE);
}

#[test]
fn restore_refuses_when_an_existing_ace_changes_mask_or_flags() {
    let journal = drifted_journal(ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after"));
    let token = binding(&journal, LIVE);
    let changed_ac = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)(A;;GR;;;AC)(A;;0x1201bf;;;S-1-15-2-2)";
    let error = restore(journal.clone(), &journal, &token, changed_ac).expect_err("mask");
    assert!(error.to_string().contains("maintenance.migration_live_dacl_changed"), "{error}");
}

#[test]
fn daily_maintainer_still_refuses_missing_generation_drift() {
    let receipt = MaintenanceResourceReceipt {
        boot_id: BOOT.into(),
        target: MaintenanceTarget::Null,
        parent_dacl: None,
        before: BEFORE.into(),
        after: ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after"),
        state: MaintenanceResourceState::Applied,
        object_generation: None,
    };
    assert_eq!(
        policy::device_generation(&receipt, LIVE, Some(GEN), ID, ExecutionDevice::Null)
            .expect("gen"),
        policy::DeviceGeneration::UnrelatedDrift
    );
    let c = config();
    let mut journal = drifted_journal(receipt.after.clone());
    let error = apply_device_observation(
        &mut journal,
        &c,
        BOOT,
        MaintenanceTarget::Null,
        None,
        LIVE,
        Some(GEN.into()),
    )
    .expect_err("daily");
    assert!(error.to_string().contains("maintenance.acl_drift"), "{error}");
    assert_eq!(journal.resources.len(), 2);
    assert_eq!(journal.resources[0].state, MaintenanceResourceState::Applied);
}

#[test]
fn intent_resume_does_not_reauthorize_or_drop_the_old_receipt() {
    let original = drifted_journal(ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after"));
    let token = binding(&original, LIVE);
    let first = restore(original.clone(), &original, &token, LIVE).expect("first");
    let resumed = restore(first.journal.clone(), &original, &token, LIVE).expect("resume");
    assert_eq!(resumed.action, RestoreAction::PersistIntentAndApply);
    assert_eq!(resumed.journal.resources.len(), 3);
    assert_eq!(resumed.journal.resources[0].state, MaintenanceResourceState::Retired);
    assert_eq!(resumed.journal.resources[0].before, BEFORE);
    assert_eq!(resumed.journal.resources[1].target, MaintenanceTarget::Ksec);
    assert_eq!(resumed.journal.resources[2].state, MaintenanceResourceState::Pending);
    assert!(error_retained(&resumed.journal));
}

#[test]
fn acl_written_without_receipt_backfills_applied_and_keeps_error() {
    let original = drifted_journal(ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after"));
    let token = binding(&original, LIVE);
    let intended = restore(original.clone(), &original, &token, LIVE).expect("intent");
    let recovered =
        restore(original.clone(), &original, &token, &intended.after).expect("backfill");
    assert_eq!(recovered.action, RestoreAction::PersistAppliedOnly);
    assert_eq!(recovered.journal.resources[0].state, MaintenanceResourceState::Retired);
    assert_eq!(recovered.journal.resources[2].state, MaintenanceResourceState::Applied);
    assert!(error_retained(&recovered.journal));
}

#[test]
fn pending_receipt_with_live_after_marks_applied_without_a_second_successor() {
    let original = drifted_journal(ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after"));
    let token = binding(&original, LIVE);
    let intended = restore(original.clone(), &original, &token, LIVE).expect("intent");
    let planned = intended.after.clone();
    let recovered = restore(intended.journal, &original, &token, &planned).expect("mark");
    assert_eq!(recovered.action, RestoreAction::PersistAppliedOnly);
    assert_eq!(
        recovered
            .journal
            .resources
            .iter()
            .filter(|item| item.state != MaintenanceResourceState::Retired)
            .count(),
        2
    );
}

#[test]
fn original_backup_missing_or_wrong_hash_refuses() {
    let journal = drifted_journal(ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after"));
    let bytes = serde_json::to_vec(&journal).expect("bytes");
    let expected = ContentHash::sha256(&bytes);
    let missing = bind_original_journal(b"other-journal", &expected, None).expect_err("missing");
    assert!(missing.to_string().contains("maintenance.migration_original_missing"), "{missing}");
    let changed =
        bind_original_journal(&bytes, &expected, Some(b"tampered-backup")).expect_err("hash");
    assert!(changed.to_string().contains("maintenance.migration_original_changed"), "{changed}");
    let restored = bind_original_journal(b"later-journal", &expected, Some(&bytes)).expect("keep");
    assert_eq!(restored, bytes);
}

#[test]
fn unrelated_receipt_change_is_not_the_bound_restore() {
    let original = drifted_journal(ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after"));
    let token = binding(&original, LIVE);
    let mut current = original.clone();
    current.resources[1].after = "D:(A;;GA;;;WD)".into();
    let error = restore(current, &original, &token, LIVE).expect_err("unrelated");
    assert!(error.to_string().contains("maintenance.migration_unrelated_receipt"), "{error}");
}

#[test]
fn duplicate_successor_is_not_the_same_restore() {
    let original = drifted_journal(ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after"));
    let token = binding(&original, LIVE);
    let first = restore(original.clone(), &original, &token, LIVE).expect("first");
    let mut duplicated = first.journal.clone();
    duplicated.resources.push(duplicated.resources[2].clone());
    let error = restore(duplicated, &original, &token, LIVE).expect_err("dup");
    assert!(error.to_string().contains("maintenance.migration_duplicate_successor"), "{error}");
}

#[test]
fn stop_pending_is_not_a_stopped_service() {
    const SERVICE_STOPPED: u32 = 1;
    const SERVICE_START_PENDING: u32 = 2;
    const SERVICE_STOP_PENDING: u32 = 3;
    const SERVICE_RUNNING: u32 = 4;
    assert!(migration_service_quiesced(SERVICE_STOP_PENDING, 0, Ok(false)).is_err());
    assert!(migration_service_quiesced(SERVICE_START_PENDING, 0, Ok(false)).is_err());
    assert!(migration_service_quiesced(SERVICE_RUNNING, 0, Ok(false)).is_err());
    assert!(migration_service_quiesced(SERVICE_STOPPED, 0, Ok(false)).is_ok());
    assert!(migration_service_quiesced(SERVICE_STOPPED, 4228, Ok(true)).is_err());
    assert!(migration_service_quiesced(SERVICE_STOPPED, 4228, Ok(false)).is_ok());
}

#[test]
fn query_refusal_is_not_treated_as_a_missing_process() {
    let denied = std::io::Error::from_raw_os_error(5);
    assert!(!process_query_confirms_absence(&denied));
    let error = migration_service_quiesced(1, 4228, Err(denied)).expect_err("denied");
    assert!(error.to_string().contains("maintenance.migration_service_process_query"), "{error}");
    let identity = std::io::Error::other("process identity changed");
    assert!(!process_query_confirms_absence(&identity));
    assert!(migration_service_quiesced(1, 4228, Err(identity)).is_err());
    let missing = std::io::Error::from_raw_os_error(87);
    assert!(process_query_confirms_absence(&missing));
}

#[test]
fn concurrent_migration_entries_are_exclusive() {
    let directory = tempfile::tempdir().expect("temp");
    let path = directory.path().join("migration.lock");
    std::fs::write(&path, b"execution-maintenance-migration-lock-v1").expect("lock file");
    let first = lock_migration_file(&path).expect("first");
    let contested = path.clone();
    let second = std::thread::spawn(move || lock_migration_file(&contested)).join().expect("join");
    match second {
        Err(error) => {
            assert!(error.to_string().contains("maintenance.migration_in_use"), "{error}")
        }
        Ok(_) => panic!("second migration entry must be exclusive"),
    }
    drop(first);
    let _released = lock_migration_file(&path).expect("released");
}

#[test]
fn journal_stage_promotes_ordinary_refresh_and_bound_intent() {
    let after = ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after");
    let committed = drifted_journal(after);
    let c = config();
    let token = binding(&committed, LIVE);
    let intended = restore(committed.clone(), &committed, &token, LIVE).expect("intent");
    assert!(staged_continues_committed(&committed, &intended.journal, &c));
    assert_eq!(
        decide_journal_stage(Some(Ok(committed.clone())), Some(Ok(intended.journal.clone())), &c),
        JournalStageDecision::PromoteStaged
    );
    let mut applied = intended.journal.clone();
    applied.resources[2].state = MaintenanceResourceState::Applied;
    assert!(staged_continues_committed(&intended.journal, &applied, &c));
    let mut cleared = committed.clone();
    cleared.error = None;
    assert_eq!(
        decide_journal_stage(Some(Ok(committed.clone())), Some(Ok(cleared.clone())), &c),
        JournalStageDecision::PromoteStaged
    );
    let promoted = load_journal_bytes(
        Some(&serde_json::to_vec(&committed).expect("c")),
        Some(&serde_json::to_vec(&cleared).expect("s")),
        &c,
        &committed.config_sha256,
    )
    .expect("refresh");
    assert!(promoted.1);
    assert_eq!(promoted.0.error, None);
    let mut rewritten = committed.clone();
    rewritten.resources[1].after = "D:(A;;GA;;;WD)".into();
    assert_eq!(
        decide_journal_stage(Some(Ok(committed.clone())), Some(Ok(rewritten)), &c),
        JournalStageDecision::Conflict
    );
    let error = load_journal_bytes(
        Some(b"{"),
        Some(&serde_json::to_vec(&intended.journal).expect("s")),
        &c,
        &committed.config_sha256,
    )
    .expect_err("corrupt");
    assert!(error.to_string().contains("maintenance.journal_corrupt"), "{error}");
}

#[test]
fn invalid_staged_journal_does_not_replace_committed() {
    let after = ExecutionDevice::Null.plan(ID, BEFORE, true).expect("after");
    let committed = drifted_journal(after);
    let c = config();
    let committed_bytes = serde_json::to_vec(&committed).expect("committed");
    let mut extra = committed.clone();
    extra.resources.push(MaintenanceResourceReceipt {
        boot_id: BOOT.into(),
        target: MaintenanceTarget::Null,
        parent_dacl: None,
        before: BEFORE.into(),
        after: "D:(A;;GA;;;WD)".into(),
        state: MaintenanceResourceState::Pending,
        object_generation: Some(GEN.into()),
    });
    assert_eq!(
        decide_journal_stage(Some(Ok(committed.clone())), Some(Ok(extra.clone())), &c),
        JournalStageDecision::Conflict
    );
    let extra_error = load_journal_bytes(
        Some(&committed_bytes),
        Some(&serde_json::to_vec(&extra).expect("extra")),
        &c,
        &committed.config_sha256,
    )
    .expect_err("invalid extra");
    assert!(
        extra_error.to_string().contains("maintenance.journal_stage_conflict"),
        "{extra_error}"
    );
    let mut rolled = committed.clone();
    rolled.resources[0].state = MaintenanceResourceState::Pending;
    assert_eq!(
        decide_journal_stage(Some(Ok(committed.clone())), Some(Ok(rolled.clone())), &c),
        JournalStageDecision::Conflict
    );
    let roll_error = load_journal_bytes(
        Some(&committed_bytes),
        Some(&serde_json::to_vec(&rolled).expect("roll")),
        &c,
        &committed.config_sha256,
    )
    .expect_err("rollback");
    assert!(roll_error.to_string().contains("maintenance.journal_stage_conflict"), "{roll_error}");
    let mut wrong = committed.clone();
    wrong.error = None;
    let config_error = load_journal_bytes(
        Some(&committed_bytes),
        Some(&serde_json::to_vec(&wrong).expect("wrong")),
        &c,
        &ContentHash::sha256(b"other-config"),
    )
    .expect_err("config");
    assert!(
        config_error.to_string().contains("maintenance.journal_config_changed"),
        "{config_error}"
    );
}
