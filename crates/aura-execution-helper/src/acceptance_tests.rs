use crate::acceptance::aggregate;
use aura_domain_common::{
    ContentHash, ToolWireVersionV1, UtcTimestamp, jcs::CanonicalSha256HashV1,
};
use aura_windows_user_contract::{
    ASSURANCE_VERSION, EXECUTION_HOST_ACCEPTANCE_VERSION, ExecutionAcceptanceCase,
    ExecutionAcceptanceObservation, ExecutionAcceptanceStage, ExecutionCheckResult,
    ExecutionFileBinding, ExecutionHostAcceptanceReport, ExecutionHostEdition,
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct PackageFacts {
    source: CanonicalSha256HashV1,
    core: ContentHash,
    runner: ContentHash,
    helper: ContentHash,
    maintainer: ContentHash,
    powershell_manifest: ContentHash,
}

struct AcceptanceFixture {
    root: PathBuf,
    package: PathBuf,
    home: PathBuf,
    pro: PathBuf,
    inventory: ContentHash,
}

impl AcceptanceFixture {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let timestamp =
            SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock").as_nanos();
        let root = std::env::temp_dir()
            .join(format!("aura-acceptance-{}-{timestamp}-{sequence}", std::process::id()));
        fs::create_dir(&root).expect("create fixture root");

        let inventory = ContentHash::sha256(b"bound bundle inventory");
        let facts = PackageFacts {
            source: CanonicalSha256HashV1::sha256(b"source"),
            core: ContentHash::sha256(b"core"),
            runner: ContentHash::sha256(b"runner"),
            helper: ContentHash::sha256(b"helper"),
            maintainer: ContentHash::sha256(b"maintainer"),
            powershell_manifest: ContentHash::sha256(b"powershell manifest"),
        };
        let package = root.join("package.json");
        write_json(
            &package,
            &json!({
                "schema": "windows-user-test-package-v2",
                "assurance_version": ASSURANCE_VERSION,
                "source_fingerprint": &facts.source,
                "core_sha256": &facts.core,
                "runner_sha256": &facts.runner,
                "helper_sha256": &facts.helper,
                "maintainer_sha256": &facts.maintainer,
                "powershell_manifest_sha256": &facts.powershell_manifest,
                "build_target": "x86_64-pc-windows-msvc",
                "rustflags": "-C target-feature=+crt-static",
                "host_acceptance": "Pending"
            }),
        );
        let home = write_report(&root.join("home"), ExecutionHostEdition::Home, &inventory, &facts);
        let pro = write_report(&root.join("pro"), ExecutionHostEdition::Pro, &inventory, &facts);
        Self { root, package, home, pro, inventory }
    }

    fn expected_inventory_argument(&self) -> String {
        self.inventory.hex.to_ascii_uppercase()
    }

    fn aggregate(&self) -> anyhow::Result<serde_json::Value> {
        aggregate(&self.expected_inventory_argument(), &self.package, &self.home, &self.pro)
    }
}

impl Drop for AcceptanceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn aggregate_reports_each_stage_without_single_stage_extrapolation() {
    let fixture = AcceptanceFixture::new();
    let complete = fixture.aggregate().expect("complete aggregate");
    for stage in ["r06", "r07", "r08", "r09"] {
        assert_eq!(complete["stages"][stage]["complete"], true, "{stage}");
        assert_eq!(complete["stages"][stage]["missing_or_failed"]["home"], json!([]));
        assert_eq!(complete["stages"][stage]["missing_or_failed"]["pro"], json!([]));
    }
    assert_eq!(complete["p04"], "Pause / PendingHostPreparation / Inconclusive");
    assert_eq!(complete["r10"], "excluded");
    assert!(complete.get("complete").is_none());

    let mut home = read_report(&fixture.home);
    home.observations
        .iter_mut()
        .find(|item| item.case == ExecutionAcceptanceCase::BackgroundCrashRecovery)
        .expect("R09 case")
        .result = ExecutionCheckResult::Failed;
    write_json(&fixture.home, &home);

    let partial = fixture.aggregate().expect("partial aggregate");
    for stage in ["r06", "r07", "r08"] {
        assert_eq!(partial["stages"][stage]["complete"], true, "{stage}");
    }
    assert_eq!(partial["stages"]["r09"]["complete"], false);
    assert_eq!(
        partial["stages"]["r09"]["missing_or_failed"]["home"],
        json!(["background_crash_recovery"])
    );
    assert_eq!(partial["stages"]["r09"]["missing_or_failed"]["pro"], json!([]));
}

#[test]
fn aggregate_rejects_inventory_mismatch() {
    let fixture = AcceptanceFixture::new();
    let wrong = ContentHash::sha256(b"a different candidate");
    let error = aggregate(&wrong.hex, &fixture.package, &fixture.home, &fixture.pro)
        .expect_err("CLI inventory mismatch must fail");
    assert!(error.to_string().contains("acceptance.candidate_inventory_mismatch"));

    let mut pro = read_report(&fixture.pro);
    pro.candidate_inventory_sha256 = wrong;
    write_json(&fixture.pro, &pro);
    let error = fixture.aggregate().expect_err("Home/Pro inventory mismatch must fail");
    assert!(error.to_string().contains("acceptance.candidate_inventory_mismatch"));
}

#[test]
fn aggregate_lineage_allows_controller_overlay_with_the_same_product() {
    let fixture = AcceptanceFixture::new();
    let home_inventory = ContentHash::sha256(b"home overlay inventory");
    let mut home = read_report(&fixture.home);
    home.candidate_inventory_sha256 = home_inventory.clone();
    write_json(&fixture.home, &home);
    let lineage = fixture.root.join("lineage.json");
    write_json(
        &lineage,
        &json!({
            "schema": "windows-user-r06-r09-acceptance-lineage-v1",
            "home": {
                "candidate_inventory_sha256": home_inventory,
                "source_fingerprint": read_report(&fixture.home).source_fingerprint,
                "core_sha256": read_report(&fixture.home).core_sha256,
                "runner_sha256": read_report(&fixture.home).runner_sha256,
                "helper_sha256": read_report(&fixture.home).helper_sha256,
                "maintainer_sha256": read_report(&fixture.home).maintainer_sha256,
                "powershell_manifest_sha256": read_report(&fixture.home).powershell_manifest_sha256
            },
            "pro": {
                "candidate_inventory_sha256": fixture.inventory,
                "source_fingerprint": read_report(&fixture.pro).source_fingerprint,
                "core_sha256": read_report(&fixture.pro).core_sha256,
                "runner_sha256": read_report(&fixture.pro).runner_sha256,
                "helper_sha256": read_report(&fixture.pro).helper_sha256,
                "maintainer_sha256": read_report(&fixture.pro).maintainer_sha256,
                "powershell_manifest_sha256": read_report(&fixture.pro).powershell_manifest_sha256
            },
            "home_reuse": {
                "same_product": true,
                "unchanged_capabilities": ["system_reboot_recovery"],
                "retest_required": []
            }
        }),
    );
    let value = crate::acceptance::aggregate_with_lineage(
        &fixture.expected_inventory_argument(),
        &fixture.package,
        &fixture.home,
        &fixture.pro,
        Some(&lineage),
    )
    .expect("controller overlay lineage");
    assert_eq!(value["product_match"], true);
    assert_eq!(value["stages"]["r06"]["complete"], true);
}

#[test]
fn aggregate_lineage_refuses_to_complete_when_core_hashes_differ() {
    let fixture = AcceptanceFixture::new();
    let old_core = ContentHash::sha256(b"previous core");
    let mut home = read_report(&fixture.home);
    home.core_sha256 = old_core.clone();
    write_json(&fixture.home, &home);
    let lineage = fixture.root.join("lineage.json");
    write_json(
        &lineage,
        &json!({
            "schema": "windows-user-r06-r09-acceptance-lineage-v1",
            "home": {
                "candidate_inventory_sha256": fixture.inventory,
                "source_fingerprint": read_report(&fixture.home).source_fingerprint,
                "core_sha256": old_core,
                "runner_sha256": read_report(&fixture.home).runner_sha256,
                "helper_sha256": read_report(&fixture.home).helper_sha256,
                "maintainer_sha256": read_report(&fixture.home).maintainer_sha256,
                "powershell_manifest_sha256": read_report(&fixture.home).powershell_manifest_sha256
            },
            "pro": {
                "candidate_inventory_sha256": fixture.inventory,
                "source_fingerprint": read_report(&fixture.pro).source_fingerprint,
                "core_sha256": read_report(&fixture.pro).core_sha256,
                "runner_sha256": read_report(&fixture.pro).runner_sha256,
                "helper_sha256": read_report(&fixture.pro).helper_sha256,
                "maintainer_sha256": read_report(&fixture.pro).maintainer_sha256,
                "powershell_manifest_sha256": read_report(&fixture.pro).powershell_manifest_sha256
            },
            "home_reuse": {
                "same_product": false,
                "unchanged_capabilities": ["managed_power_shell"],
                "retest_required": ["controlled_web_redirect"]
            }
        }),
    );
    let value = crate::acceptance::aggregate_with_lineage(
        &fixture.expected_inventory_argument(),
        &fixture.package,
        &fixture.home,
        &fixture.pro,
        Some(&lineage),
    )
    .expect("different-core lineage must still verify each host");
    assert_eq!(value["product_match"], false);
    assert_eq!(value["stages"]["r06"]["complete"], false);
    assert_eq!(value["stages"]["r07"]["complete"], false);
}

#[test]
fn aggregate_rejects_changed_evidence() {
    let fixture = AcceptanceFixture::new();
    let home = read_report(&fixture.home);
    let evidence =
        fixture.home.parent().expect("report parent").join(&home.observations[0].evidence_path);
    fs::write(evidence, b"changed after report").expect("change evidence");

    let error = fixture.aggregate().expect_err("changed evidence must fail");
    assert!(error.to_string().contains("acceptance.evidence_changed"));
}

#[test]
fn aggregate_rejects_duplicate_and_non_normal_evidence_paths() {
    let fixture = AcceptanceFixture::new();
    let mut home = read_report(&fixture.home);
    let duplicate_path = home.observations[0].evidence_path.clone();
    let duplicate_hash = home.observations[0].evidence_sha256.clone();
    home.observations[1].evidence_path = duplicate_path;
    home.observations[1].evidence_sha256 = duplicate_hash;
    write_json(&fixture.home, &home);
    let error = fixture.aggregate().expect_err("duplicate path must fail");
    assert!(error.to_string().contains("acceptance.evidence_path_duplicate"));

    let fixture = AcceptanceFixture::new();
    let mut home = read_report(&fixture.home);
    home.observations[0].evidence_path = "evidence/../escape.json".into();
    write_json(&fixture.home, &home);
    let error = fixture.aggregate().expect_err("non-normal path must fail");
    assert!(error.to_string().contains("acceptance.evidence_path"));
}

fn write_report(
    root: &Path,
    edition: ExecutionHostEdition,
    inventory: &ContentHash,
    facts: &PackageFacts,
) -> PathBuf {
    let evidence_root = root.join("evidence");
    fs::create_dir_all(&evidence_root).expect("create evidence root");
    let mut observations = Vec::new();
    let mut index = 0_u32;
    for stage in ExecutionAcceptanceStage::ALL {
        for case in stage.required_cases() {
            let case_name = serde_json::to_value(case)
                .expect("case value")
                .as_str()
                .expect("case string")
                .to_owned();
            let relative = format!("evidence/{index:03}-{case_name}.json");
            let bytes = format!("{stage:?}:{case:?}\n").into_bytes();
            fs::write(root.join(&relative), &bytes).expect("write evidence");
            observations.push(ExecutionAcceptanceObservation {
                case: *case,
                result: if *case == ExecutionAcceptanceCase::DnscacheKnownLimitation {
                    ExecutionCheckResult::AcceptedKnownLimitation
                } else {
                    ExecutionCheckResult::Passed
                },
                evidence_path: relative,
                evidence_sha256: ContentHash::sha256(&bytes),
                detail: "unit acceptance evidence".into(),
            });
            index += 1;
        }
    }
    let report = ExecutionHostAcceptanceReport {
        schema_version: ToolWireVersionV1,
        acceptance_version: EXECUTION_HOST_ACCEPTANCE_VERSION.into(),
        assurance_version: ASSURANCE_VERSION.into(),
        edition,
        windows_build: "26200".into(),
        observed_at: UtcTimestamp::now(),
        candidate_inventory_sha256: inventory.clone(),
        source_fingerprint: facts.source.clone(),
        installation_fingerprint: CanonicalSha256HashV1::sha256(
            format!("{edition:?} installation").as_bytes(),
        ),
        core_sha256: facts.core.clone(),
        runner_sha256: facts.runner.clone(),
        helper_sha256: facts.helper.clone(),
        maintainer_sha256: facts.maintainer.clone(),
        powershell_manifest_sha256: facts.powershell_manifest.clone(),
        cmd_image: ExecutionFileBinding {
            path: "C:/Windows/System32/cmd.exe".into(),
            volume_serial: "1".into(),
            file_id: [1; 16],
            sha256: Some(ContentHash::sha256(b"system cmd")),
        },
        observations,
    };
    let path = root.join("host-report.json");
    write_json(&path, &report);
    path
}

fn read_report(path: &Path) -> ExecutionHostAcceptanceReport {
    serde_json::from_slice(&fs::read(path).expect("read host report")).expect("parse host report")
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec_pretty(value).expect("serialize JSON")).expect("write JSON");
}
