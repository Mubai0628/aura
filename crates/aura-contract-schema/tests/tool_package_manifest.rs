use aura_contract_schema::{
    RUST_GENERATED_PATHS, TOOL_PACKAGE_CANONICAL_FIXTURE_PATH,
    TOOL_PACKAGE_CANONICAL_HTTP_ARRAY_ORDER_FIXTURE_PATH,
    TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_NESTED_FIXTURE_PATH,
    TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_UNICODE_FIXTURE_PATH,
    TOOL_PACKAGE_RUNTIME_CLOSURE_FIXTURE_PATH,
    TOOL_PACKAGE_VALID_RAW_HTTP_PROPERTIES_UNSORTED_FIXTURE_PATH,
    TOOL_PACKAGE_VALID_RAW_MULTILINE_FIXTURE_PATH, TOOL_PACKAGE_VALID_STDIO_FIXTURE_PATH,
    additional_canonical_tool_package_manifest_fixtures, canonical_fixture, canonical_schema_value,
    canonical_tool_package_manifest_fixture, canonical_tool_package_manifest_schema_value,
    invalid_canonical_tool_package_manifest_fixtures, invalid_tool_package_manifest_fixtures,
    package_runtime_closure_fixture, semantic_invalid_canonical_tool_package_manifest_fixtures,
    semantic_invalid_tool_package_manifest_fixtures, tool_package_manifest_schema_value,
    valid_tool_package_manifest_fixtures,
};
use aura_domain_common::jcs::{
    canonical_blake3, canonical_json_jcs, canonical_sha256, parse_json_strict,
};
use aura_tool_contract::{MAX_TOOL_PACKAGE_DECLARED_FILES_V1, TOOL_PACKAGE_IMPORT_POLICY_V1};
use aura_tool_domain::{CanonicalToolPackageManifestV1, RawToolPackageManifestV1};
use jsonschema::Draft;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[test]
fn generated_manifest_fixture_inventory_is_complete_unique_and_stable() {
    let valid = valid_tool_package_manifest_fixtures().expect("valid fixtures");
    let structural_invalid =
        invalid_tool_package_manifest_fixtures().expect("structural invalid fixtures");
    let semantic_invalid =
        semantic_invalid_tool_package_manifest_fixtures().expect("semantic invalid fixtures");
    let canonical_structural_invalid =
        invalid_canonical_tool_package_manifest_fixtures().expect("canonical structural invalid");
    let canonical_semantic_invalid = semantic_invalid_canonical_tool_package_manifest_fixtures()
        .expect("canonical semantic invalid");
    let additional_canonical =
        additional_canonical_tool_package_manifest_fixtures().expect("canonical fixtures");

    assert_eq!(valid.len(), 9);
    // Includes the two `LOCALAPPDATA` cases: a package may not declare a name Core
    // binds to the attempt's isolated directory, neither as an environment entry nor
    // as a credential destination.
    assert_eq!(structural_invalid.len(), 52);
    assert_eq!(semantic_invalid.len(), 24);
    assert_eq!(canonical_structural_invalid.len(), 7);
    assert_eq!(canonical_semantic_invalid.len(), 2);
    assert_eq!(1 + additional_canonical.len(), 5);

    let mut emitted_paths = valid
        .iter()
        .chain(&structural_invalid)
        .chain(&semantic_invalid)
        .chain(&canonical_structural_invalid)
        .chain(&canonical_semantic_invalid)
        .chain(&additional_canonical)
        .map(|(path, _)| *path)
        .collect::<Vec<_>>();
    emitted_paths
        .extend([TOOL_PACKAGE_CANONICAL_FIXTURE_PATH, TOOL_PACKAGE_RUNTIME_CLOSURE_FIXTURE_PATH]);
    assert_eq!(emitted_paths.len(), 100);
    assert_eq!(
        emitted_paths.iter().copied().collect::<BTreeSet<_>>().len(),
        emitted_paths.len(),
        "fixture paths must be globally unique"
    );

    let registered_paths = RUST_GENERATED_PATHS
        .iter()
        .copied()
        .filter(|path| path.starts_with("fixtures/tool-package/manifest-v1/"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        emitted_paths.into_iter().collect::<BTreeSet<_>>(),
        registered_paths,
        "the generator registry must reference every and only the review fixtures"
    );
}

#[test]
fn raw_manifest_golden_fixtures_match_the_generated_schema_and_rust_contract() {
    let schema = tool_package_manifest_schema_value().expect("raw manifest schema");
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("compile raw manifest schema");

    for (path, fixture) in valid_tool_package_manifest_fixtures().expect("valid fixtures") {
        assert!(validator.is_valid(&fixture), "schema rejected valid fixture {path}");
        serde_json::from_value::<RawToolPackageManifestV1>(fixture)
            .unwrap_or_else(|error| panic!("Rust contract rejected valid fixture {path}: {error}"));
    }
}

#[test]
fn strict_json_gate_rejects_duplicate_keys_inside_manifest_json_documents() {
    let cases: [(&str, &[u8]); 3] = [
        (
            "input_schema",
            br#"{"tools":[{"model":{"input_schema":{"type":"object","type":"object"}}}]}"#,
        ),
        (
            "result_schema",
            br#"{"tools":[{"model":{"result_contract":{"type":"structured_json","result_schema":{"type":"string","type":"string"}}}}]}"#,
        ),
        (
            "conformance arguments",
            br#"{"tools":[{"conformance_cases":[{"arguments":{"value":1,"value":2}}]}]}"#,
        ),
    ];

    for (label, bytes) in cases {
        assert!(parse_json_strict(bytes).is_err(), "strict JSON accepted duplicate {label} key");
    }
}

#[test]
fn strict_json_gate_rejects_trailing_json_or_non_whitespace() {
    let fixture = valid_tool_package_manifest_fixtures()
        .expect("valid fixtures")
        .into_iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_STDIO_FIXTURE_PATH)
        .map(|(_, fixture)| fixture)
        .expect("stdio fixture");
    let mut trailing_value = serde_json::to_vec(&fixture).expect("manifest JSON");
    trailing_value.extend_from_slice(b"\n{}");
    assert!(parse_json_strict(&trailing_value).is_err());

    let mut trailing_junk = serde_json::to_vec(&fixture).expect("manifest JSON");
    trailing_junk.extend_from_slice(b" trailing");
    assert!(parse_json_strict(&trailing_junk).is_err());

    let mut trailing_whitespace = serde_json::to_vec(&fixture).expect("manifest JSON");
    trailing_whitespace.extend_from_slice(b" \r\n\t");
    assert!(parse_json_strict(&trailing_whitespace).is_ok());
}

#[test]
fn invalid_manifest_golden_fixtures_fail_closed_in_schema_and_rust() {
    let schema = tool_package_manifest_schema_value().expect("raw manifest schema");
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("compile raw manifest schema");

    for (path, fixture) in invalid_tool_package_manifest_fixtures().expect("invalid fixtures") {
        assert!(!validator.is_valid(&fixture), "schema accepted invalid fixture {path}");
        assert!(
            serde_json::from_value::<RawToolPackageManifestV1>(fixture).is_err(),
            "Rust contract accepted invalid fixture {path}"
        );
    }
}

#[test]
fn package_file_and_runtime_entrypoint_reject_the_full_windows_name_set() {
    let schema = tool_package_manifest_schema_value().expect("raw manifest schema");
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("compile raw manifest schema");
    let base = valid_tool_package_manifest_fixtures()
        .expect("valid fixtures")
        .into_iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_STDIO_FIXTURE_PATH)
        .map(|(_, fixture)| fixture)
        .expect("stdio fixture");

    let mut invalid_names = vec![
        "bad<name".to_owned(),
        "bad>name".to_owned(),
        "bad\"name".to_owned(),
        "bad|name".to_owned(),
        "bad?name".to_owned(),
        "bad*name".to_owned(),
        "CON.txt".to_owned(),
        "PRN.bin".to_owned(),
        "AUX.log".to_owned(),
        "NUL.data".to_owned(),
        "COM¹.exe".to_owned(),
        "COM².dll".to_owned(),
        "COM³.txt".to_owned(),
        "LPT¹.exe".to_owned(),
        "LPT².dll".to_owned(),
        "LPT³.txt".to_owned(),
    ];
    invalid_names.extend(
        (1..=9).flat_map(|number| [format!("COM{number}.exe"), format!("LPT{number}.dll")]),
    );

    for name in invalid_names {
        for (surface, runtime_entrypoint) in [("files[].path", false), ("runtime.executable", true)]
        {
            let mut fixture = base.clone();
            if runtime_entrypoint {
                fixture["tools"][0]["runtime"]["executable"] = json!(format!("bin/{name}"));
            } else {
                fixture["files"][0]["path"] = json!(format!("bin/{name}"));
            }
            assert!(!validator.is_valid(&fixture), "schema accepted {surface}={name}");
            assert!(
                serde_json::from_value::<RawToolPackageManifestV1>(fixture).is_err(),
                "Rust contract accepted {surface}={name}"
            );
        }
    }
}

#[test]
fn semantic_invalid_manifest_fixtures_are_rejected_by_the_rust_contract() {
    for (path, fixture) in
        semantic_invalid_tool_package_manifest_fixtures().expect("semantic invalid fixtures")
    {
        assert!(
            serde_json::from_value::<RawToolPackageManifestV1>(fixture).is_err(),
            "Rust semantic validator accepted invalid fixture {path}"
        );
    }
}

#[test]
fn canonical_required_nullable_omissions_fail_in_schema_and_rust() {
    let schema = canonical_tool_package_manifest_schema_value().expect("canonical schema");
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("compile canonical manifest schema");

    for (path, fixture) in
        invalid_canonical_tool_package_manifest_fixtures().expect("invalid canonical fixtures")
    {
        assert!(!validator.is_valid(&fixture), "canonical schema accepted {path}");
        assert!(
            serde_json::from_value::<CanonicalToolPackageManifestV1>(fixture).is_err(),
            "canonical Rust contract accepted {path}"
        );
    }
}

#[test]
fn canonical_semantic_order_violations_fail_closed_in_rust() {
    for (path, fixture) in
        semantic_invalid_canonical_tool_package_manifest_fixtures().expect("semantic canonical")
    {
        assert!(
            serde_json::from_value::<CanonicalToolPackageManifestV1>(fixture).is_err(),
            "canonical Rust contract accepted semantic violation {path}"
        );
    }
}

#[test]
fn additional_canonical_fixtures_match_schema_and_rust_contract() {
    let schema = canonical_tool_package_manifest_schema_value().expect("canonical schema");
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("compile canonical manifest schema");
    for (path, fixture) in
        additional_canonical_tool_package_manifest_fixtures().expect("canonical fixtures")
    {
        assert!(validator.is_valid(&fixture), "canonical schema rejected {path}");
        serde_json::from_value::<CanonicalToolPackageManifestV1>(fixture)
            .unwrap_or_else(|error| panic!("canonical Rust contract rejected {path}: {error}"));
    }
}

#[test]
fn canonical_manifest_fixture_requires_explicit_defaults_and_round_trips() {
    let fixture = canonical_tool_package_manifest_fixture().expect("canonical fixture");
    let encoded = serde_json::to_value(&fixture).expect("serialize canonical fixture");
    let schema = canonical_tool_package_manifest_schema_value().expect("canonical schema");
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("compile canonical manifest schema");

    let errors = validator.iter_errors(&encoded).map(|error| error.to_string()).collect::<Vec<_>>();
    assert!(errors.is_empty(), "canonical schema rejected canonical fixture: {errors:?}");
    let decoded: CanonicalToolPackageManifestV1 =
        serde_json::from_value(encoded.clone()).expect("canonical Rust round trip");
    assert_eq!(decoded, fixture);
    assert_eq!(encoded["tools"][0]["semantic_version"], serde_json::Value::Null);
    assert_eq!(encoded["tools"][0]["instance_configuration"], serde_json::Value::Null);
    assert_eq!(encoded["ui"]["description"], serde_json::Value::Null);
    assert_eq!(encoded["tools"][0]["audience"], "subagent_configurable");
    assert_eq!(
        encoded["tools"][0]["runtime"]["command"]["credential_slot"],
        serde_json::Value::Null
    );
    assert_eq!(
        encoded["tools"][0]["runtime"]["command"]["success_exit_codes"],
        serde_json::json!([0])
    );
}

#[test]
fn raw_multiline_text_normalizes_crlf_and_cr_before_canonical_validation() {
    let fixture = valid_tool_package_manifest_fixtures()
        .expect("valid fixtures")
        .into_iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_RAW_MULTILINE_FIXTURE_PATH)
        .map(|(_, fixture)| fixture)
        .expect("raw multiline fixture");
    let manifest: RawToolPackageManifestV1 =
        serde_json::from_value(fixture).expect("raw multiline contract");
    assert_eq!(
        manifest.ui.description.as_ref().expect("UI description").as_str(),
        "Package line one\nline two\nline three"
    );
    assert_eq!(
        manifest.tools[0].model.description.as_str(),
        "Model line one\nline two\nline three"
    );
}

#[test]
fn package_runtime_closure_is_v1_ordered_and_hashable() {
    let closure = package_runtime_closure_fixture().expect("runtime closure fixture");
    assert_eq!(closure["policy_version"], serde_json::json!(1));
    let files = closure["files"].as_array().expect("closure files");
    assert!(files.windows(2).all(|pair| {
        pair[0]["path"].as_str().expect("left path").as_bytes()
            < pair[1]["path"].as_str().expect("right path").as_bytes()
    }));
    assert!(aura_domain_common::jcs::canonical_sha256(&closure).is_ok());
}

#[test]
fn manifest_file_count_and_runtime_root_byte_limits_include_the_root_manifest() {
    let raw_fixture = valid_tool_package_manifest_fixtures()
        .expect("valid fixtures")
        .into_iter()
        .find(|(path, _)| path.ends_with("aura-stdio.json"))
        .map(|(_, value)| value)
        .expect("stdio fixture");
    let source_file = raw_fixture["files"][0].clone();

    let mut at_limit = raw_fixture.clone();
    at_limit["files"] = Value::Array(
        (0..MAX_TOOL_PACKAGE_DECLARED_FILES_V1)
            .map(|index| {
                let mut file = source_file.clone();
                file["path"] = serde_json::json!(format!("runtime/{index:04}.bin"));
                file["byte_length"] = serde_json::json!(0);
                file
            })
            .collect(),
    );
    serde_json::from_value::<RawToolPackageManifestV1>(at_limit.clone())
        .expect("4095 authored files remain valid before source-byte accounting");

    at_limit["files"].as_array_mut().expect("files").push({
        let mut file = source_file;
        file["path"] = serde_json::json!("runtime/4095.bin");
        file["byte_length"] = serde_json::json!(0);
        file
    });
    assert!(
        serde_json::from_value::<RawToolPackageManifestV1>(at_limit).is_err(),
        "4096 authored files plus the root Manifest must exceed the 4096-entry Package policy"
    );

    let mut oversized_manifest =
        canonical_tool_package_manifest_fixture().expect("canonical manifest for source-byte gate");
    let source_tool = oversized_manifest.tools[0].clone();
    for ordinal in 1..=2_048 {
        let mut tool = source_tool.clone();
        tool.tool_key = format!("fixture_{ordinal:04}").parse().expect("tool key");
        tool.model.canonical_name = format!("user.org.aura.fixture.canonical.fixture_{ordinal:04}")
            .parse()
            .expect("canonical name");
        oversized_manifest.tools.push(tool);
    }
    let oversized_bytes = canonical_json_jcs(&oversized_manifest).expect("oversized bytes");
    assert!(
        oversized_bytes.len()
            > usize::try_from(TOOL_PACKAGE_IMPORT_POLICY_V1.max_manifest_bytes)
                .expect("manifest limit fits usize")
    );
    assert!(oversized_bytes.len() > 1_048_576);
}

#[test]
fn raw_http_json_properties_canonicalize_recursively_without_reordering_arrays() {
    let raw_fixture = valid_tool_package_manifest_fixtures()
        .expect("valid fixtures")
        .into_iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_RAW_HTTP_PROPERTIES_UNSORTED_FIXTURE_PATH)
        .map(|(_, value)| value)
        .expect("raw unsorted fixture");
    let mut reordered_fixture = raw_fixture.clone();
    reordered_fixture["tools"][0]["runtime"]["request"]["body"]["template"]["properties"]
        .as_array_mut()
        .expect("raw properties")
        .reverse();

    let canonical = serde_json::from_value::<RawToolPackageManifestV1>(raw_fixture)
        .expect("raw manifest")
        .into_canonical_contract()
        .expect("canonical manifest");
    let reordered = serde_json::from_value::<RawToolPackageManifestV1>(reordered_fixture)
        .expect("reordered raw manifest")
        .into_canonical_contract()
        .expect("reordered canonical manifest");
    assert_eq!(
        canonical_json_jcs(&canonical).expect("canonical bytes"),
        canonical_json_jcs(&reordered).expect("reordered canonical bytes"),
        "property author order must not alter the canonical Manifest fingerprint input"
    );

    let encoded = serde_json::to_value(canonical).expect("canonical value");
    let properties = encoded["tools"][0]["runtime"]["request"]["body"]["template"]["properties"]
        .as_array()
        .expect("canonical properties");
    let names = properties
        .iter()
        .map(|property| property["name"].as_str().expect("property name"))
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["a", "array", "z"]);
    let nested = properties[0]["value"]["properties"].as_array().expect("nested properties");
    assert_eq!(nested[0]["name"], "nested_a");
    assert_eq!(nested[1]["name"], "nested_z");
    let array_items = properties[1]["value"]["items"].as_array().expect("array items");
    assert_eq!(array_items[0]["value"], "second");
    assert_eq!(array_items[1]["value"], "first");
}

#[test]
fn http_json_property_order_uses_utf16_code_units() {
    let fixture = additional_canonical_tool_package_manifest_fixtures()
        .expect("canonical fixtures")
        .into_iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_UNICODE_FIXTURE_PATH)
        .map(|(_, value)| value)
        .expect("Unicode ordering fixture");
    let properties = fixture["tools"][0]["runtime"]["request"]["body"]["template"]["properties"]
        .as_array()
        .expect("properties");
    assert_eq!(properties[0]["name"], "\u{10000}");
    assert_eq!(properties[1]["name"], "\u{e000}");

    for path in [
        TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_NESTED_FIXTURE_PATH,
        TOOL_PACKAGE_CANONICAL_HTTP_ARRAY_ORDER_FIXTURE_PATH,
    ] {
        assert!(
            additional_canonical_tool_package_manifest_fixtures()
                .expect("canonical fixtures")
                .iter()
                .any(|(candidate, _)| *candidate == path)
        );
    }
}

#[test]
fn generated_hash_and_schema_document_contracts_freeze_v1_literals() {
    let schema = canonical_tool_package_manifest_schema_value().expect("canonical schema");
    let definitions = schema["$defs"].as_object().expect("schema definitions");

    let document = &definitions["CanonicalJsonSchemaDocumentV1"];
    assert_eq!(document["properties"]["schema_policy_version"]["const"], 1);
    assert_eq!(document["properties"]["document"]["type"], "object");
    assert_eq!(document["properties"]["schema_hash"]["properties"]["algorithm"]["const"], "sha256");
    assert_eq!(
        document["properties"]["schema_hash"]["properties"]["canonicalization_version"]["const"],
        "rfc8785_jcs_v1"
    );

    let file_hash = &definitions["ToolPackageFileSha256V1"];
    assert_eq!(file_hash["properties"]["algorithm"]["const"], "sha256");
    assert!(file_hash["properties"].get("canonicalization_version").is_none());
    assert_eq!(file_hash["properties"]["hex"]["pattern"], "^[0-9a-f]{64}$");

    let contract_schema = canonical_schema_value().expect("contract schema");
    let contract_definitions = contract_schema["$defs"].as_object().expect("contract definitions");
    let visibility = &contract_definitions["PackageRuntimeVisibilityPolicyV1"];
    assert_eq!(visibility["const"], "full_package");
    let closure_version = &contract_definitions["PackageRuntimeClosurePolicyVersionV1"];
    assert_eq!(closure_version["const"], 1);
    assert_eq!(contract_definitions["FingerprintPreimageVersionV1"]["const"], 1);
    for (kind, wire) in [
        ("ToolContractFingerprintKindV1", "aura.p07.tool_contract_fingerprint"),
        ("PackageFingerprintKindV1", "aura.p07.package_fingerprint"),
        ("PackageSecurityFingerprintKindV1", "aura.p07.package_security_fingerprint"),
    ] {
        assert_eq!(contract_definitions[kind]["const"], wire);
    }
    for preimage in [
        "ToolContractFingerprintPreimageV1",
        "PackageFingerprintPreimageV1",
        "PackageSecurityFingerprintPreimageV1",
    ] {
        assert_eq!(contract_definitions[preimage]["additionalProperties"], false);
        assert!(
            contract_definitions[preimage]["required"]
                .as_array()
                .expect("required preimage fields")
                .contains(&json!("preimage_version"))
        );
    }
}

#[test]
fn generated_fingerprint_preview_recomputes_from_each_exact_preimage() {
    let fixture = canonical_fixture().expect("canonical contract fixture");
    let tools = fixture.tools;
    let tool_preimage = tools.tool_contract_fingerprint_preimage.as_ref().expect("Tool preimage");
    let tool_fingerprint = tools.tool_contract_fingerprint.as_ref().expect("Tool fingerprint");
    assert_eq!(tool_preimage["tool_key"], tool_preimage["tool_contract"]["tool_key"]);
    assert_eq!(
        tool_fingerprint.fingerprint,
        canonical_sha256(tool_preimage).expect("Tool preimage hash")
    );
    assert_eq!(
        serde_json::to_value(&tool_fingerprint.package_runtime_closure_hash)
            .expect("Closure hash JSON"),
        tool_preimage["runtime_closure_hash"]
    );

    let package_preimage = tools.package_fingerprint_preimage.as_ref().expect("Package preimage");
    let package_fingerprint = tools.package_fingerprint.as_ref().expect("Package fingerprint");
    assert_eq!(
        package_fingerprint.0,
        canonical_sha256(package_preimage).expect("Package preimage hash")
    );
    assert_eq!(
        package_preimage["package_key"],
        package_preimage["canonical_manifest"]["package_key"]
    );
    assert_eq!(
        package_preimage["package_version"],
        package_preimage["canonical_manifest"]["package_version"]
    );
    assert_eq!(
        package_preimage["runtime_closure"]["canonical_manifest_hash"],
        serde_json::to_value(
            canonical_sha256(&package_preimage["canonical_manifest"])
                .expect("Canonical Manifest hash")
        )
        .expect("Canonical Manifest hash JSON")
    );

    let security_preimage =
        tools.package_security_fingerprint_preimage.as_ref().expect("Security preimage");
    let security_fingerprint =
        tools.package_security_fingerprint.as_ref().expect("Security fingerprint");
    assert_eq!(
        security_fingerprint.0,
        canonical_blake3(security_preimage).expect("Security preimage hash")
    );
    assert_ne!(
        security_fingerprint.0,
        canonical_blake3(package_fingerprint).expect("Package fingerprint output hash")
    );

    let mut package_facts = package_preimage.clone();
    let mut security_facts = security_preimage.clone();
    package_facts.as_object_mut().expect("Package preimage object").remove("kind");
    security_facts.as_object_mut().expect("Security preimage object").remove("kind");
    assert_eq!(package_facts, security_facts);
}

#[test]
fn generated_manifest_schema_freezes_runtime_text_limits_and_reserved_names() {
    let schema = tool_package_manifest_schema_value().expect("raw manifest schema");
    let definitions = schema["$defs"].as_object().expect("schema definitions");

    assert_eq!(definitions["InstanceConfigurationKeyV1"]["maxLength"], 64);
    assert_eq!(
        definitions["InstanceConfigurationKeyV1"]["pattern"],
        "^[a-z0-9]+(?:[-_][a-z0-9]+)*$"
    );
    assert_eq!(definitions["CommandLiteralV1"]["maxLength"], 8192);
    assert_eq!(definitions["CommandFlagV1"]["maxLength"], 256);
    assert_eq!(definitions["CommandOptionNameV1"]["maxLength"], 256);
    assert_eq!(definitions["EnvironmentVariableNameV1"]["maxLength"], 64);
    assert_eq!(definitions["EnvironmentVariableNameV1"]["pattern"], "^[A-Z_][A-Z0-9_]{0,63}$");
    let controlled_environment_names = definitions["EnvironmentVariableNameV1"]["not"]["enum"]
        .as_array()
        .expect("Core-controlled environment names");
    for name in ["AURA_TOOL_CALL_ID", "AURA_TOOL_EXECUTION_ATTEMPT_ID"] {
        assert!(controlled_environment_names.iter().any(|value| value == name), "{name}");
    }
    assert_eq!(definitions["EnvironmentLiteralValueV1"]["maxLength"], 8192);
    assert_eq!(definitions["FixedHttpsOriginV1"]["maxLength"], 2048);
    assert_eq!(definitions["HttpBasePathV1"]["maxLength"], 2048);
    assert_eq!(definitions["HttpPathLiteralSegmentV1"]["maxLength"], 512);
    assert_eq!(definitions["HttpQueryNameV1"]["maxLength"], 128);
    assert_eq!(definitions["HttpQueryLiteralValueV1"]["maxLength"], 8192);
    assert_eq!(definitions["HttpHeaderLiteralValueV1"]["maxLength"], 8192);
    assert_eq!(definitions["HttpJsonPropertyNameV1"]["maxLength"], 256);
    assert_eq!(schema["properties"]["files"]["maxItems"], 4095);
    assert_eq!(definitions["ToolPackageFileEntryV1"]["properties"]["byte_length"]["minimum"], 0);
    assert_eq!(
        definitions["ToolPackageFileEntryV1"]["properties"]["byte_length"]["maximum"],
        67_108_864
    );
    for definition in ["HttpQueryNameV1", "HttpQueryLiteralValueV1", "HttpJsonPropertyNameV1"] {
        assert!(
            definitions[definition]["pattern"]
                .as_str()
                .expect("control-rejecting pattern")
                .contains("\\u0000-\\u001f\\u007f")
        );
    }
    assert!(
        definitions["HttpHeaderNameV1"]["not"]["pattern"]
            .as_str()
            .expect("controlled header pattern")
            .contains("[Ss][Ee][Tt]-[Cc][Oo][Oo][Kk][Ii][Ee]")
    );
}
