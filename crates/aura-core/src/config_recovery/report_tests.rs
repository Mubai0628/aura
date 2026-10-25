use std::sync::Arc;

use super::*;

struct ReportRebuilder {
    result: Result<CapabilityRebuildOutcome, CapabilityRebuildError>,
}

impl DependentServiceRebuilder for ReportRebuilder {
    fn rebuild_missing(
        &self,
        _: &[ConfigModule],
    ) -> Result<CapabilityRebuildOutcome, CapabilityRebuildError> {
        self.result.clone()
    }

    fn is_available(&self, capability: ConfigDependentCapability) -> bool {
        self.result.as_ref().is_ok_and(|result| result.available.contains(&capability))
    }
}

fn entry(
    degradation: Arc<ConfigDegradationRegistry>,
    rebuilder: Option<ReportRebuilder>,
) -> (tempfile::TempDir, ConfigRecoveryEntry) {
    let directory = tempfile::tempdir().expect("isolated configuration root");
    let paths = aura_config::AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    );
    let settings = SettingsFacade::open(paths).expect("validated first-use configuration");
    let entry = ConfigRecoveryEntry::new(settings, degradation);
    let entry = match rebuilder {
        Some(rebuilder) => entry.with_rebuilder(Arc::new(rebuilder)),
        None => entry,
    };
    (directory, entry)
}

fn all_services_present() -> ReportRebuilder {
    ReportRebuilder {
        result: Ok(CapabilityRebuildOutcome {
            available: vec![
                ConfigDependentCapability::ToolPolicy,
                ConfigDependentCapability::SecurityAuthority,
                ConfigDependentCapability::ModelAuthority,
            ],
            still_missing: Vec::new(),
        }),
    }
}

#[test]
fn a_healthy_transaction_judgement_is_not_reported_as_a_missing_service() {
    let (_directory, entry) =
        entry(Arc::new(ConfigDegradationRegistry::default()), Some(all_services_present()));
    let (restored, unavailable) = entry.supply_and_report(&[ConfigModule::Ui]);
    assert!(unavailable.is_empty(), "a healthy judgement has no missing service: {unavailable:?}");
    assert_eq!(restored, vec![ConfigDependentCapability::ConfigTransactionRecovery.key()]);
}

#[test]
fn a_present_service_with_another_domain_fault_is_still_reported_unavailable() {
    let degradation = Arc::new(ConfigDegradationRegistry::default());
    degradation.degrade(
        ConfigDependentCapability::ToolPolicy,
        Some(ConfigModule::Aura),
        "config.domain.codec",
        true,
    );
    let (_directory, entry) = entry(degradation, Some(all_services_present()));
    let (restored, unavailable) = entry.supply_and_report(&[ConfigModule::Tools]);
    assert!(!restored.iter().any(|key| key == ConfigDependentCapability::ToolPolicy.key()));
    let remaining = unavailable
        .iter()
        .find(|entry| entry.capability == ConfigDependentCapability::ToolPolicy.key())
        .expect("repairing Tools must still explain the remaining Aura fault");
    assert!(remaining.reason.contains("config.domain.codec"), "{remaining:?}");
    assert!(remaining.reason.contains("aura"), "{remaining:?}");
}

#[test]
fn a_report_keeps_the_reason_returned_by_its_own_rebuild() {
    let mut rebuilder = all_services_present();
    let outcome = rebuilder.result.as_mut().expect("successful rebuild assessment");
    outcome.available.retain(|capability| *capability != ConfigDependentCapability::ToolPolicy);
    outcome.still_missing.push((ConfigDependentCapability::ToolPolicy, "this_attempt".into()));
    let (_directory, entry) =
        entry(Arc::new(ConfigDegradationRegistry::default()), Some(rebuilder));
    let (_, unavailable) = entry.supply_and_report(&[ConfigModule::Tools]);
    let remaining = unavailable
        .iter()
        .find(|entry| entry.capability == ConfigDependentCapability::ToolPolicy.key())
        .expect("the refused service is reported");
    assert_eq!(remaining.reason, "this_attempt");
}

#[test]
fn an_unknown_transaction_scope_is_reported_with_its_real_fault() {
    let degradation = Arc::new(ConfigDegradationRegistry::default());
    degradation.degrade(
        ConfigDependentCapability::ConfigTransactionRecovery,
        None,
        "core.config_recovery.unresolved",
        false,
    );
    let (_directory, entry) = entry(degradation, Some(all_services_present()));
    let (restored, unavailable) = entry.supply_and_report(&[ConfigModule::Ui]);
    assert!(restored.is_empty());
    assert_eq!(unavailable.len(), 1);
    assert!(unavailable[0].reason.contains("core.config_recovery.unresolved"));
}

#[test]
fn an_entry_without_a_runtime_does_not_fabricate_a_tool_service() {
    let (_directory, entry) = entry(Arc::new(ConfigDegradationRegistry::default()), None);
    let (restored, unavailable) = entry.supply_and_report(&[ConfigModule::Tools]);
    assert!(!restored.iter().any(|key| key == ConfigDependentCapability::ToolPolicy.key()));
    assert_eq!(unavailable.len(), 1);
    assert_eq!(unavailable[0].capability, ConfigDependentCapability::ToolPolicy.key());
}

#[test]
fn a_rebuild_failure_keeps_its_reason_without_breaking_a_healthy_judgement() {
    let rebuilder = ReportRebuilder {
        result: Err(CapabilityRebuildError { reason: "construction_unavailable".into() }),
    };
    let (_directory, entry) =
        entry(Arc::new(ConfigDegradationRegistry::default()), Some(rebuilder));
    let (restored, unavailable) = entry.supply_and_report(&[ConfigModule::Models]);
    assert_eq!(restored, vec![ConfigDependentCapability::ConfigTransactionRecovery.key()]);
    assert_eq!(unavailable.len(), 1);
    assert_eq!(unavailable[0].reason, "construction_unavailable");
}
