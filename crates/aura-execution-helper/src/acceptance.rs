use anyhow::{Result, anyhow, ensure};
use aura_domain_common::{
    ContentHash, ContentHashAlgorithm,
    jcs::{CanonicalSha256HashV1, canonical_sha256, parse_json_strict},
};
use aura_windows_user_contract::{
    ASSURANCE_VERSION, EXECUTION_HOST_ACCEPTANCE_VERSION, ExecutionAcceptanceStage,
    ExecutionHostAcceptanceReport, ExecutionHostEdition,
};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    path::{Component, Path},
};

const MAX_PACKAGE_BYTES: u64 = 1024 * 1024;
const MAX_REPORT_BYTES: u64 = 1024 * 1024;
const MAX_EVIDENCE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Deserialize)]
struct AcceptancePackage {
    schema: String,
    assurance_version: String,
    source_fingerprint: CanonicalSha256HashV1,
    core_sha256: ContentHash,
    runner_sha256: ContentHash,
    helper_sha256: ContentHash,
    maintainer_sha256: ContentHash,
    powershell_manifest_sha256: ContentHash,
}

#[derive(Clone, Deserialize)]
struct AcceptanceLineageHost {
    candidate_inventory_sha256: ContentHash,
    source_fingerprint: CanonicalSha256HashV1,
    core_sha256: ContentHash,
    runner_sha256: ContentHash,
    helper_sha256: ContentHash,
    maintainer_sha256: ContentHash,
    powershell_manifest_sha256: ContentHash,
}

#[derive(Deserialize)]
struct AcceptanceLineageReuse {
    same_product: bool,
    #[serde(default)]
    unchanged_capabilities: Vec<String>,
    #[serde(default)]
    retest_required: Vec<String>,
}

#[derive(Deserialize)]
struct AcceptanceLineage {
    schema: String,
    home: AcceptanceLineageHost,
    pro: AcceptanceLineageHost,
    home_reuse: AcceptanceLineageReuse,
}

struct VerifiedHost {
    report: ExecutionHostAcceptanceReport,
    report_sha256: ContentHash,
}

pub fn aggregate(
    expected_inventory_sha256: &str,
    package_path: &Path,
    home_path: &Path,
    pro_path: &Path,
) -> Result<serde_json::Value> {
    aggregate_with_lineage(expected_inventory_sha256, package_path, home_path, pro_path, None)
}

pub fn aggregate_with_lineage(
    expected_inventory_sha256: &str,
    package_path: &Path,
    home_path: &Path,
    pro_path: &Path,
    lineage_path: Option<&Path>,
) -> Result<serde_json::Value> {
    let expected_inventory_sha256 = parse_inventory_sha256(expected_inventory_sha256)?;
    let package_bytes = read_bounded(
        package_path,
        MAX_PACKAGE_BYTES,
        "acceptance.package_not_file",
        "acceptance.package_limit",
    )?;
    let package_value =
        parse_json_strict(&package_bytes).map_err(|_| anyhow!("acceptance.package_json"))?;
    let package: AcceptancePackage = serde_json::from_value(package_value.clone())?;
    ensure!(
        package.schema == "windows-user-test-package-v2"
            && package.assurance_version == ASSURANCE_VERSION,
        "acceptance.package_version"
    );
    for hash in [
        &package.core_sha256,
        &package.runner_sha256,
        &package.helper_sha256,
        &package.maintainer_sha256,
        &package.powershell_manifest_sha256,
    ] {
        ensure!(
            hash.algorithm == ContentHashAlgorithm::Sha256,
            "acceptance.package_hash_algorithm"
        );
    }

    let lineage = lineage_path.map(read_lineage).transpose()?;
    let home = if let Some(lineage) = lineage.as_ref() {
        ensure!(
            lineage.pro.candidate_inventory_sha256 == expected_inventory_sha256
                && lineage.pro.core_sha256 == package.core_sha256
                && lineage.pro.runner_sha256 == package.runner_sha256
                && lineage.pro.helper_sha256 == package.helper_sha256
                && lineage.pro.maintainer_sha256 == package.maintainer_sha256
                && lineage.pro.powershell_manifest_sha256 == package.powershell_manifest_sha256
                && lineage.pro.source_fingerprint == package.source_fingerprint,
            "acceptance.lineage_pro_package_mismatch"
        );
        verify_host_declared(home_path, ExecutionHostEdition::Home, &lineage.home)?
    } else {
        verify_host(home_path, ExecutionHostEdition::Home, &expected_inventory_sha256, &package)?
    };
    let pro =
        verify_host(pro_path, ExecutionHostEdition::Pro, &expected_inventory_sha256, &package)?;
    let hosts = [home, pro];
    let product_match = hosts[0].report.core_sha256 == package.core_sha256
        && hosts[0].report.runner_sha256 == package.runner_sha256
        && hosts[0].report.helper_sha256 == package.helper_sha256
        && hosts[0].report.maintainer_sha256 == package.maintainer_sha256
        && hosts[0].report.powershell_manifest_sha256 == package.powershell_manifest_sha256
        && hosts[0].report.source_fingerprint == package.source_fingerprint
        && hosts[1].report.core_sha256 == package.core_sha256;
    if let Some(lineage) = lineage.as_ref() {
        ensure!(
            lineage.home_reuse.same_product == product_match,
            "acceptance.lineage_same_product_mismatch"
        );
    }

    let mut stages = serde_json::Map::new();
    for (key, stage) in [
        ("r06", ExecutionAcceptanceStage::R06),
        ("r07", ExecutionAcceptanceStage::R07),
        ("r08", ExecutionAcceptanceStage::R08),
        ("r09", ExecutionAcceptanceStage::R09),
    ] {
        let home_missing = hosts[0].report.missing_or_failed_for(stage);
        let pro_missing = hosts[1].report.missing_or_failed_for(stage);
        stages.insert(
            key.into(),
            serde_json::json!({
                "complete": product_match && home_missing.is_empty() && pro_missing.is_empty(),
                "missing_or_failed": {
                    "home": home_missing,
                    "pro": pro_missing,
                },
                "product_match": product_match,
            }),
        );
    }

    let host_summaries = hosts
        .iter()
        .map(|host| {
            serde_json::json!({
                "edition": host.report.edition,
                "windows_build": host.report.windows_build,
                "report_sha256": host.report_sha256,
                "installation_fingerprint": host.report.installation_fingerprint,
                "candidate_inventory_sha256": host.report.candidate_inventory_sha256,
                "core_sha256": host.report.core_sha256,
                "cmd_image": host.report.cmd_image,
            })
        })
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "schema": "windows-user-r06-r09-acceptance-v1",
        "assurance_version": ASSURANCE_VERSION,
        "candidate_inventory_sha256": expected_inventory_sha256,
        "package_fingerprint": canonical_sha256(&package_value)?,
        "hosts": host_summaries,
        "stages": stages,
        "product_match": product_match,
        "lineage": lineage.as_ref().map(|item| serde_json::json!({
            "path": lineage_path.map(|path| path.display().to_string()),
            "same_product": item.home_reuse.same_product,
            "unchanged_capabilities": item.home_reuse.unchanged_capabilities,
            "retest_required": item.home_reuse.retest_required,
            "home_inventory": item.home.candidate_inventory_sha256,
            "pro_inventory": item.pro.candidate_inventory_sha256,
        })),
        "p04": "Pause / PendingHostPreparation / Inconclusive",
        "r10": "excluded",
    }))
}

fn parse_inventory_sha256(value: &str) -> Result<ContentHash> {
    ensure!(
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "acceptance.expected_inventory_sha256"
    );
    ContentHash::new(ContentHashAlgorithm::Sha256, value.to_ascii_lowercase())
        .map_err(|_| anyhow!("acceptance.expected_inventory_sha256"))
}

fn verify_host(
    path: &Path,
    edition: ExecutionHostEdition,
    expected_inventory_sha256: &ContentHash,
    package: &AcceptancePackage,
) -> Result<VerifiedHost> {
    let bytes = read_bounded(
        path,
        MAX_REPORT_BYTES,
        "acceptance.report_not_file",
        "acceptance.report_limit",
    )?;
    let report_value = parse_json_strict(&bytes).map_err(|_| anyhow!("acceptance.report_json"))?;
    let report: ExecutionHostAcceptanceReport = serde_json::from_value(report_value)?;
    ensure!(
        report.edition == edition
            && !report.windows_build.trim().is_empty()
            && report.assurance_version == ASSURANCE_VERSION
            && report.acceptance_version == EXECUTION_HOST_ACCEPTANCE_VERSION,
        "acceptance.host_or_assurance_mismatch"
    );
    ensure!(
        report.candidate_inventory_sha256 == *expected_inventory_sha256,
        "acceptance.candidate_inventory_mismatch"
    );
    ensure!(report.source_fingerprint == package.source_fingerprint, "acceptance.source_mismatch");
    for (actual, expected) in [
        (&report.core_sha256, &package.core_sha256),
        (&report.runner_sha256, &package.runner_sha256),
        (&report.helper_sha256, &package.helper_sha256),
        (&report.maintainer_sha256, &package.maintainer_sha256),
        (&report.powershell_manifest_sha256, &package.powershell_manifest_sha256),
    ] {
        ensure!(actual == expected, "acceptance.binary_or_runtime_mismatch");
    }
    ensure!(
        matches!(
            report.cmd_image.sha256.as_ref(),
            Some(hash) if hash.algorithm == ContentHashAlgorithm::Sha256
        ),
        "acceptance.binary_or_runtime_mismatch"
    );
    verify_evidence(path, &report)?;
    Ok(VerifiedHost { report, report_sha256: ContentHash::sha256(&bytes) })
}

fn read_lineage(path: &Path) -> Result<AcceptanceLineage> {
    let bytes = read_bounded(
        path,
        MAX_PACKAGE_BYTES,
        "acceptance.lineage_not_file",
        "acceptance.lineage_limit",
    )?;
    let value = parse_json_strict(&bytes).map_err(|_| anyhow!("acceptance.lineage_json"))?;
    let lineage: AcceptanceLineage = serde_json::from_value(value)?;
    ensure!(
        lineage.schema == "windows-user-r06-r09-acceptance-lineage-v1",
        "acceptance.lineage_schema"
    );
    Ok(lineage)
}

fn verify_host_declared(
    path: &Path,
    edition: ExecutionHostEdition,
    declared: &AcceptanceLineageHost,
) -> Result<VerifiedHost> {
    let bytes = read_bounded(
        path,
        MAX_REPORT_BYTES,
        "acceptance.report_not_file",
        "acceptance.report_limit",
    )?;
    let report_value = parse_json_strict(&bytes).map_err(|_| anyhow!("acceptance.report_json"))?;
    let report: ExecutionHostAcceptanceReport = serde_json::from_value(report_value)?;
    ensure!(
        report.edition == edition
            && !report.windows_build.trim().is_empty()
            && report.assurance_version == ASSURANCE_VERSION
            && report.acceptance_version == EXECUTION_HOST_ACCEPTANCE_VERSION,
        "acceptance.host_or_assurance_mismatch"
    );
    ensure!(
        report.candidate_inventory_sha256 == declared.candidate_inventory_sha256,
        "acceptance.lineage_home_inventory_mismatch"
    );
    ensure!(
        report.source_fingerprint == declared.source_fingerprint,
        "acceptance.lineage_home_source_mismatch"
    );
    for (actual, expected) in [
        (&report.core_sha256, &declared.core_sha256),
        (&report.runner_sha256, &declared.runner_sha256),
        (&report.helper_sha256, &declared.helper_sha256),
        (&report.maintainer_sha256, &declared.maintainer_sha256),
        (&report.powershell_manifest_sha256, &declared.powershell_manifest_sha256),
    ] {
        ensure!(actual == expected, "acceptance.lineage_home_binary_mismatch");
    }
    ensure!(
        matches!(
            report.cmd_image.sha256.as_ref(),
            Some(hash) if hash.algorithm == ContentHashAlgorithm::Sha256
        ),
        "acceptance.binary_or_runtime_mismatch"
    );
    verify_evidence(path, &report)?;
    Ok(VerifiedHost { report, report_sha256: ContentHash::sha256(&bytes) })
}

fn verify_evidence(path: &Path, report: &ExecutionHostAcceptanceReport) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let root = if parent.as_os_str().is_empty() { Path::new(".") } else { parent }
        .canonicalize()
        .map_err(|_| anyhow!("acceptance.report_parent"))?;
    let mut relative_paths = HashSet::new();
    for item in &report.observations {
        ensure!(is_normal_relative_path(&item.evidence_path), "acceptance.evidence_path");
        ensure!(
            relative_paths.insert(item.evidence_path.to_lowercase()),
            "acceptance.evidence_path_duplicate"
        );
        let evidence = root
            .join(&item.evidence_path)
            .canonicalize()
            .map_err(|_| anyhow!("acceptance.evidence_missing"))?;
        ensure!(evidence.starts_with(&root), "acceptance.evidence_escape");
        let evidence_bytes = read_bounded(
            &evidence,
            MAX_EVIDENCE_BYTES,
            "acceptance.evidence_not_file",
            "acceptance.evidence_limit",
        )?;
        ensure!(
            ContentHash::sha256(&evidence_bytes) == item.evidence_sha256,
            "acceptance.evidence_changed"
        );
    }
    Ok(())
}

fn is_normal_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains([':', '\\'])
        && !value.starts_with('/')
        && !value.ends_with('/')
        && value.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && Path::new(value).components().all(|component| matches!(component, Component::Normal(_)))
}

fn read_bounded(
    path: &Path,
    limit: u64,
    not_file_error: &'static str,
    limit_error: &'static str,
) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)?;
    ensure!(metadata.is_file(), "{not_file_error}");
    ensure!(metadata.len() <= limit, "{limit_error}");
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= limit, "{limit_error}");
    Ok(bytes)
}
