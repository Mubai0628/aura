//! One request builder for package HTTP tools, used by Conformance and by the Agent.
use anyhow::{Context, Result, ensure};
use aura_core_broker::package_credentials::declarative;
use aura_domain_common::{NetworkRequestId, jcs::canonical_sha256};
use aura_network_control::{GatewayRequest, SecretScanner};
use aura_permission_domain::{NetworkByteLimits, NetworkRequestPlan, RedirectPolicy};
use aura_security_contract::{InvocationIdentity, NetworkCallerKind, ResourceDescriptor};
use aura_tool_contract::{
    ArgumentJsonPointer, CompiledToolSchemaV1, DeclarativeHttpBodyV1, DeclarativeHttpV1,
    HttpHeaderValueSourceV1, HttpJsonTemplateV1, HttpPathSegmentV1, HttpValueSourceV1,
    MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES, MAX_HTTP_RESPONSE_WIRE_BYTES,
};
use aura_tool_domain::{CanonicalToolPackageToolV1, PackageToolSettingsV1};
use aura_tool_runtime::{
    HttpArgumentEgressAuthorizationV1, OrdinaryToolIoMeterV1, PreparedDeclarativeHttpRequestV1,
    PreparedHttpCredentialSlotV1, ResolvedToolHttpEndpointV1, prepare_declarative_http_request_v1,
};
use serde_json::Value;
use std::collections::BTreeMap;

/// The Core-built request for one call or one Conformance case. Nothing here comes
/// from the model except the validated arguments the manifest maps.
pub struct PackageHttpRequest {
    pub gateway: GatewayRequest,
    pub prepared: PreparedDeclarativeHttpRequestV1,
    /// The exact origin the package declared or the user supplied.
    pub origin: ResourceDescriptor,
}

impl PackageHttpRequest {
    #[must_use]
    pub fn credential_slot(&self) -> Option<&PreparedHttpCredentialSlotV1> {
        self.prepared.credential_slot.as_ref()
    }
}

/// Validates arguments against the tool's own input schema, maps them with the
/// manifest's declarative mapping and returns the exact gateway request.
pub fn build(
    tool: &CanonicalToolPackageToolV1,
    arguments: &Value,
    settings: Option<&PackageToolSettingsV1>,
    identity: InvocationIdentity,
) -> Result<PackageHttpRequest> {
    let declared = declarative(tool)?;
    let validator = CompiledToolSchemaV1::compile(tool.model.input_schema.canonical_document())?;
    ensure!(validator.validate(arguments).valid, "package.arguments_invalid");
    let endpoints = settings
        .map(|entry| {
            entry
                .endpoints
                .iter()
                .map(|endpoint| {
                    (
                        endpoint.endpoint_slot_key.clone(),
                        ResolvedToolHttpEndpointV1 {
                            origin: endpoint.origin.clone(),
                            base_path: endpoint.base_path.clone(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    // Argument egress is authorized by the operation the user's mode or approval
    // allowed, not per value. Each mapped value is still bound by its hash into the
    // request hash, so a later change of any argument is a different request.
    let authorizations = argument_sources(&declared)
        .into_iter()
        .filter_map(|source| {
            aura_tool_runtime::optional_pointer(arguments, &source).map(|value| (source, value))
        })
        .map(|(source, value)| {
            Ok((
                source,
                HttpArgumentEgressAuthorizationV1 {
                    value_hash: canonical_sha256(value)?,
                    data_class: aura_security_contract::NetworkDataClass::PublicQuery,
                    source_authorization_hash: canonical_sha256(&"aura.package-http.operation")?,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let prepared = prepare_declarative_http_request_v1(
        &declared,
        arguments,
        &endpoints,
        &authorizations,
        &mut OrdinaryToolIoMeterV1::default(),
    )
    .context("package.http_request_invalid")?;
    prepared.validate_integrity().context("package.http_request_invalid")?;
    let url = url::Url::parse(&prepared.exact_url)?;
    let origin = aura_core_base::web_origin::resource(&url)?;
    let gateway = GatewayRequest {
        plan: NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            identity,
            caller: NetworkCallerKind::AuraTool,
            method: prepared.method.clone(),
            requested_url: prepared.exact_url.clone(),
            destination: None,
            request_data: vec![],
            data_classes: vec![],
            // A package declares one exact origin; following a redirect would leave it.
            redirect_policy: RedirectPolicy::Deny,
            byte_limits: NetworkByteLimits {
                max_upload_bytes: prepared.body.len() as u64,
                max_download_bytes: MAX_HTTP_RESPONSE_WIRE_BYTES as u64,
                max_decompressed_bytes: MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES as u64,
            },
            timeout_ms: 30_000,
            credential_refs: vec![],
            network_grant_ids: vec![],
        },
        headers: prepared.headers.clone(),
        body: prepared.body.clone(),
    };
    // Before trusted credential injection: a secret typed into an argument or a
    // literal in the manifest is refused, exactly as for the built-in HTTP tool.
    SecretScanner.scan_http_request(&gateway, true)?;
    SecretScanner.scan_plan(&gateway.plan)?;
    Ok(PackageHttpRequest { gateway, prepared, origin })
}

fn argument_sources(request: &DeclarativeHttpV1) -> Vec<ArgumentJsonPointer> {
    let mut sources = Vec::new();
    for segment in &request.path {
        if let HttpPathSegmentV1::Argument { source } = segment {
            sources.push(source.clone());
        }
    }
    for entry in &request.query {
        match &entry.value {
            HttpValueSourceV1::Literal { .. } => {}
            HttpValueSourceV1::Scalar { source } | HttpValueSourceV1::Repeated { source } => {
                sources.push(source.clone());
            }
        }
    }
    for entry in &request.headers {
        if let HttpHeaderValueSourceV1::Scalar { source } = &entry.value {
            sources.push(source.clone());
        }
    }
    if let DeclarativeHttpBodyV1::ApplicationJson { template } = &request.body {
        template_sources(template, &mut sources);
    }
    sources.sort();
    sources.dedup();
    sources
}

fn template_sources(template: &HttpJsonTemplateV1, sources: &mut Vec<ArgumentJsonPointer>) {
    match template {
        HttpJsonTemplateV1::Literal { .. } => {}
        HttpJsonTemplateV1::Argument { source } => sources.push(source.clone()),
        HttpJsonTemplateV1::Array { items } => {
            for item in items {
                template_sources(item, sources);
            }
        }
        HttpJsonTemplateV1::Object { properties } => {
            for property in properties.as_slice() {
                template_sources(&property.value, sources);
            }
        }
    }
}
