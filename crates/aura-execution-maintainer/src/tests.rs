use crate::{engine::apply_device_observation, policy};
use aura_domain_common::{ContentHash, UtcTimestamp};
use aura_windows_user_contract::*;
#[test]
fn resource_creation_race_retries_but_identity_or_dacl_drift_never_does() {
    for code in [2, 3, 21, 1168] {
        let result = Err(anyhow::Error::new(std::io::Error::from_raw_os_error(code))
            .context("fixed owner namespace"));
        assert_eq!(crate::transient_delay(&result, 0), Some(std::time::Duration::from_secs(1)));
        assert_eq!(crate::transient_delay(&result, 100), Some(std::time::Duration::from_secs(30)));
    }
    for result in [
        Ok(()),
        Err(anyhow::anyhow!("maintenance.parent_drift")),
        Err(anyhow::anyhow!("maintenance.journal_config_changed")),
        Err(anyhow::Error::new(std::io::Error::from_raw_os_error(5))),
    ] {
        assert_eq!(crate::transient_delay(&result, 0), None);
    }
}
fn config() -> MaintenanceConfig {
    MaintenanceConfig {
        schema: MAINTENANCE_VERSION.into(),
        installation_id: "073773f9-1798-4a46-a6d1-b8d5c9e3d19a".into(),
        owner_sid: "S-1-5-21-1-2-3-1001".into(),
        offline_sid: "S-1-5-21-1-2-3-1002".into(),
        online_sid: Some("S-1-5-21-1-2-3-1003".into()),
        initial_resources: vec![],
    }
}
#[test]
fn config_cannot_express_commands_paths_masks_or_broad_principals() {
    let original = config();
    policy::validate(&original).expect("config");
    for field in ["command", "path", "mask", "dns", "service_endpoint"] {
        let mut value = serde_json::to_value(&original).expect("json");
        value[field] = "untrusted".into();
        assert!(serde_json::from_value::<MaintenanceConfig>(value).is_err());
    }
    for sid in ["S-1-1-0", "S-1-5-18", "S-1-5-32-544", "S-1-5-21-1-2-3-500", "S-1-5-21-1-2-3-1001"]
    {
        let mut changed = original.clone();
        changed.offline_sid = sid.into();
        assert!(policy::validate(&changed).is_err());
    }
}
#[test]
fn interruption_can_resume_only_exact_recorded_states() {
    let c = config();
    for target in [
        MaintenanceTarget::Null,
        MaintenanceTarget::Ksec,
        MaintenanceTarget::OwnerNamespace { session_id: 1, logon_id: "0000000000123456".into() },
    ] {
        let before = "D:P(A;;GA;;;SY)".to_string();
        let after = policy::planned(&c, &target, &before).expect("plan");
        let record = MaintenanceResourceReceipt {
            boot_id: c.installation_id.clone(),
            parent_dacl: matches!(target, MaintenanceTarget::OwnerNamespace { .. })
                .then(|| "D:(A;;GA;;;SY)".into()),
            target,
            before,
            after,
            state: MaintenanceResourceState::Pending,
            object_generation: None,
        };
        policy::validate_receipt(&c, &record).expect("receipt");
        assert_eq!(policy::desired(&record, &record.before, false).expect("before"), record.after);
        assert_eq!(policy::desired(&record, &record.after, false).expect("after"), record.after);
        assert_eq!(policy::desired(&record, &record.after, true).expect("remove"), record.before);
        assert!(policy::desired(&record, &format!("{}(A;;GR;;;WD)", record.after), false).is_err());
        let mut forged = record.clone();
        forged.after.push_str("(A;;GA;;;WD)");
        assert!(policy::validate_receipt(&c, &forged).is_err());
    }
}

#[test]
fn relogon_reuses_only_a_previous_owned_exact_state_in_the_same_session_and_boot() {
    let c = config();
    let old =
        MaintenanceTarget::OwnerNamespace { session_id: 1, logon_id: "0000000000000001".into() };
    let current =
        MaintenanceTarget::OwnerNamespace { session_id: 1, logon_id: "0000000000000002".into() };
    let before = "D:(A;;GA;;;SY)".to_string();
    let after = policy::planned(&c, &old, &before).expect("plan");
    let record = MaintenanceResourceReceipt {
        boot_id: c.installation_id.clone(),
        target: old,
        parent_dacl: Some("old parent".into()),
        before,
        after: after.clone(),
        state: MaintenanceResourceState::Applied,
        object_generation: None,
    };
    assert_eq!(
        policy::prior_namespace(
            std::slice::from_ref(&record),
            &c.installation_id,
            &current,
            &after
        )
        .expect("owned"),
        Some(0)
    );
    for (boot, target, observed) in [
        ("other-boot", current.clone(), after.clone()),
        (
            &*c.installation_id,
            MaintenanceTarget::OwnerNamespace {
                session_id: 2,
                logon_id: "0000000000000002".into(),
            },
            after.clone(),
        ),
        (&*c.installation_id, current.clone(), format!("{after}(A;;GA;;;WD)")),
    ] {
        assert!(
            policy::prior_namespace(std::slice::from_ref(&record), boot, &target, &observed)
                .expect("not owned")
                .is_none()
        );
    }
    assert!(
        policy::prior_namespace(&[record.clone(), record], &c.installation_id, &current, &after)
            .is_err()
    );
}

#[test]
fn device_generation_adopts_a_new_os_baseline_without_the_capability_ace() {
    use aura_windows_platform::ExecutionDevice;
    let id = "4b486eb4-dd7f-4681-b8ac-7eece6ce8d25";
    let before = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)";
    let after = ExecutionDevice::Null.plan(id, before, true).expect("plan");
    let live = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)(A;;0x1201bf;;;AC)(A;;0x1201bf;;;S-1-15-2-2)";
    let same = "object:00000000000000a1";
    let restarted = "object:00000000000000a2";
    let receipt = MaintenanceResourceReceipt {
        boot_id: "6b675559-aa09-11f1-8d48-00155d006a00".into(),
        target: MaintenanceTarget::Null,
        parent_dacl: None,
        before: before.into(),
        after: after.clone(),
        state: MaintenanceResourceState::Applied,
        object_generation: Some(same.into()),
    };
    assert_eq!(
        policy::device_generation(&receipt, before, Some(restarted), id, ExecutionDevice::Null)
            .expect("before"),
        policy::DeviceGeneration::Current
    );
    assert_eq!(
        policy::device_generation(&receipt, &after, Some(same), id, ExecutionDevice::Null)
            .expect("after"),
        policy::DeviceGeneration::Current
    );
    assert_eq!(
        policy::device_generation(&receipt, live, Some(restarted), id, ExecutionDevice::Null)
            .expect("serviced restart"),
        policy::DeviceGeneration::NewOsBaseline
    );
    assert_eq!(
        policy::device_generation(&receipt, live, Some(same), id, ExecutionDevice::Null)
            .expect("same-boot extra aces"),
        policy::DeviceGeneration::UnrelatedDrift
    );
    let mut missing = receipt.clone();
    missing.object_generation = None;
    assert_eq!(
        policy::device_generation(&missing, live, Some(restarted), id, ExecutionDevice::Null)
            .expect("missing generation"),
        policy::DeviceGeneration::UnrelatedDrift
    );
    assert_eq!(
        policy::device_generation(&receipt, live, None, id, ExecutionDevice::Null)
            .expect("capture failed"),
        policy::DeviceGeneration::UnrelatedDrift
    );
    let drifted = format!("{after}(A;;GR;;;WD)");
    assert_eq!(
        policy::device_generation(&receipt, &drifted, Some(restarted), id, ExecutionDevice::Null)
            .expect("extra"),
        policy::DeviceGeneration::UnrelatedDrift
    );
    let capability = ExecutionDevice::Null.sid(id).expect("sid");
    let extra_sid = format!("{after}(A;;GR;;;{capability})");
    assert_eq!(
        policy::device_generation(&receipt, &extra_sid, Some(restarted), id, ExecutionDevice::Null)
            .expect("sid"),
        policy::DeviceGeneration::UnrelatedDrift
    );
    let ksec_before = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)(A;;0x1201bf;;;AC)(A;;0x1201bf;;;S-1-15-2-2)";
    let ksec_after = ExecutionDevice::Ksec.plan(id, ksec_before, true).expect("ksec plan");
    let ksec = MaintenanceResourceReceipt {
        boot_id: receipt.boot_id.clone(),
        target: MaintenanceTarget::Ksec,
        parent_dacl: None,
        before: ksec_before.into(),
        after: ksec_after,
        state: MaintenanceResourceState::Applied,
        object_generation: Some(same.into()),
    };
    assert_eq!(
        policy::device_generation(&ksec, ksec_before, Some(same), id, ExecutionDevice::Ksec)
            .expect("ksec service restart"),
        policy::DeviceGeneration::Current
    );
}

#[test]
fn current_generation_advance_is_saved_and_same_mapping_drift_stays_closed() {
    use aura_windows_platform::ExecutionDevice;
    let c = config();
    let id = c.installation_id.clone();
    let before = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)";
    let after = ExecutionDevice::Null.plan(&id, before, true).expect("plan");
    let live = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)(A;;0x1201bf;;;AC)(A;;0x1201bf;;;S-1-15-2-2)";
    let gen_a = "object:00000000000000a1".to_string();
    let gen_b = "object:00000000000000a2".to_string();
    let boot = "6b675559-aa09-11f1-8d48-00155d006a00";
    let receipt = MaintenanceResourceReceipt {
        boot_id: boot.into(),
        target: MaintenanceTarget::Null,
        parent_dacl: None,
        before: before.into(),
        after: after.clone(),
        state: MaintenanceResourceState::Applied,
        object_generation: Some(gen_a.clone()),
    };
    let mut journal = MaintenanceJournal {
        schema: MAINTENANCE_VERSION.into(),
        config_sha256: ContentHash::sha256(b"cfg"),
        resources: vec![receipt.clone()],
        observed_at: UtcTimestamp::now(),
        error: None,
    };
    let index = apply_device_observation(
        &mut journal,
        &c,
        boot,
        MaintenanceTarget::Null,
        None,
        &after,
        Some(gen_b.clone()),
    )
    .expect("adopt mapping B while DACL is still after");
    assert_eq!(index, 1);
    assert_eq!(journal.resources[0].state, MaintenanceResourceState::Retired);
    assert_eq!(journal.resources[0].object_generation.as_deref(), Some(gen_a.as_str()));
    assert_eq!(journal.resources[1].object_generation.as_deref(), Some(gen_b.as_str()));
    assert_eq!(journal.resources[1].before, receipt.before);
    assert_eq!(journal.resources[1].after, receipt.after);
    journal.resources[1].state = MaintenanceResourceState::Applied;
    let bytes = serde_json::to_vec(&journal).expect("save");
    let mut reloaded: MaintenanceJournal = serde_json::from_slice(&bytes).expect("reload");
    let error = apply_device_observation(
        &mut reloaded,
        &c,
        boot,
        MaintenanceTarget::Null,
        None,
        live,
        Some(gen_b.clone()),
    )
    .expect_err("same mapping B with extra ACEs");
    assert!(error.to_string().contains("maintenance.acl_drift"), "{error}");
    assert_eq!(
        policy::device_generation(&receipt, live, Some(&gen_b), &id, ExecutionDevice::Null)
            .expect("stale A"),
        policy::DeviceGeneration::NewOsBaseline
    );
    let mut missing = journal.clone();
    missing.resources =
        vec![MaintenanceResourceReceipt { object_generation: None, ..receipt.clone() }];
    let filled = apply_device_observation(
        &mut missing,
        &c,
        boot,
        MaintenanceTarget::Null,
        None,
        &after,
        Some(gen_b.clone()),
    )
    .expect("first capture");
    assert_eq!(filled, 0);
    assert_eq!(missing.resources.len(), 1);
    assert_eq!(missing.resources[0].state, MaintenanceResourceState::Applied);
    assert_eq!(missing.resources[0].object_generation.as_deref(), Some(gen_b.as_str()));
    let unchanged = apply_device_observation(
        &mut reloaded,
        &c,
        boot,
        MaintenanceTarget::Null,
        None,
        &after,
        Some(gen_b),
    )
    .expect("same generation");
    assert_eq!(unchanged, 1);
    assert_eq!(
        reloaded.resources.iter().filter(|r| r.state != MaintenanceResourceState::Retired).count(),
        1
    );
}
