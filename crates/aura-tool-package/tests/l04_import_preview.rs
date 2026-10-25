use aura_domain_common::{Revision, ToolPackageId};
use aura_tool_contract::{PackageResourceClaimKeyV1, TOOL_PACKAGE_MANIFEST_NAME};
use aura_tool_domain::{
    PackageConformanceStateV1, PackageExecutionLocationV1, PackageImportDiagnosticLocationV1,
    PackageRuntimeSupportV1, PackageVersionBindingV1, PackageWorkingDirectoryPreviewV1,
};
use aura_tool_package::*;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

const MANIFEST: &[u8] =
    include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
const RUNTIME: &[u8] = b"stdio fixture runtime";

fn write_source(root: &std::path::Path, manifest: &[u8]) {
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::write(root.join(TOOL_PACKAGE_MANIFEST_NAME), manifest).expect("manifest");
    fs::write(root.join("bin/fixture.exe"), RUNTIME).expect("runtime bytes only, never executed");
}

#[test]
fn preview_preserves_declared_working_directory_and_labels_program_credential_deferral() {
    let context = tempfile::tempdir().expect("temporary preview directories");
    let package = context.path().join("package");
    let workspace = context.path().join("workspace");
    let claimed = context.path().join("selected");
    for path in [&package, &workspace, &claimed] {
        fs::create_dir(path).expect("preview directory");
    }
    let claims =
        BTreeMap::from([(PackageResourceClaimKeyV1::new("output").expect("key"), claimed.clone())]);
    for (declared, expected) in [
        (json!({"type":"package_root"}), package.clone()),
        (json!({"type":"workspace_root"}), workspace.clone()),
        (json!({"type":"resolved_directory_claim","claim_key":"output"}), claimed),
    ] {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        let mut raw: serde_json::Value = serde_json::from_slice(MANIFEST).expect("fixture");
        raw["tools"][0]["runtime"]["working_directory"] = declared.clone();
        raw["tools"][0]["runtime"]["credential_slot"] =
            json!({"slot_key":"api_key","environment_variable":"AURA_L04_TEST_CREDENTIAL"});
        write_source(source.path(), &serde_json::to_vec(&raw).expect("source JSON"));
        let staged =
            stage_directory_package_v1(source.path(), staging.path()).expect("strict stage");
        let content = package_content_descriptor_v1(&staged).expect("actual fingerprint");
        let version = PackageVersionBindingV1 {
            package_id: ToolPackageId::new(),
            package_version: content.manifest.package_version.clone(),
            package_fingerprint: content.package_fingerprint.clone(),
            revision: Revision::INITIAL,
        };
        let directories = PackagePreviewDirectories {
            package_root: &package,
            workspace_root: Some(&workspace),
            directory_claims: &claims,
        };
        let preview = package_import_preview_v1(
            &staged,
            version.clone(),
            &directories,
            PackageConformanceStateV1::NotRun,
        )
        .expect("preview");
        assert_eq!(preview.runtimes[0].execution_location, PackageExecutionLocationV1::UserJob);
        assert_eq!(
            preview.runtimes[0].support,
            PackageRuntimeSupportV1::ProgramCredentialUnsupported
        );
        assert!(
            matches!(&preview.runtimes[0].working_directory,PackageWorkingDirectoryPreviewV1::Resolved { declared:actual,path } if serde_json::to_value(actual).expect("declaration")==declared && Path::new(path)==expected.as_path())
        );
        let absent = PackagePreviewDirectories {
            package_root: &package,
            workspace_root: None,
            directory_claims: &BTreeMap::new(),
        };
        let unresolved = package_import_preview_v1(
            &staged,
            version.clone(),
            &absent,
            PackageConformanceStateV1::NotRun,
        )
        .expect("preview before selection");
        if declared["type"] != "package_root" {
            assert!(matches!(
                unresolved.runtimes[0].working_directory,
                PackageWorkingDirectoryPreviewV1::Unresolved { .. }
            ));
        }
        if declared["type"] == "package_root" {
            let invalid = PackagePreviewDirectories {
                package_root: Path::new("relative"),
                workspace_root: None,
                directory_claims: &BTreeMap::new(),
            };
            assert!(matches!(
                package_import_preview_v1(
                    &staged,
                    version.clone(),
                    &invalid,
                    PackageConformanceStateV1::NotRun
                ),
                Err(PackageImportPreviewErrorV1::InvalidDirectoryDisplay)
            ));
        }
        let mut wrong = version;
        wrong.package_version =
            aura_domain_common::CanonicalSemVer::new("9.0.0").expect("different version");
        assert!(matches!(
            package_import_preview_v1(
                &staged,
                wrong,
                &directories,
                PackageConformanceStateV1::NotRun
            ),
            Err(PackageImportPreviewErrorV1::VersionBindingMismatch)
        ));
    }
}

#[test]
fn manifest_formatting_is_the_same_content_but_a_payload_byte_is_not() {
    let first = tempfile::tempdir().expect("first source");
    let second = tempfile::tempdir().expect("second source");
    let staging = tempfile::tempdir().expect("staging");
    let raw: serde_json::Value = serde_json::from_slice(MANIFEST).expect("fixture");
    write_source(first.path(), MANIFEST);
    write_source(second.path(), &serde_json::to_vec(&raw).expect("reformatted JSON"));
    let first = stage_directory_package_v1(first.path(), staging.path()).expect("first");
    let second = stage_directory_package_v1(second.path(), staging.path()).expect("second");
    assert_eq!(
        package_content_descriptor_v1(&first).expect("descriptor"),
        package_content_descriptor_v1(&second).expect("descriptor")
    );
    fs::write(second.root().join("bin/fixture.exe"), b"changed fixture bytes")
        .expect("tamper staging");
    assert!(
        package_content_descriptor_v1(&second).is_err(),
        "cached evidence must not hide a payload change"
    );
}

#[test]
fn diagnostics_retain_source_positions_without_internal_types_or_raw_values() {
    for source in
        [b"{\n\"schema_version\":1,\n\"schema_version\":1}".as_slice(), b"{\n\"oops\":]".as_slice()]
    {
        let error = parse_tool_package_manifest_v1(source).expect_err("invalid source");
        let diagnostic = manifest_import_diagnostic_v1(&error);
        assert!(
            matches!(diagnostic.location,PackageImportDiagnosticLocationV1::Manifest { line:Some(line),column:Some(column),.. } if line>0 && column>0)
        );
        assert!(
            !diagnostic.message.contains("Raw")
                && !diagnostic.message.contains("Canonical")
                && !diagnostic.message.contains("V1::")
        );
    }
}

#[test]
fn zip_and_directory_import_produce_the_same_v2_content_descriptor() {
    use std::io::Write;
    let source = tempfile::tempdir().expect("source");
    let staging = tempfile::tempdir().expect("staging");
    let archives = tempfile::tempdir().expect("archives");
    write_source(source.path(), MANIFEST);
    let archive = archives.path().join("fixture.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).expect("zip"));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in [(TOOL_PACKAGE_MANIFEST_NAME, MANIFEST), ("bin/fixture.exe", RUNTIME)] {
        zip.start_file(name, options).expect("entry");
        zip.write_all(bytes).expect("bytes");
    }
    zip.finish().expect("finish");
    let directory = stage_directory_package_v1(source.path(), staging.path()).expect("directory");
    let zip = stage_zip_package_v1(&archive, staging.path()).expect("zip");
    assert_eq!(
        package_content_descriptor_v1(&directory).expect("directory descriptor"),
        package_content_descriptor_v1(&zip).expect("zip descriptor")
    );
    drop((directory, zip));
    assert_eq!(
        fs::read_dir(staging.path()).expect("staging entries").count(),
        0,
        "temporary staging is RAII owned"
    );
}

#[test]
fn http_preview_keeps_capabilities_targets_resources_and_credentials_without_running_anything() {
    let source = tempfile::tempdir().expect("source");
    let staging = tempfile::tempdir().expect("staging");
    let mut raw: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/tool-package/manifest-v1/valid/declarative-http.json"
    ))
    .expect("HTTP fixture");
    raw["files"] = json!([]);
    fs::write(
        source.path().join(TOOL_PACKAGE_MANIFEST_NAME),
        serde_json::to_vec(&raw).expect("JSON"),
    )
    .expect("source");
    let staged = stage_directory_package_v1(source.path(), staging.path()).expect("HTTP package");
    let content = package_content_descriptor_v1(&staged).expect("descriptor");
    let version = PackageVersionBindingV1 {
        package_id: ToolPackageId::new(),
        package_version: content.manifest.package_version.clone(),
        package_fingerprint: content.package_fingerprint.clone(),
        revision: Revision::INITIAL,
    };
    let root = source.path().to_path_buf();
    let directories = PackagePreviewDirectories {
        package_root: &root,
        workspace_root: None,
        directory_claims: &BTreeMap::new(),
    };
    let preview = package_import_preview_v1(
        &staged,
        version,
        &directories,
        PackageConformanceStateV1::NotRun,
    )
    .expect("preview");
    assert_eq!(preview.runtimes[0].execution_location, PackageExecutionLocationV1::ControlledHttp);
    assert_eq!(
        preview.runtimes[0].working_directory,
        PackageWorkingDirectoryPreviewV1::NotApplicable
    );
    assert_eq!(preview.runtimes[0].support, PackageRuntimeSupportV1::ReadyForConformance);
    assert!(!preview.content.manifest.tools[0].capability_requirements.is_empty());
    assert!(!preview.content.manifest.tools[0].resource_claims.is_empty());
    let wire = serde_json::to_value(&preview).expect("wire");
    let request = &wire["content"]["manifest"]["tools"][0]["runtime"]["request"];
    assert_eq!(request["target"]["origin"], "https://example.invalid");
    assert!(!request["credential_slot"].is_null());
    assert!(
        serde_json::from_value::<aura_tool_domain::PackageImportPreviewV1>(wire.clone()).is_ok()
    );
    let mut malformed = wire.clone();
    malformed["contract_version"] = json!(2);
    assert!(serde_json::from_value::<aura_tool_domain::PackageImportPreviewV1>(malformed).is_err());
    let mut malformed = wire;
    malformed["unapproved"] = json!(true);
    assert!(serde_json::from_value::<aura_tool_domain::PackageImportPreviewV1>(malformed).is_err());
}
