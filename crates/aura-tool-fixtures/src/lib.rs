#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
};

use aura_domain_common::jcs::{
    CanonicalHashV1, CanonicalSha256HashV1, canonical_json_jcs, canonical_sha256,
};
use aura_security_contract::NetworkDataClass;
use aura_tool_contract::{
    ArgumentJsonPointer, HttpHeaderValueSourceV1, HttpJsonTemplateV1, HttpPathSegmentV1,
    HttpValueSourceV1, ToolKey, ToolPackageFileSha256V1,
};
use aura_tool_domain::{
    CanonicalToolPackageDeclarativeHttpV1, CanonicalToolPackageRuntimeV1,
    ConformanceSyntheticCredentialPlacementV1, ToolPackageConformanceCaseNameV1,
};
use aura_tool_package::{
    ConformanceObservedBytesV1, ConformanceRuntimeErrorV1, ConformanceRuntimeHostV1,
    ConformanceRuntimeInvocationV1, ConformanceRuntimeObservationV1, ConformanceRuntimeTerminalV1,
    conformance_fixture_request_hash_v1,
};
use aura_tool_runtime::{
    HttpArgumentEgressAuthorizationV1, OrdinaryToolIoMeterV1, PreparedDeclarativeHttpRequestV1,
    PreparedHttpCredentialSlotV1, ResolvedToolHttpEndpointV1, prepare_declarative_http_request_v1,
};
use zeroize::Zeroizing;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FixtureCaseKey {
    tool_key: ToolKey,
    case_name: ToolPackageConformanceCaseNameV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HermeticFixtureTerminalV1 {
    Succeeded,
    Failed,
    Cancelled,
    Indeterminate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HermeticFixtureCaseV1 {
    pub tool_key: ToolKey,
    pub case_name: ToolPackageConformanceCaseNameV1,
    pub expected_request_hash: Option<CanonicalSha256HashV1>,
    pub response_status: Option<u16>,
    pub result_body: Vec<u8>,
    pub terminal: HermeticFixtureTerminalV1,
    pub protocol_verified: bool,
    pub artifacts_verified: bool,
    pub cleanup_verified: bool,
    /// Test-only fault injection. The host copies the synthetic canary to the
    /// selected observed channel so the W3 leak gate can prove it fails.
    pub leak_synthetic_credential_to: Option<&'static str>,
}

/// Hermetic W3 host. It never opens a socket or resolves DNS. HTTP execution
/// is an exact request-hash lookup against the frozen Fixture bindings; the
/// command and Stdio variants exercise the same evidence/result/leak gates
/// without entering the ordinary W7 dispatch path.
#[derive(Default)]
pub struct HermeticConformanceFixtureHostV1 {
    cases: BTreeMap<FixtureCaseKey, HermeticFixtureCaseV1>,
}

impl HermeticConformanceFixtureHostV1 {
    pub fn new(cases: Vec<HermeticFixtureCaseV1>) -> Result<Self, &'static str> {
        let mut indexed = BTreeMap::new();
        for case in cases {
            let key = FixtureCaseKey {
                tool_key: case.tool_key.clone(),
                case_name: case.case_name.clone(),
            };
            if indexed.insert(key, case).is_some() {
                return Err("duplicate hermetic conformance fixture case");
            }
        }
        Ok(Self { cases: indexed })
    }
}

impl ConformanceRuntimeHostV1 for HermeticConformanceFixtureHostV1 {
    fn execute<'a>(
        &'a self,
        invocation: ConformanceRuntimeInvocationV1<'a>,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<ConformanceRuntimeObservationV1, ConformanceRuntimeErrorV1>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let key = FixtureCaseKey {
                tool_key: invocation.binding.tool_key().clone(),
                case_name: invocation.binding.conformance_case().clone(),
            };
            let case =
                self.cases.get(&key).ok_or(ConformanceRuntimeErrorV1::FixtureRequestMismatch)?;
            let actual_request_hash = conformance_fixture_request_hash_v1(
                invocation.tool,
                invocation.binding.arguments(),
                invocation.binding.instance_configuration(),
                invocation.binding.synthetic_credential_bindings(),
                invocation.binding.fixture_endpoint_bindings(),
            )
            .map_err(|_| ConformanceRuntimeErrorV1::FixtureRequestMismatch)?;
            match &invocation.tool.runtime {
                CanonicalToolPackageRuntimeV1::DeclarativeHttp { request } => {
                    let prepared_request = prepare_hermetic_http_request(&invocation, request)?;
                    let credential_placement = materialize_hermetic_http_credential(
                        &prepared_request,
                        invocation.credentials,
                    )?;
                    let response = invocation
                        .binding
                        .fixture_response_bindings()
                        .iter()
                        .find(|response| response.expected_request_hash == actual_request_hash);
                    if case.expected_request_hash.as_ref() != Some(&actual_request_hash)
                        || response.is_none_or(|response| {
                            Some(response.status) != case.response_status
                                || response.response_body_sha256
                                    != ToolPackageFileSha256V1::sha256(&case.result_body)
                        })
                    {
                        return Err(ConformanceRuntimeErrorV1::FixtureRequestMismatch);
                    }
                    let request_evidence = canonical_json_jcs(&HermeticHttpRequestEvidenceV1 {
                        method: &prepared_request.method,
                        exact_url: &prepared_request.exact_url,
                        canonical_path: &prepared_request.canonical_path,
                        headers: &prepared_request.headers,
                        body_sha256: ToolPackageFileSha256V1::sha256(&prepared_request.body),
                        credential_placement,
                        prepared_request_hash: &prepared_request.request_hash,
                        grant_manifest_hash: &prepared_request.grant_manifest_hash,
                    })
                    .map_err(|_| ConformanceRuntimeErrorV1::FixtureRequestMismatch)?;
                    let mut observed_bytes = vec![ConformanceObservedBytesV1 {
                        channel: "fixture_http_request",
                        bytes: request_evidence,
                    }];
                    append_injected_leak(&mut observed_bytes, case, &invocation);
                    return Ok(observation(case, observed_bytes));
                }
                _ if case.expected_request_hash.is_some() || case.response_status.is_some() => {
                    return Err(ConformanceRuntimeErrorV1::FixtureRequestMismatch);
                }
                _ => {}
            }

            let mut observed_bytes = Vec::new();
            append_injected_leak(&mut observed_bytes, case, &invocation);
            Ok(observation(case, observed_bytes))
        })
    }
}

fn prepare_hermetic_http_request(
    invocation: &ConformanceRuntimeInvocationV1<'_>,
    request: &CanonicalToolPackageDeclarativeHttpV1,
) -> Result<PreparedDeclarativeHttpRequestV1, ConformanceRuntimeErrorV1> {
    let request = aura_tool_contract::DeclarativeHttpV1 {
        target: request.target.clone(),
        method: request.method,
        path: request.path.clone(),
        query: request.query.clone(),
        headers: request.headers.clone(),
        body: request.body.clone(),
        credential_slot: request.credential_slot.0.clone(),
        success_status_ranges: request.success_status_ranges.as_slice().to_vec(),
    };
    let endpoints = invocation
        .binding
        .fixture_endpoint_bindings()
        .iter()
        .map(|binding| {
            (
                binding.endpoint_slot_key.clone(),
                ResolvedToolHttpEndpointV1 {
                    origin: binding.origin.clone(),
                    base_path: binding.base_path.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut pointers = BTreeSet::new();
    collect_http_argument_sources(&request, &mut pointers);
    let authorizations = pointers
        .into_iter()
        .map(|pointer| {
            let value = invocation
                .binding
                .arguments()
                .pointer(pointer.as_str())
                .ok_or(ConformanceRuntimeErrorV1::FixtureRequestMismatch)?;
            Ok((
                pointer,
                HttpArgumentEgressAuthorizationV1 {
                    value_hash: canonical_sha256(value)
                        .map_err(|_| ConformanceRuntimeErrorV1::FixtureRequestMismatch)?,
                    // This synthetic evidence is local to the socket-free W3
                    // Fixture Gateway. Unknown remains fail-closed if it is
                    // ever misrouted into the production P04 network path.
                    data_class: NetworkDataClass::Unknown,
                    source_authorization_hash: invocation.binding.binding_hash().clone(),
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, ConformanceRuntimeErrorV1>>()?;
    let mut meter = OrdinaryToolIoMeterV1::default();
    let prepared = prepare_declarative_http_request_v1(
        &request,
        invocation.binding.arguments(),
        &endpoints,
        &authorizations,
        &mut meter,
    )
    .map_err(|_| ConformanceRuntimeErrorV1::FixtureRequestMismatch)?;
    prepared.validate_integrity().map_err(|_| ConformanceRuntimeErrorV1::FixtureRequestMismatch)?;
    Ok(prepared)
}

fn collect_http_argument_sources(
    request: &aura_tool_contract::DeclarativeHttpV1,
    pointers: &mut BTreeSet<ArgumentJsonPointer>,
) {
    for segment in &request.path {
        if let HttpPathSegmentV1::Argument { source } = segment {
            pointers.insert(source.clone());
        }
    }
    for query in &request.query {
        match &query.value {
            HttpValueSourceV1::Literal { .. } => {}
            HttpValueSourceV1::Scalar { source } | HttpValueSourceV1::Repeated { source } => {
                pointers.insert(source.clone());
            }
        }
    }
    for header in &request.headers {
        if let HttpHeaderValueSourceV1::Scalar { source } = &header.value {
            pointers.insert(source.clone());
        }
    }
    if let aura_tool_contract::DeclarativeHttpBodyV1::ApplicationJson { template } = &request.body {
        collect_http_json_argument_sources(template, pointers);
    }
}

fn collect_http_json_argument_sources(
    template: &HttpJsonTemplateV1,
    pointers: &mut BTreeSet<ArgumentJsonPointer>,
) {
    match template {
        HttpJsonTemplateV1::Literal { .. } => {}
        HttpJsonTemplateV1::Argument { source } => {
            pointers.insert(source.clone());
        }
        HttpJsonTemplateV1::Array { items } => {
            for item in items {
                collect_http_json_argument_sources(item, pointers);
            }
        }
        HttpJsonTemplateV1::Object { properties } => {
            for property in properties.as_slice() {
                collect_http_json_argument_sources(&property.value, pointers);
            }
        }
    }
}

fn materialize_hermetic_http_credential(
    prepared: &PreparedDeclarativeHttpRequestV1,
    credentials: &[aura_tool_package::SyntheticCredentialMaterialV1],
) -> Result<Option<ConformanceSyntheticCredentialPlacementV1>, ConformanceRuntimeErrorV1> {
    match (&prepared.credential_slot, credentials) {
        (None, []) => Ok(None),
        (Some(slot), [credential]) => {
            let (placement, header_name, prefix): (_, &str, &[u8]) = match slot {
                PreparedHttpCredentialSlotV1::BearerAuthorization { slot_key }
                    if credential.binding().slot_key == *slot_key
                        && matches!(
                            credential.binding().placement,
                            ConformanceSyntheticCredentialPlacementV1::HttpBearerAuthorization
                        ) =>
                {
                    (
                        ConformanceSyntheticCredentialPlacementV1::HttpBearerAuthorization,
                        "authorization",
                        b"Bearer ",
                    )
                }
                PreparedHttpCredentialSlotV1::FixedHeader { slot_key, header_name }
                    if credential.binding().slot_key == *slot_key
                        && matches!(
                            &credential.binding().placement,
                            ConformanceSyntheticCredentialPlacementV1::HttpFixedHeader {
                                header_name: expected
                            } if expected.as_str().eq_ignore_ascii_case(header_name)
                        ) =>
                {
                    (credential.binding().placement.clone(), header_name.as_str(), b"")
                }
                _ => return Err(ConformanceRuntimeErrorV1::FixtureRequestMismatch),
            };
            if prepared.headers.iter().any(|(name, _)| name.eq_ignore_ascii_case(header_name)) {
                return Err(ConformanceRuntimeErrorV1::FixtureRequestMismatch);
            }
            let mut materialized = Zeroizing::new(Vec::with_capacity(
                header_name.len() + 2 + prefix.len() + credential.expose_for_injection().len(),
            ));
            materialized.extend_from_slice(header_name.as_bytes());
            materialized.extend_from_slice(b": ");
            materialized.extend_from_slice(prefix);
            materialized.extend_from_slice(credential.expose_for_injection());
            if !materialized.ends_with(credential.expose_for_injection()) {
                return Err(ConformanceRuntimeErrorV1::FixtureRequestMismatch);
            }
            Ok(Some(placement))
        }
        _ => Err(ConformanceRuntimeErrorV1::FixtureRequestMismatch),
    }
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct HermeticHttpRequestEvidenceV1<'a> {
    method: &'a str,
    exact_url: &'a str,
    canonical_path: &'a str,
    headers: &'a [(String, String)],
    body_sha256: ToolPackageFileSha256V1,
    credential_placement: Option<ConformanceSyntheticCredentialPlacementV1>,
    prepared_request_hash: &'a CanonicalHashV1,
    grant_manifest_hash: &'a CanonicalHashV1,
}

fn append_injected_leak(
    observed_bytes: &mut Vec<ConformanceObservedBytesV1>,
    case: &HermeticFixtureCaseV1,
    invocation: &ConformanceRuntimeInvocationV1<'_>,
) {
    if let Some(channel) = case.leak_synthetic_credential_to {
        observed_bytes.push(ConformanceObservedBytesV1 {
            channel,
            bytes: invocation
                .credentials
                .first()
                .map(|credential| credential.expose_for_injection().to_vec())
                .unwrap_or_default(),
        });
    }
}

fn observation(
    case: &HermeticFixtureCaseV1,
    observed_bytes: Vec<ConformanceObservedBytesV1>,
) -> ConformanceRuntimeObservationV1 {
    ConformanceRuntimeObservationV1 {
        terminal: match case.terminal {
            HermeticFixtureTerminalV1::Succeeded => ConformanceRuntimeTerminalV1::Succeeded,
            HermeticFixtureTerminalV1::Failed => ConformanceRuntimeTerminalV1::Failed,
            HermeticFixtureTerminalV1::Cancelled => ConformanceRuntimeTerminalV1::Cancelled,
            HermeticFixtureTerminalV1::Indeterminate => ConformanceRuntimeTerminalV1::Indeterminate,
        },
        result_body: case.result_body.clone(),
        observed_bytes,
        cleanup_verified: case.cleanup_verified,
        protocol_verified: case.protocol_verified,
        artifacts_verified: case.artifacts_verified,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use aura_domain_common::Revision;
    use aura_domain_common::{ToolConformanceRunId, ToolDraftId};
    use aura_tool_contract::{TOOL_PACKAGE_MANIFEST_NAME, ToolPackageFileSha256V1};
    use aura_tool_domain::{
        ConformanceCaseStatusV1, ConformanceDraftRevisionRefV1, ToolPackageConformanceCaseNameV1,
    };
    use aura_tool_package::{
        ConformanceFixtureResponseInputV1, PrepareConformanceRunV1, PreparedConformanceRunV1,
        StagedToolPackageV1, execute_prepared_conformance_run_v1, prepare_conformance_run_v1,
        stage_directory_package_v1,
    };
    use serde_json::{Value, json};

    use super::*;

    fn manifest(runtime: Value, file_path: Option<&str>, file_bytes: &[u8]) -> Value {
        let files = file_path
            .map(|path| {
                vec![json!({
                    "path": path,
                    "byte_length": file_bytes.len(),
                    "sha256": ToolPackageFileSha256V1::sha256(file_bytes),
                    "role": if path.ends_with(".ps1") { "runtime_script" } else { "runtime_executable" }
                })]
            })
            .unwrap_or_default();
        json!({
            "schema_version": 1,
            "package_key": "example.hermetic",
            "package_version": "1.0.0",
            "minimum_aura_version": "0.1.0",
            "ui": { "display_name": "Hermetic", "description": null },
            "files": files,
            "tools": [{
                "tool_key": "fixture",
                "ui": { "display_name": "Fixture" },
                "model": {
                    "canonical_suffix": "fixture",
                    "description": "Exercise one hermetic runtime fixture.",
                    "summary": "Exercise a fixture",
                    "input_schema": {
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    },
                    "strict": true,
                    "result_contract": { "type": "utf8_text" }
                },
                "instance_configuration": null,
                "runtime": runtime,
                "capability_requirements": if file_path.is_some() { json!(["process_execute"]) } else { json!(["public_network_read"]) },
                "resource_claims": [],
                "execution_semantics": {
                    "automatic_retry_allowed": false,
                    "repeat_is_idempotent": true,
                    "may_create_persistent_effects": false
                },
                "conformance_cases": [{ "name": "empty", "arguments": {} }]
            }]
        })
    }

    fn stage(
        runtime: Value,
        file_path: Option<&str>,
        file_bytes: &[u8],
        fixture_response: Option<Vec<u8>>,
    ) -> (tempfile::TempDir, StagedToolPackageV1, PreparedConformanceRunV1) {
        stage_manifest(
            manifest(runtime, file_path, file_bytes),
            file_path,
            file_bytes,
            fixture_response,
        )
    }

    fn stage_manifest(
        manifest: Value,
        file_path: Option<&str>,
        file_bytes: &[u8],
        fixture_response: Option<Vec<u8>>,
    ) -> (tempfile::TempDir, StagedToolPackageV1, PreparedConformanceRunV1) {
        let source = tempfile::tempdir().expect("source");
        if let Some(path) = file_path {
            let target = source.path().join(path.replace('/', "\\"));
            fs::create_dir_all(target.parent().expect("file parent")).expect("create file parent");
            fs::write(target, file_bytes).expect("write runtime");
        }
        fs::write(
            source.path().join(TOOL_PACKAGE_MANIFEST_NAME),
            serde_json::to_vec(&manifest).expect("manifest"),
        )
        .expect("write manifest");
        let staging_parent = source.path().parent().expect("staging parent");
        let staged = stage_directory_package_v1(source.path(), staging_parent).expect("stage");
        let prepared = prepare_conformance_run_v1(
            &staged,
            PrepareConformanceRunV1 {
                tool_conformance_run_id: ToolConformanceRunId::new(),
                draft: ConformanceDraftRevisionRefV1 {
                    tool_draft_id: ToolDraftId::new(),
                    draft_revision: Revision::INITIAL,
                    draft_snapshot_hash: CanonicalSha256HashV1::sha256(b"draft"),
                },
                tool_key: ToolKey::new("fixture").expect("tool key"),
                conformance_case: ToolPackageConformanceCaseNameV1::new("empty").expect("case"),
                instance_configuration: None,
                fixture_endpoint_bindings: Vec::new(),
                fixture_responses: fixture_response
                    .map(|response_body| {
                        vec![ConformanceFixtureResponseInputV1 { status: 200, response_body }]
                    })
                    .unwrap_or_default(),
            },
        )
        .expect("prepare");
        (source, staged, prepared)
    }

    fn fixture_case(prepared: &PreparedConformanceRunV1, body: &[u8]) -> HermeticFixtureCaseV1 {
        let response = prepared.binding().fixture_response_bindings().first();
        HermeticFixtureCaseV1 {
            tool_key: prepared.binding().tool_key().clone(),
            case_name: prepared.binding().conformance_case().clone(),
            expected_request_hash: response.map(|value| value.expected_request_hash.clone()),
            response_status: response.map(|value| value.status),
            result_body: body.to_vec(),
            terminal: HermeticFixtureTerminalV1::Succeeded,
            protocol_verified: true,
            artifacts_verified: true,
            cleanup_verified: true,
            leak_synthetic_credential_to: None,
        }
    }

    #[tokio::test]
    async fn all_three_runtime_variants_use_the_same_fail_closed_gate() {
        let command = json!({
            "type": "declarative_command",
            "command": {
                "type": "argv",
                "executable": "bin/fixture.exe",
                "arguments": [],
                "working_directory": { "type": "package_root" },
                "environment": [],
                "credential_slot": null
            }
        });
        let (_source, staged, prepared) = stage(command, Some("bin/fixture.exe"), b"command", None);
        let host = HermeticConformanceFixtureHostV1::new(vec![fixture_case(&prepared, b"ok")])
            .expect("host");
        assert_eq!(
            execute_prepared_conformance_run_v1(&prepared, &staged, &host)
                .await
                .expect("command")
                .status,
            ConformanceCaseStatusV1::Passed
        );

        let stdio = json!({
            "type": "aura_stdio",
            "protocol_version": 1,
            "executable": "bin/fixture.exe",
            "arguments": [],
            "working_directory": { "type": "package_root" },
            "environment": [],
            "credential_slot": null
        });
        let (_source, staged, prepared) = stage(stdio, Some("bin/fixture.exe"), b"stdio", None);
        let host = HermeticConformanceFixtureHostV1::new(vec![fixture_case(&prepared, b"ok")])
            .expect("host");
        assert_eq!(
            execute_prepared_conformance_run_v1(&prepared, &staged, &host)
                .await
                .expect("stdio")
                .status,
            ConformanceCaseStatusV1::Passed
        );

        let http = json!({
            "type": "declarative_http",
            "request": {
                "target": { "type": "fixed_https", "origin": "https://fixture.invalid", "base_path": "/v1" },
                "method": "POST",
                "path": [{ "type": "literal", "value": "echo" }],
                "query": [],
                "headers": [],
                "body": { "type": "none" },
                "credential_slot": null
            }
        });
        let (_source, staged, prepared) = stage(http, None, b"", Some(b"ok".to_vec()));
        let host = HermeticConformanceFixtureHostV1::new(vec![fixture_case(&prepared, b"ok")])
            .expect("host");
        assert_eq!(
            execute_prepared_conformance_run_v1(&prepared, &staged, &host)
                .await
                .expect("HTTP")
                .status,
            ConformanceCaseStatusV1::Passed
        );
    }

    #[tokio::test]
    async fn http_fixture_uses_the_production_p1a_r1_builder_and_ephemeral_credential_slot() {
        let http = json!({
            "type": "declarative_http",
            "request": {
                "target": { "type": "fixed_https", "origin": "https://fixture.invalid", "base_path": "/v1/" },
                "method": "POST",
                "path": [
                    { "type": "literal", "value": "items" },
                    { "type": "argument", "source": "/resource" }
                ],
                "query": [{ "name": "tag", "value": { "type": "repeated", "source": "/tags" } }],
                "headers": [{ "name": "x-label", "value": { "type": "scalar", "source": "/label" } }],
                "body": {
                    "type": "application_json",
                    "template": {
                        "type": "object",
                        "properties": [
                            { "name": "payload", "value": { "type": "argument", "source": "/payload" } }
                        ]
                    }
                },
                "credential_slot": { "type": "bearer_authorization", "slot_key": "api_key" }
            }
        });
        let mut manifest = manifest(http, None, b"");
        manifest["tools"][0]["model"]["input_schema"] = json!({
            "type": "object",
            "properties": {
                "resource": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "label": { "type": "string" },
                "payload": { "type": "object" }
            },
            "required": ["resource", "tags", "label", "payload"],
            "additionalProperties": false
        });
        manifest["tools"][0]["conformance_cases"][0]["arguments"] = json!({
            "resource": "group%2Fproject",
            "tags": ["one two", "three"],
            "label": "fixture label",
            "payload": { "z": 2, "a": 1 }
        });
        let (_source, staged, prepared) = stage_manifest(manifest, None, b"", Some(b"ok".to_vec()));
        let CanonicalToolPackageRuntimeV1::DeclarativeHttp { request } = &prepared.tool().runtime
        else {
            panic!("HTTP runtime")
        };
        let invocation = ConformanceRuntimeInvocationV1 {
            staged_package: &staged,
            binding: prepared.binding(),
            tool: prepared.tool(),
            credentials: prepared.credentials(),
        };
        let exact = prepare_hermetic_http_request(&invocation, request).expect("exact request");
        assert_eq!(exact.method, "POST");
        assert_eq!(exact.canonical_path, "/v1/items/group%252Fproject");
        assert_eq!(
            exact.exact_url,
            "https://fixture.invalid/v1/items/group%252Fproject?tag=one%20two&tag=three"
        );
        assert_eq!(
            exact.headers,
            vec![
                ("x-label".to_owned(), "fixture label".to_owned()),
                ("content-type".to_owned(), "application/json".to_owned())
            ]
        );
        assert_eq!(exact.body, br#"{"payload":{"a":1,"z":2}}"#);
        assert_eq!(exact.argument_evidence.len(), 4);
        assert_eq!(
            materialize_hermetic_http_credential(&exact, prepared.credentials())
                .expect("synthetic credential"),
            Some(ConformanceSyntheticCredentialPlacementV1::HttpBearerAuthorization)
        );

        let host = HermeticConformanceFixtureHostV1::new(vec![fixture_case(&prepared, b"ok")])
            .expect("host");
        assert_eq!(
            execute_prepared_conformance_run_v1(&prepared, &staged, &host)
                .await
                .expect("HTTP conformance")
                .status,
            ConformanceCaseStatusV1::Passed
        );
    }

    #[tokio::test]
    async fn fixture_mismatch_and_protocol_conflict_fail_closed() {
        let http = json!({
            "type": "declarative_http",
            "request": {
                "target": { "type": "fixed_https", "origin": "https://fixture.invalid", "base_path": "/" },
                "method": "GET",
                "path": [],
                "query": [],
                "headers": [],
                "body": { "type": "none" },
                "credential_slot": null
            }
        });
        let (_source, staged, prepared) = stage(http, None, b"", Some(b"ok".to_vec()));
        let mut case = fixture_case(&prepared, b"different");
        case.protocol_verified = false;
        let host = HermeticConformanceFixtureHostV1::new(vec![case]).expect("host");
        assert!(execute_prepared_conformance_run_v1(&prepared, &staged, &host).await.is_err());
    }

    #[tokio::test]
    async fn cancellation_protocol_failure_and_uncertain_terminal_never_pass() {
        let command = json!({
            "type": "declarative_command",
            "command": {
                "type": "argv",
                "executable": "bin/fixture.exe",
                "arguments": [],
                "working_directory": { "type": "package_root" },
                "environment": [],
                "credential_slot": null
            }
        });
        let (_source, staged, prepared) = stage(command, Some("bin/fixture.exe"), b"command", None);

        let mut cancelled = fixture_case(&prepared, b"");
        cancelled.terminal = HermeticFixtureTerminalV1::Cancelled;
        let host = HermeticConformanceFixtureHostV1::new(vec![cancelled]).expect("host");
        assert_eq!(
            execute_prepared_conformance_run_v1(&prepared, &staged, &host)
                .await
                .expect("cancelled")
                .status,
            ConformanceCaseStatusV1::Cancelled
        );

        let mut protocol_failure = fixture_case(&prepared, b"ok");
        protocol_failure.protocol_verified = false;
        let host = HermeticConformanceFixtureHostV1::new(vec![protocol_failure]).expect("host");
        assert_eq!(
            execute_prepared_conformance_run_v1(&prepared, &staged, &host)
                .await
                .expect("protocol failure")
                .status,
            ConformanceCaseStatusV1::Failed
        );

        let mut indeterminate = fixture_case(&prepared, b"ok");
        indeterminate.terminal = HermeticFixtureTerminalV1::Indeterminate;
        let host = HermeticConformanceFixtureHostV1::new(vec![indeterminate]).expect("host");
        assert_eq!(
            execute_prepared_conformance_run_v1(&prepared, &staged, &host)
                .await
                .expect("indeterminate")
                .status,
            ConformanceCaseStatusV1::RecoveryRequired
        );
    }
}
