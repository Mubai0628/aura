//! Tool-package route contracts, owned by the tool domain and collected here.
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{Revision, UtcTimestamp};
use aura_tool_domain::*;
use std::str::FromStr;

pub(crate) const PATH: &str = "fixtures/contracts/package-routes.v1.json";

pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<PackageListV1>();
    generator.subschema_for::<PackageImportSourceV1>();
    generator.subschema_for::<PackageImportOutcomeV1>();
    generator.subschema_for::<PackageVersionRefV1>();
    generator.subschema_for::<PackageVersionViewV1>();
    generator.subschema_for::<PackageConformancePrepareRequestV1>();
    generator.subschema_for::<PackageConformancePreparationV1>();
    generator.subschema_for::<PackageConformanceStartRequestV1>();
    generator.subschema_for::<PackageConformanceCancelRequestV1>();
    generator.subschema_for::<PackageConformanceViewV1>();
    generator.subschema_for::<PackageInstallRequestV1>();
    generator.subschema_for::<PackageEnableRequestV1>();
    generator.subschema_for::<PackageChangeRequestV1>();
    generator.subschema_for::<PackageCredentialBindRequestV1>();
    generator.subschema_for::<PackageCredentialClearRequestV1>();
    generator.subschema_for::<PackageSummaryV1>();
}

/// Deterministic examples of every package route value, built from the canonical
/// package fixture so TypeScript and Rust decode the same bytes.
pub(crate) fn bytes() -> anyhow::Result<Vec<u8>> {
    let tools = crate::canonical_fixture()?.tools;
    let content = PackageContentDescriptorV1 {
        manifest: tools.canonical_package_manifest.ok_or_else(|| anyhow::anyhow!("manifest"))?,
        package_fingerprint: tools
            .package_fingerprint
            .ok_or_else(|| anyhow::anyhow!("fingerprint"))?,
        security_fingerprint: tools
            .package_security_fingerprint
            .ok_or_else(|| anyhow::anyhow!("security fingerprint"))?,
        runtime_closure_hash: tools
            .package_runtime_closure_hash
            .ok_or_else(|| anyhow::anyhow!("closure"))?,
    };
    let package_id = "tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?;
    let at = UtcTimestamp::from_str("2026-10-16T00:00:00Z")?;
    let version = PackageVersionBindingV1 {
        package_id,
        package_version: content.manifest.package_version.clone(),
        package_fingerprint: content.package_fingerprint.clone(),
        revision: Revision::new(2),
    };
    let reference = PackageVersionRefV1 {
        package_id,
        package_version: version.package_version.clone(),
        package_fingerprint: version.package_fingerprint.clone(),
    };
    let tool = &content.manifest.tools[0];
    let report = PackageConformanceReportV1 {
        run_id: "tcr_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?,
        version: version.clone(),
        started_at: at,
        completed_at: at,
        cases: vec![PackageConformanceCaseResultV1 {
            tool_key: tool.tool_key.clone(),
            case_name: tool.conformance_cases[0].name.clone(),
            execution_location: PackageExecutionLocationV1::UserJob,
            reason: PackageConformanceReasonV1::Completed,
            runtime_evidence_hash: CanonicalSha256HashV1::sha256(b"fixture evidence"),
        }],
        cleanup_verified: true,
        settings_hash: package_settings_hash_v1(&[])?,
    };
    let conformance = PackageConformanceViewV1 {
        version: version.clone(),
        state: PackageConformanceStateV1::Succeeded,
        run_id: Some(report.run_id),
        started_at: Some(at),
        report_hash: Some(aura_domain_common::jcs::canonical_sha256(&report)?),
        report: Some(report),
    };
    let preview = aura_tool_package::package_content_preview_v1(
        content.clone(),
        version.clone(),
        conformance.state,
    )?;
    let view = PackageVersionViewV1 {
        preview: preview.clone(),
        conformance: conformance.clone(),
        settings: vec![],
        package_revision: Revision::new(3),
    };
    let (tool_id, tool_instance_id) =
        package_tool_identity_v1(package_id, &tool.tool_key, Revision::new(4));
    let summary = PackageSummaryV1 {
        package_id,
        package_key: content.manifest.package_key.clone(),
        display_name: content.manifest.ui.display_name.as_str().to_owned(),
        revision: Revision::new(4),
        installed: Some(PackageInstalledV1 {
            version: reference.clone(),
            enabled: true,
            trusted: true,
            tools: vec![PackageToolSummaryV1 {
                tool_key: tool.tool_key.clone(),
                display_name: tool.ui.display_name.as_str().to_owned(),
                canonical_name: tool.model.canonical_name.clone(),
                execution_location: PackageExecutionLocationV1::UserJob,
                tool_id,
                tool_instance_id,
                credential: None,
            }],
        }),
        versions: vec![PackageVersionSummaryV1 {
            version: version.clone(),
            conformance: PackageConformanceStateV1::Succeeded,
            imported_at: at,
        }],
    };
    let value = serde_json::json!({
        "import_source": PackageImportSourceV1 { path: r"C:\Synthetic\汉字 package".into() },
        "import_imported": PackageImportOutcomeV1::Imported { view: Box::new(view.clone()) },
        "import_cancelled": PackageImportOutcomeV1::Cancelled,
        "version_ref": reference.clone(),
        "version_view": view,
        "prepare": PackageConformancePrepareRequestV1 { version: version.clone(), settings: vec![] },
        "preparation": PackageConformancePreparationV1 {
            preparation_id: "0123456789abcdef0123456789abcdef".into(),
            preview,
            preview_hash: CanonicalSha256HashV1::sha256(b"fixture preview"),
        },
        "start": PackageConformanceStartRequestV1 {
            preparation_id: "0123456789abcdef0123456789abcdef".into(),
            preview_hash: CanonicalSha256HashV1::sha256(b"fixture preview"),
        },
        "conformance": conformance.clone(),
        "cancel": PackageConformanceCancelRequestV1 { version: reference, run_id: "tcr_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()? },
        "install": PackageInstallRequestV1 {
            version,
            expected_package_revision: Revision::new(3),
            report_hash: conformance.report_hash.clone().ok_or_else(|| anyhow::anyhow!("hash"))?,
        },
        "enable": PackageEnableRequestV1 { package_id, expected_package_revision: Revision::new(4), enabled: false },
        "change": PackageChangeRequestV1 { package_id, expected_package_revision: Revision::new(4) },
        "list": PackageListV1 { packages: vec![summary] },
    });
    let mut bytes = serde_json::to_vec_pretty(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
