use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::jcs::{CanonicalHashV1, canonical_json_jcs, canonical_sha256};
use aura_permission_domain::ResolvedScope;
use aura_security_contract::NetworkDataClass;
use aura_tool_contract::{
    ArgumentJsonPointer, CredentialSlotKey, DeclarativeHttpBodyV1, DeclarativeHttpMethodV1,
    DeclarativeHttpTargetV1, DeclarativeHttpV1, FixedHttpsOriginV1, HttpBasePathV1,
    HttpCredentialSlotV1, HttpEndpointSlotKeyV1, HttpHeaderLiteralValueV1, HttpHeaderNameV1,
    HttpPathArgumentSegmentV1, HttpPathSegmentV1, MAX_HTTP_REQUEST_BODY_BYTES,
};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use url::Url;

use crate::{
    OrdinaryToolIoChannelV1, OrdinaryToolIoLimitErrorV1, OrdinaryToolIoMeterV1,
    ToolArgumentResolutionErrorV1, optional_pointer, render_http_header_source_v1,
    render_http_json_template_v1, render_http_value_source_v1,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedToolHttpEndpointV1 {
    pub origin: FixedHttpsOriginV1,
    pub base_path: HttpBasePathV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpArgumentEgressAuthorizationV1 {
    pub value_hash: CanonicalHashV1,
    pub data_class: NetworkDataClass,
    pub source_authorization_hash: CanonicalHashV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedHttpArgumentEvidenceV1 {
    pub location: String,
    pub source: ArgumentJsonPointer,
    pub value_hash: CanonicalHashV1,
    pub data_class: NetworkDataClass,
    pub source_authorization_hash: CanonicalHashV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedHttpCredentialSlotV1 {
    BearerAuthorization { slot_key: CredentialSlotKey },
    FixedHeader { slot_key: CredentialSlotKey, header_name: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedDeclarativeHttpRequestV1 {
    pub method: String,
    pub exact_url: String,
    pub canonical_path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub credential_slot: Option<PreparedHttpCredentialSlotV1>,
    pub success_status_ranges: Vec<(u16, u16)>,
    pub argument_evidence: Vec<PreparedHttpArgumentEvidenceV1>,
    pub request_hash: CanonicalHashV1,
    pub grant_manifest_hash: CanonicalHashV1,
    network_origin_scope: ResolvedScope,
}

#[derive(Debug, Error)]
pub enum PreparedHttpRequestErrorV1 {
    #[error("the declarative HTTP contract is invalid")]
    ContractInvalid,
    #[error("the user-bound Tool endpoint is unavailable")]
    EndpointUnavailable,
    #[error("an HTTP argument mapping is invalid")]
    Argument(#[from] ToolArgumentResolutionErrorV1),
    #[error("an argument-derived HTTP value lacks exact P04 egress authorization")]
    EgressAuthorizationMissing,
    #[error("an argument-derived HTTP value differs from its P04 egress authorization")]
    EgressAuthorizationMismatch,
    #[error("the request exceeds OrdinaryToolIoPolicyV1")]
    IoLimit(#[from] OrdinaryToolIoLimitErrorV1),
    #[error("the request hash cannot be represented as canonical JSON")]
    CanonicalJson,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedHttpRequestHashMaterialV1<'a> {
    kind: &'static str,
    preimage_version: u32,
    method: &'a str,
    exact_url: &'a str,
    canonical_path: &'a str,
    headers: &'a [(String, String)],
    body_hash: CanonicalHashV1,
    credential_slot: &'a Option<PreparedHttpCredentialSlotHashMaterialV1<'a>>,
    argument_evidence: &'a [PreparedHttpArgumentEvidenceHashMaterialV1<'a>],
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum PreparedHttpCredentialSlotHashMaterialV1<'a> {
    BearerAuthorization { slot_key: &'a CredentialSlotKey },
    FixedHeader { slot_key: &'a CredentialSlotKey, header_name: &'a str },
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedHttpArgumentEvidenceHashMaterialV1<'a> {
    location: &'a str,
    source: &'a ArgumentJsonPointer,
    value_hash: &'a CanonicalHashV1,
    data_class: NetworkDataClass,
    source_authorization_hash: &'a CanonicalHashV1,
}

pub fn prepare_declarative_http_request_v1(
    request: &DeclarativeHttpV1,
    arguments: &Value,
    endpoints: &BTreeMap<HttpEndpointSlotKeyV1, ResolvedToolHttpEndpointV1>,
    argument_authorizations: &BTreeMap<ArgumentJsonPointer, HttpArgumentEgressAuthorizationV1>,
    meter: &mut OrdinaryToolIoMeterV1,
) -> Result<PreparedDeclarativeHttpRequestV1, PreparedHttpRequestErrorV1> {
    request.validate().map_err(|_| PreparedHttpRequestErrorV1::ContractInvalid)?;
    let (origin, base_path) = match &request.target {
        DeclarativeHttpTargetV1::FixedHttps { origin, base_path } => {
            (origin.clone(), base_path.clone())
        }
        DeclarativeHttpTargetV1::UserBoundEndpoint { endpoint_slot_key } => {
            let endpoint = endpoints
                .get(endpoint_slot_key)
                .ok_or(PreparedHttpRequestErrorV1::EndpointUnavailable)?;
            (endpoint.origin.clone(), endpoint.base_path.clone())
        }
    };
    let network_origin_scope = resolved_network_origin_scope(&origin)?;

    let mut evidence = Vec::new();
    let mut encoded_segments = Vec::with_capacity(request.path.len());
    for (index, segment) in request.path.iter().enumerate() {
        match segment {
            HttpPathSegmentV1::Literal { value } => {
                encoded_segments.push(value.percent_encoded());
            }
            HttpPathSegmentV1::Argument { source } => {
                let value = optional_pointer(arguments, source)
                    .ok_or(ToolArgumentResolutionErrorV1::Missing)?;
                let Value::String(value) = value else {
                    return Err(PreparedHttpRequestErrorV1::Argument(
                        ToolArgumentResolutionErrorV1::ScalarRequired,
                    ));
                };
                let segment = HttpPathArgumentSegmentV1::new(value.clone())
                    .map_err(|_| PreparedHttpRequestErrorV1::ContractInvalid)?;
                encoded_segments.push(segment.percent_encoded());
                evidence.push(verify_argument_authorization(
                    format!("path[{index}]"),
                    source,
                    arguments,
                    argument_authorizations,
                )?);
            }
        }
    }
    let canonical_path = append_path_segments(&base_path, &encoded_segments);

    let mut query_pairs = Vec::new();
    for (index, entry) in request.query.iter().enumerate() {
        let values = render_http_value_source_v1(arguments, &entry.value)?;
        for value in values {
            query_pairs.push((entry.name.as_str().to_owned(), value));
        }
        match &entry.value {
            aura_tool_contract::HttpValueSourceV1::Literal { .. } => {}
            aura_tool_contract::HttpValueSourceV1::Scalar { source }
            | aura_tool_contract::HttpValueSourceV1::Repeated { source } => {
                evidence.push(verify_argument_authorization(
                    format!("query[{index}]"),
                    source,
                    arguments,
                    argument_authorizations,
                )?);
            }
        }
    }
    let query = query_pairs
        .iter()
        .map(|(name, value)| {
            format!("{}={}", percent_encode_component(name), percent_encode_component(value))
        })
        .collect::<Vec<_>>()
        .join("&");

    let mut headers = Vec::with_capacity(request.headers.len() + 1);
    for (index, entry) in request.headers.iter().enumerate() {
        headers.push((
            entry.name.as_str().to_owned(),
            render_http_header_source_v1(arguments, &entry.value)?,
        ));
        if let aura_tool_contract::HttpHeaderValueSourceV1::Scalar { source } = &entry.value {
            evidence.push(verify_argument_authorization(
                format!("header[{index}]"),
                source,
                arguments,
                argument_authorizations,
            )?);
        }
    }

    let body = match &request.body {
        DeclarativeHttpBodyV1::None => Vec::new(),
        DeclarativeHttpBodyV1::ApplicationJson { template } => {
            collect_json_template_evidence(
                template,
                "body",
                arguments,
                argument_authorizations,
                &mut evidence,
            )?;
            if headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("content-type")) {
                return Err(PreparedHttpRequestErrorV1::ContractInvalid);
            }
            headers.push(("content-type".to_owned(), "application/json".to_owned()));
            canonical_json_jcs(&render_http_json_template_v1(arguments, template)?)
                .map_err(|_| PreparedHttpRequestErrorV1::CanonicalJson)?
        }
    };
    meter.charge(
        OrdinaryToolIoChannelV1::HttpRequestBody,
        u64::try_from(body.len()).unwrap_or(u64::MAX),
    )?;

    let exact_url = if query.is_empty() {
        format!("{}{}", origin.as_str(), canonical_path)
    } else {
        format!("{}{}?{}", origin.as_str(), canonical_path, query)
    };
    let credential_slot = request.credential_slot.as_ref().map(|slot| match slot {
        HttpCredentialSlotV1::BearerAuthorization { slot_key } => {
            PreparedHttpCredentialSlotV1::BearerAuthorization { slot_key: slot_key.clone() }
        }
        HttpCredentialSlotV1::FixedHeader { slot_key, header_name } => {
            PreparedHttpCredentialSlotV1::FixedHeader {
                slot_key: slot_key.clone(),
                header_name: header_name.as_str().to_owned(),
            }
        }
    });
    let credential_hash_material = credential_slot.as_ref().map(|slot| match slot {
        PreparedHttpCredentialSlotV1::BearerAuthorization { slot_key } => {
            PreparedHttpCredentialSlotHashMaterialV1::BearerAuthorization { slot_key }
        }
        PreparedHttpCredentialSlotV1::FixedHeader { slot_key, header_name } => {
            PreparedHttpCredentialSlotHashMaterialV1::FixedHeader { slot_key, header_name }
        }
    });
    let evidence_hash_material = evidence
        .iter()
        .map(|item| PreparedHttpArgumentEvidenceHashMaterialV1 {
            location: &item.location,
            source: &item.source,
            value_hash: &item.value_hash,
            data_class: item.data_class,
            source_authorization_hash: &item.source_authorization_hash,
        })
        .collect::<Vec<_>>();
    let method = method_wire(request.method).to_owned();
    let body_hash = CanonicalHashV1::sha256(&body);
    let request_hash = canonical_sha256(&PreparedHttpRequestHashMaterialV1 {
        kind: "aura.p07.declarative_http_request",
        preimage_version: 1,
        method: &method,
        exact_url: &exact_url,
        canonical_path: &canonical_path,
        headers: &headers,
        body_hash: body_hash.clone(),
        credential_slot: &credential_hash_material,
        argument_evidence: &evidence_hash_material,
    })
    .map_err(|_| PreparedHttpRequestErrorV1::CanonicalJson)?;
    let grant_manifest_hash = canonical_sha256(&PreparedHttpRequestHashMaterialV1 {
        kind: "aura.p07.declarative_http_grant_manifest",
        preimage_version: 1,
        method: &method,
        exact_url: &exact_url,
        canonical_path: &canonical_path,
        headers: &headers,
        body_hash,
        credential_slot: &credential_hash_material,
        argument_evidence: &evidence_hash_material,
    })
    .map_err(|_| PreparedHttpRequestErrorV1::CanonicalJson)?;

    Ok(PreparedDeclarativeHttpRequestV1 {
        method,
        exact_url,
        canonical_path,
        headers,
        body,
        credential_slot,
        success_status_ranges: request
            .success_status_ranges
            .iter()
            .map(|range| (range.start, range.end))
            .collect(),
        argument_evidence: evidence,
        request_hash,
        grant_manifest_hash,
        network_origin_scope,
    })
}

impl PreparedDeclarativeHttpRequestV1 {
    /// Revalidates the P1a/R1 builder output before Core uses it at a P04 or
    /// Gateway boundary. Stored hashes are compared with independently
    /// recomputed values and are never treated as authority by themselves.
    pub fn validate_integrity(&self) -> Result<(), PreparedHttpRequestErrorV1> {
        if !matches!(self.method.as_str(), "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE")
            || self.body.len() > MAX_HTTP_REQUEST_BODY_BYTES
            || self.success_status_ranges.is_empty()
            || self.success_status_ranges.len() > 16
            || self
                .success_status_ranges
                .iter()
                .any(|(start, end)| start > end || *start < 100 || *end > 599)
            || self.success_status_ranges.windows(2).any(|pair| pair[0].1 >= pair[1].0)
        {
            return Err(PreparedHttpRequestErrorV1::ContractInvalid);
        }

        let parsed =
            Url::parse(&self.exact_url).map_err(|_| PreparedHttpRequestErrorV1::ContractInvalid)?;
        let host = parsed.host_str().ok_or(PreparedHttpRequestErrorV1::ContractInvalid)?;
        let port =
            parsed.port_or_known_default().ok_or(PreparedHttpRequestErrorV1::ContractInvalid)?;
        if parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != self.canonical_path
            || self.network_origin_scope
                != (ResolvedScope::NetworkOrigin {
                    scheme: "https".to_owned(),
                    host: host.to_owned(),
                    port,
                })
        {
            return Err(PreparedHttpRequestErrorV1::ContractInvalid);
        }

        let credential_header = self.credential_slot.as_ref().map(|slot| match slot {
            PreparedHttpCredentialSlotV1::BearerAuthorization { .. } => "authorization",
            PreparedHttpCredentialSlotV1::FixedHeader { header_name, .. } => header_name.as_str(),
        });
        let mut header_names = BTreeSet::new();
        for (name, value) in &self.headers {
            HttpHeaderNameV1::new(name.clone())
                .map_err(|_| PreparedHttpRequestErrorV1::ContractInvalid)?;
            HttpHeaderLiteralValueV1::new(value.clone())
                .map_err(|_| PreparedHttpRequestErrorV1::ContractInvalid)?;
            if !header_names.insert(name.to_ascii_lowercase())
                || credential_header.is_some_and(|credential| name.eq_ignore_ascii_case(credential))
            {
                return Err(PreparedHttpRequestErrorV1::ContractInvalid);
            }
        }
        let mut evidence_locations = BTreeSet::new();
        if self.argument_evidence.iter().any(|item| {
            item.location.is_empty() || !evidence_locations.insert(item.location.as_str())
        }) {
            return Err(PreparedHttpRequestErrorV1::ContractInvalid);
        }

        let credential_hash_material = self.credential_slot.as_ref().map(|slot| match slot {
            PreparedHttpCredentialSlotV1::BearerAuthorization { slot_key } => {
                PreparedHttpCredentialSlotHashMaterialV1::BearerAuthorization { slot_key }
            }
            PreparedHttpCredentialSlotV1::FixedHeader { slot_key, header_name } => {
                PreparedHttpCredentialSlotHashMaterialV1::FixedHeader { slot_key, header_name }
            }
        });
        let evidence_hash_material = self
            .argument_evidence
            .iter()
            .map(|item| PreparedHttpArgumentEvidenceHashMaterialV1 {
                location: &item.location,
                source: &item.source,
                value_hash: &item.value_hash,
                data_class: item.data_class,
                source_authorization_hash: &item.source_authorization_hash,
            })
            .collect::<Vec<_>>();
        let body_hash = CanonicalHashV1::sha256(&self.body);
        let request_hash = canonical_sha256(&PreparedHttpRequestHashMaterialV1 {
            kind: "aura.p07.declarative_http_request",
            preimage_version: 1,
            method: &self.method,
            exact_url: &self.exact_url,
            canonical_path: &self.canonical_path,
            headers: &self.headers,
            body_hash: body_hash.clone(),
            credential_slot: &credential_hash_material,
            argument_evidence: &evidence_hash_material,
        })
        .map_err(|_| PreparedHttpRequestErrorV1::CanonicalJson)?;
        let grant_manifest_hash = canonical_sha256(&PreparedHttpRequestHashMaterialV1 {
            kind: "aura.p07.declarative_http_grant_manifest",
            preimage_version: 1,
            method: &self.method,
            exact_url: &self.exact_url,
            canonical_path: &self.canonical_path,
            headers: &self.headers,
            body_hash,
            credential_slot: &credential_hash_material,
            argument_evidence: &evidence_hash_material,
        })
        .map_err(|_| PreparedHttpRequestErrorV1::CanonicalJson)?;
        if self.request_hash != request_hash || self.grant_manifest_hash != grant_manifest_hash {
            return Err(PreparedHttpRequestErrorV1::ContractInvalid);
        }
        Ok(())
    }

    #[must_use]
    pub fn status_is_success(&self, status: u16) -> bool {
        self.success_status_ranges.iter().any(|(start, end)| (*start..=*end).contains(&status))
    }

    /// Returns the exact P04 network resource derived from the same canonical
    /// Origin that produced `exact_url`. Private-network classification, when
    /// applicable, remains the responsibility of the authoritative network
    /// resolver and is not guessed while preparing the request.
    #[must_use]
    pub const fn network_origin_scope(&self) -> &ResolvedScope {
        &self.network_origin_scope
    }
}

fn resolved_network_origin_scope(
    origin: &FixedHttpsOriginV1,
) -> Result<ResolvedScope, PreparedHttpRequestErrorV1> {
    let parsed =
        Url::parse(origin.as_str()).map_err(|_| PreparedHttpRequestErrorV1::ContractInvalid)?;
    let host = parsed.host_str().ok_or(PreparedHttpRequestErrorV1::ContractInvalid)?.to_owned();
    let port = parsed.port_or_known_default().ok_or(PreparedHttpRequestErrorV1::ContractInvalid)?;
    Ok(ResolvedScope::NetworkOrigin { scheme: parsed.scheme().to_owned(), host, port })
}

fn verify_argument_authorization(
    location: String,
    source: &ArgumentJsonPointer,
    arguments: &Value,
    authorizations: &BTreeMap<ArgumentJsonPointer, HttpArgumentEgressAuthorizationV1>,
) -> Result<PreparedHttpArgumentEvidenceV1, PreparedHttpRequestErrorV1> {
    let value =
        optional_pointer(arguments, source).ok_or(ToolArgumentResolutionErrorV1::Missing)?;
    let actual_hash =
        canonical_sha256(value).map_err(|_| PreparedHttpRequestErrorV1::CanonicalJson)?;
    let authorization =
        authorizations.get(source).ok_or(PreparedHttpRequestErrorV1::EgressAuthorizationMissing)?;
    if actual_hash != authorization.value_hash {
        return Err(PreparedHttpRequestErrorV1::EgressAuthorizationMismatch);
    }
    Ok(PreparedHttpArgumentEvidenceV1 {
        location,
        source: source.clone(),
        value_hash: actual_hash,
        data_class: authorization.data_class,
        source_authorization_hash: authorization.source_authorization_hash.clone(),
    })
}

fn collect_json_template_evidence(
    template: &aura_tool_contract::HttpJsonTemplateV1,
    location: &str,
    arguments: &Value,
    authorizations: &BTreeMap<ArgumentJsonPointer, HttpArgumentEgressAuthorizationV1>,
    evidence: &mut Vec<PreparedHttpArgumentEvidenceV1>,
) -> Result<(), PreparedHttpRequestErrorV1> {
    match template {
        aura_tool_contract::HttpJsonTemplateV1::Literal { .. } => {}
        aura_tool_contract::HttpJsonTemplateV1::Argument { source } => {
            evidence.push(verify_argument_authorization(
                location.to_owned(),
                source,
                arguments,
                authorizations,
            )?);
        }
        aura_tool_contract::HttpJsonTemplateV1::Array { items } => {
            for (index, item) in items.iter().enumerate() {
                collect_json_template_evidence(
                    item,
                    &format!("{location}[{index}]"),
                    arguments,
                    authorizations,
                    evidence,
                )?;
            }
        }
        aura_tool_contract::HttpJsonTemplateV1::Object { properties } => {
            for property in properties.as_slice() {
                collect_json_template_evidence(
                    &property.value,
                    &format!("{location}.{}", property.name.as_str()),
                    arguments,
                    authorizations,
                    evidence,
                )?;
            }
        }
    }
    Ok(())
}

fn append_path_segments(base: &HttpBasePathV1, segments: &[String]) -> String {
    if segments.is_empty() {
        return base.as_str().to_owned();
    }
    let mut path = base.as_str().to_owned();
    if !path.ends_with('/') {
        path.push('/');
    }
    path.push_str(&segments.join("/"));
    path
}

fn percent_encode_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

const fn method_wire(method: DeclarativeHttpMethodV1) -> &'static str {
    match method {
        DeclarativeHttpMethodV1::Get => "GET",
        DeclarativeHttpMethodV1::Head => "HEAD",
        DeclarativeHttpMethodV1::Post => "POST",
        DeclarativeHttpMethodV1::Put => "PUT",
        DeclarativeHttpMethodV1::Patch => "PATCH",
        DeclarativeHttpMethodV1::Delete => "DELETE",
    }
}

#[cfg(test)]
mod tests {
    use aura_tool_contract::{
        DeclarativeHttpBodyV1, DeclarativeHttpMethodV1, DeclarativeHttpTargetV1, HttpHeaderEntryV1,
        HttpHeaderNameV1, HttpHeaderValueSourceV1, HttpJsonPropertiesV1, HttpJsonPropertyNameV1,
        HttpJsonPropertyV1, HttpJsonTemplateV1, HttpPathLiteralSegmentV1, HttpQueryEntryV1,
        HttpQueryNameV1, HttpStatusRangeV1, HttpValueSourceV1,
    };
    use serde_json::json;

    use super::*;

    fn authorization(value: &Value) -> HttpArgumentEgressAuthorizationV1 {
        HttpArgumentEgressAuthorizationV1 {
            value_hash: canonical_sha256(value).expect("hash"),
            data_class: NetworkDataClass::PublicQuery,
            source_authorization_hash: CanonicalHashV1::sha256(b"authorized"),
        }
    }

    fn request(base_path: &str) -> DeclarativeHttpV1 {
        DeclarativeHttpV1 {
            target: DeclarativeHttpTargetV1::FixedHttps {
                origin: FixedHttpsOriginV1::new("https://example.com").expect("origin"),
                base_path: HttpBasePathV1::new(base_path).expect("base path"),
            },
            method: DeclarativeHttpMethodV1::Post,
            path: vec![
                HttpPathSegmentV1::Literal {
                    value: HttpPathLiteralSegmentV1::new("items").expect("literal"),
                },
                HttpPathSegmentV1::Argument { source: "/id".parse().expect("pointer") },
            ],
            query: vec![HttpQueryEntryV1 {
                name: HttpQueryNameV1::new("tag").expect("query name"),
                value: HttpValueSourceV1::Repeated { source: "/tags".parse().expect("pointer") },
            }],
            headers: vec![HttpHeaderEntryV1 {
                name: HttpHeaderNameV1::new("x-label").expect("header"),
                value: HttpHeaderValueSourceV1::Scalar {
                    source: "/label".parse().expect("pointer"),
                },
            }],
            body: DeclarativeHttpBodyV1::ApplicationJson {
                template: HttpJsonTemplateV1::Object {
                    properties: HttpJsonPropertiesV1::new(vec![
                        HttpJsonPropertyV1 {
                            name: HttpJsonPropertyNameV1::new("a").expect("property"),
                            value: HttpJsonTemplateV1::Argument {
                                source: "/body".parse().expect("pointer"),
                            },
                        },
                        HttpJsonPropertyV1 {
                            name: HttpJsonPropertyNameV1::new("z").expect("property"),
                            value: HttpJsonTemplateV1::Literal { value: json!(2) },
                        },
                    ])
                    .expect("canonical properties"),
                },
            },
            credential_slot: None,
            success_status_ranges: vec![HttpStatusRangeV1 { start: 200, end: 299 }],
        }
    }

    #[test]
    fn p1a_and_r1_build_exact_request_once_with_egress_evidence() {
        let arguments = json!({
            "id": "group%2Fproject",
            "tags": ["one two", "three"],
            "label": "literal label",
            "body": {"b": 2, "a": 1}
        });
        let authorizations = ["/id", "/tags", "/label", "/body"]
            .into_iter()
            .map(|pointer| {
                let pointer: ArgumentJsonPointer = pointer.parse().expect("pointer");
                let value = arguments.pointer(pointer.as_str()).expect("value");
                (pointer, authorization(value))
            })
            .collect();
        let mut meter = OrdinaryToolIoMeterV1::default();
        let prepared = prepare_declarative_http_request_v1(
            &request("/v1/"),
            &arguments,
            &BTreeMap::new(),
            &authorizations,
            &mut meter,
        )
        .expect("prepare");
        prepared.validate_integrity().expect("builder output integrity");
        assert_eq!(prepared.canonical_path, "/v1/items/group%252Fproject");
        assert_eq!(
            prepared.exact_url,
            "https://example.com/v1/items/group%252Fproject?tag=one%20two&tag=three"
        );
        assert_eq!(prepared.body, br#"{"a":{"a":1,"b":2},"z":2}"#);
        assert_eq!(prepared.argument_evidence.len(), 4);
        assert_eq!(
            prepared.network_origin_scope(),
            &ResolvedScope::NetworkOrigin {
                scheme: "https".to_owned(),
                host: "example.com".to_owned(),
                port: 443,
            }
        );
        assert!(prepared.status_is_success(204));
        assert!(!prepared.status_is_success(400));
    }

    #[test]
    fn network_scope_uses_the_same_exact_origin_and_explicit_port_as_the_request() {
        let mut request = request("/v1");
        request.target = DeclarativeHttpTargetV1::FixedHttps {
            origin: FixedHttpsOriginV1::new("https://example.com:8443").expect("origin"),
            base_path: HttpBasePathV1::new("/v1").expect("base path"),
        };
        request.path.clear();
        request.query.clear();
        request.headers.clear();
        request.body = DeclarativeHttpBodyV1::None;
        let mut meter = OrdinaryToolIoMeterV1::default();
        let prepared = prepare_declarative_http_request_v1(
            &request,
            &json!({}),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut meter,
        )
        .expect("prepare");

        assert_eq!(prepared.exact_url, "https://example.com:8443/v1");
        assert_eq!(
            prepared.network_origin_scope(),
            &ResolvedScope::NetworkOrigin {
                scheme: "https".to_owned(),
                host: "example.com".to_owned(),
                port: 8443,
            }
        );
    }

    #[test]
    fn explicit_base_trailing_slash_changes_path_request_and_grant_hash() {
        let prepare = |base_path| {
            let mut request = request(base_path);
            request.path.clear();
            request.query.clear();
            request.headers.clear();
            request.body = DeclarativeHttpBodyV1::None;
            let mut meter = OrdinaryToolIoMeterV1::default();
            prepare_declarative_http_request_v1(
                &request,
                &json!({}),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &mut meter,
            )
            .expect("prepare")
        };
        let plain = prepare("/v1/items");
        let slash = prepare("/v1/items/");
        assert_ne!(plain.canonical_path, slash.canonical_path);
        assert_ne!(plain.request_hash, slash.request_hash);
        assert_ne!(plain.grant_manifest_hash, slash.grant_manifest_hash);
    }

    #[test]
    fn prepared_request_integrity_rejects_post_builder_mutation() {
        let arguments = json!({
            "id": "id",
            "tags": [],
            "label": "label",
            "body": null
        });
        let authorizations = ["/id", "/tags", "/label", "/body"]
            .into_iter()
            .map(|pointer| {
                let pointer: ArgumentJsonPointer = pointer.parse().expect("pointer");
                let value = arguments.pointer(pointer.as_str()).expect("value");
                (pointer, authorization(value))
            })
            .collect();
        let mut meter = OrdinaryToolIoMeterV1::default();
        let mut prepared = prepare_declarative_http_request_v1(
            &request("/"),
            &arguments,
            &BTreeMap::new(),
            &authorizations,
            &mut meter,
        )
        .expect("prepare");
        prepared.validate_integrity().expect("builder output integrity");

        prepared.headers.push(("x-extra".to_owned(), "changed".to_owned()));
        assert!(matches!(
            prepared.validate_integrity(),
            Err(PreparedHttpRequestErrorV1::ContractInvalid)
        ));
    }

    #[test]
    fn missing_or_changed_egress_authorization_fails_before_gateway() {
        let arguments = json!({
            "id": "id",
            "tags": [],
            "label": "label",
            "body": null
        });
        let mut meter = OrdinaryToolIoMeterV1::default();
        assert!(matches!(
            prepare_declarative_http_request_v1(
                &request("/"),
                &arguments,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &mut meter,
            ),
            Err(PreparedHttpRequestErrorV1::EgressAuthorizationMissing)
        ));
    }
}
