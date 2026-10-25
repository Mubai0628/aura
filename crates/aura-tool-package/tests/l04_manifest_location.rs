use aura_tool_package::{ToolPackageManifestParseErrorV1, parse_tool_package_manifest_v1};

const MANIFEST: &str =
    include_str!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");

#[test]
fn raw_contract_errors_preserve_the_authored_source_line_and_column() {
    for source in [
        MANIFEST.replacen('{', "{\n\"unexpected_l04_field\": true,", 1),
        MANIFEST.replace("\"schema_version\": 1", "\"schema_version\": 999"),
    ] {
        let error = parse_tool_package_manifest_v1(source.as_bytes()).expect_err("bad contract");
        let ToolPackageManifestParseErrorV1::RawContract(error) = error else {
            panic!("expected a typed contract refusal");
        };
        assert!(error.line() > 0, "the original manifest line must survive strict validation");
        assert!(error.column() > 0, "the original manifest column must survive strict validation");
    }
}
