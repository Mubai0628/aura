use super::*;
use aura_domain_common::{
    ContentHash, ToolWireVersionV1, UtcTimestamp, jcs::CanonicalSha256HashV1,
};
use std::collections::HashSet;

fn report() -> ExecutionHostAcceptanceReport {
    ExecutionHostAcceptanceReport {
        schema_version: ToolWireVersionV1,
        acceptance_version: EXECUTION_HOST_ACCEPTANCE_VERSION.into(),
        assurance_version: ASSURANCE_VERSION.into(),
        edition: ExecutionHostEdition::Home,
        windows_build: "26200".into(),
        observed_at: UtcTimestamp::now(),
        candidate_inventory_sha256: ContentHash::sha256(b"candidate-inventory"),
        source_fingerprint: CanonicalSha256HashV1::sha256(b"fixture-source"),
        installation_fingerprint: CanonicalSha256HashV1::sha256(b"fixture-install"),
        core_sha256: ContentHash::sha256(b"core"),
        runner_sha256: ContentHash::sha256(b"runner"),
        helper_sha256: ContentHash::sha256(b"helper"),
        maintainer_sha256: ContentHash::sha256(b"maintainer"),
        powershell_manifest_sha256: ContentHash::sha256(b"pin"),
        cmd_image: ExecutionFileBinding {
            path: "C:/fixture/cmd.exe".into(),
            volume_serial: "1".into(),
            file_id: [1; 16],
            sha256: Some(ContentHash::sha256(b"cmd")),
        },
        observations: vec![],
    }
}

fn expected_observation(case: ExecutionAcceptanceCase) -> ExecutionAcceptanceObservation {
    ExecutionAcceptanceObservation {
        case,
        result: if case == ExecutionAcceptanceCase::DnscacheKnownLimitation {
            ExecutionCheckResult::AcceptedKnownLimitation
        } else {
            ExecutionCheckResult::Passed
        },
        evidence_path: "fixture.json".into(),
        evidence_sha256: ContentHash::sha256(b"unit fixture"),
        detail: "unit aggregation only".into(),
    }
}

#[test]
fn required_case_sets_are_nonempty_and_disjoint() {
    let mut seen = HashSet::new();
    for stage in ExecutionAcceptanceStage::ALL {
        assert!(!stage.required_cases().is_empty(), "{stage:?}");
        for case in stage.required_cases() {
            assert!(seen.insert(*case), "{case:?} is required by multiple stages");
        }
    }
    assert_eq!(ExecutionAcceptanceStage::R06.required_cases(), ExecutionAcceptanceCase::R06);
    assert!(
        ExecutionAcceptanceCase::R09
            .contains(&ExecutionAcceptanceCase::ElectronMainCoreProductionChain)
    );
    assert!(
        !ExecutionAcceptanceCase::R06.contains(&ExecutionAcceptanceCase::CoreRouteBrokerToolLoop)
    );
}

#[test]
fn stages_are_assessed_independently() {
    let mut report = report();
    report.observations =
        ExecutionAcceptanceCase::R06.iter().copied().map(expected_observation).collect();

    assert!(report.is_complete_for(ExecutionAcceptanceStage::R06));
    for stage in [
        ExecutionAcceptanceStage::R07,
        ExecutionAcceptanceStage::R08,
        ExecutionAcceptanceStage::R09,
    ] {
        assert_eq!(report.missing_or_failed_for(stage), stage.required_cases());
    }

    report
        .observations
        .extend(ExecutionAcceptanceCase::R07.iter().copied().map(expected_observation));
    let failed = ExecutionAcceptanceCase::OnlineSyntheticUpload;
    report
        .observations
        .iter_mut()
        .find(|item| item.case == failed)
        .expect("online upload observation")
        .result = ExecutionCheckResult::Failed;
    assert_eq!(report.missing_or_failed_for(ExecutionAcceptanceStage::R07), vec![failed]);
    assert!(report.is_complete_for(ExecutionAcceptanceStage::R06));
}

#[test]
fn duplicate_observation_is_one_exact_unresolved_case() {
    let mut report = report();
    report.observations =
        ExecutionAcceptanceCase::R08.iter().copied().map(expected_observation).collect();
    let duplicate = ExecutionAcceptanceCase::StableRegistrySnapshot;
    report.observations.push(expected_observation(duplicate));
    assert_eq!(report.missing_or_failed_for(ExecutionAcceptanceStage::R08), vec![duplicate]);
}

#[test]
fn only_the_named_dns_case_accepts_the_known_limitation_result() {
    let mut report = report();
    report.observations =
        ExecutionAcceptanceCase::R06.iter().copied().map(expected_observation).collect();
    assert!(report.is_complete_for(ExecutionAcceptanceStage::R06));

    let invalid = ExecutionAcceptanceCase::SourceBuildInstallBinding;
    report
        .observations
        .iter_mut()
        .find(|item| item.case == invalid)
        .expect("source binding observation")
        .result = ExecutionCheckResult::AcceptedKnownLimitation;
    assert_eq!(report.missing_or_failed_for(ExecutionAcceptanceStage::R06), vec![invalid]);

    report
        .observations
        .iter_mut()
        .find(|item| item.case == invalid)
        .expect("source binding observation")
        .result = ExecutionCheckResult::Passed;
    report
        .observations
        .iter_mut()
        .find(|item| item.case == ExecutionAcceptanceCase::DnscacheKnownLimitation)
        .expect("dnscache observation")
        .result = ExecutionCheckResult::Passed;
    assert_eq!(
        report.missing_or_failed_for(ExecutionAcceptanceStage::R06),
        vec![ExecutionAcceptanceCase::DnscacheKnownLimitation]
    );
}
