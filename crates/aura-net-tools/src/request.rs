use anyhow::{Result, ensure};
use aura_domain_common::NetworkRequestId;
use aura_network_control::{GatewayRequest, SecretScanner};
use aura_permission_domain::{NetworkByteLimits, NetworkRequestPlan, RedirectPolicy};
use aura_security_contract::{InvocationIdentity, NetworkCallerKind};
use aura_web_domain::*;

pub fn request_url(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value).map_err(|_| anyhow::anyhow!("http.invalid_url"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "http.invalid_url"
    );
    Ok(url)
}

pub fn http_request(
    input: &HttpRequestInput,
    identity: InvocationIdentity,
) -> Result<GatewayRequest> {
    let method = input.method.to_ascii_uppercase();
    reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| anyhow::anyhow!("http.invalid_method"))?;
    ensure!(method != "CONNECT", "http.unsupported_method");
    ensure!(
        input.body.is_none() || !matches!(method.as_str(), "GET" | "HEAD"),
        "http.body_not_supported"
    );
    ensure!(input.headers.len() <= 64, "http.header_limit");
    let mut size = 0usize;
    let mut headers = Vec::new();
    for header in &input.headers {
        let name = reqwest::header::HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|_| anyhow::anyhow!("http.invalid_header_name"))?;
        reqwest::header::HeaderValue::from_str(&header.value)
            .map_err(|_| anyhow::anyhow!("http.invalid_header_value"))?;
        let name = name.as_str();
        SecretScanner.scan_header(name, &header.value)?;
        ensure!(
            !matches!(
                name,
                "host"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "upgrade"
                    | "proxy-connection"
                    | "proxy-authorization"
                    | "proxy-authenticate"
                    | "trailer"
                    | "te"
                    | "keep-alive"
            ) && !name.starts_with("x-aura-")
                && !name.starts_with("sec-")
                && !header.value.contains(['\r', '\n']),
            "http.header_controlled"
        );
        size = size.saturating_add(name.len() + header.value.len());
        ensure!(size <= 16 * 1024, "http.header_limit");
        headers.push((name.to_owned(), header.value.clone()));
    }
    let (body, content_type) = match &input.body {
        None => (vec![], None),
        Some(HttpRequestBody::Text { text, content_type }) => {
            (text.as_bytes().to_vec(), content_type.clone())
        }
        Some(HttpRequestBody::Json { value }) => {
            (serde_json::to_vec(value)?, Some("application/json".into()))
        }
    };
    ensure!(body.len() <= HTTP_BODY_LIMIT, "http.body_limit");
    if let Some(value) = content_type {
        reqwest::header::HeaderValue::from_str(&value)
            .map_err(|_| anyhow::anyhow!("http.invalid_content_type"))?;
        ensure!(!value.contains(['\r', '\n']), "http.invalid_content_type");
        if !headers.iter().any(|(name, _)| name == "content-type") {
            headers.push(("content-type".into(), value));
        }
    }
    let request = request(
        &input.url,
        method,
        headers,
        body,
        identity,
        NetworkCallerKind::AuraTool,
        HTTP_RESPONSE_LIMIT,
    )?;
    SecretScanner.scan_http_request(&request, true)?;
    Ok(request)
}

/// The headers a download may send, for a site that serves its
/// files only to its own pages. Never a cookie or a credential.
const DOWNLOAD_HEADERS: &[&str] = &["user-agent", "referer", "accept", "accept-language"];

pub fn download_request(
    input: &HttpDownloadInput,
    identity: InvocationIdentity,
) -> Result<GatewayRequest> {
    ensure!(!input.path.trim().is_empty(), "http.path_required");
    let mut headers: Vec<(String, String)> = Vec::new();
    for header in &input.headers {
        let name = header.name.trim().to_ascii_lowercase();
        ensure!(
            DOWNLOAD_HEADERS.contains(&name.as_str())
                && !headers.iter().any(|(sent, _)| *sent == name),
            "http.header_controlled"
        );
        reqwest::header::HeaderValue::from_str(&header.value)
            .map_err(|_| anyhow::anyhow!("http.invalid_header_value"))?;
        ensure!(
            header.value.len() <= 1024 && !header.value.contains(['\r', '\n']),
            "http.invalid_header_value"
        );
        SecretScanner.scan_header(&name, &header.value)?;
        headers.push((name, header.value.clone()));
    }
    request(
        &input.url,
        "GET".into(),
        headers,
        vec![],
        identity,
        NetworkCallerKind::AuraTool,
        HTTP_DOWNLOAD_LIMIT,
    )
}

/// One JSON request to a web service the user configured (DeepSeek's search or fetch), as
/// the web runtime; Core injects the service's key at the gateway.
pub fn web_service_request(
    url: &str,
    body: &serde_json::Value,
    identity: InvocationIdentity,
    limit: u64,
) -> Result<GatewayRequest> {
    ensure!(limit > 0 && limit <= HTTP_RESPONSE_LIMIT, "web.byte_limit");
    request(
        url,
        "POST".into(),
        vec![("content-type".into(), "application/json".into())],
        serde_json::to_vec(body)?,
        identity,
        NetworkCallerKind::WebRuntime,
        limit,
    )
}

pub fn web_request(url: &str, identity: InvocationIdentity, limit: u64) -> Result<GatewayRequest> {
    ensure!(limit > 0 && limit <= HTTP_RESPONSE_LIMIT, "web.byte_limit");
    request(url, "GET".into(), vec![], vec![], identity, NetworkCallerKind::WebRuntime, limit)
}

fn request(
    url: &str,
    method: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    identity: InvocationIdentity,
    caller: NetworkCallerKind,
    limit: u64,
) -> Result<GatewayRequest> {
    let url = request_url(url)?;
    let redirects = if matches!(method.as_str(), "GET" | "HEAD") {
        RedirectPolicy::ReauthorizeEachHop { max_hops: 3 }
    } else {
        RedirectPolicy::Deny
    };
    Ok(GatewayRequest {
        plan: NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            identity,
            caller,
            method,
            requested_url: url.to_string(),
            destination: None,
            request_data: vec![],
            data_classes: vec![],
            redirect_policy: redirects,
            byte_limits: NetworkByteLimits {
                max_upload_bytes: body.len() as u64,
                max_download_bytes: limit,
                max_decompressed_bytes: limit,
            },
            timeout_ms: if limit > HTTP_RESPONSE_LIMIT { 300_000 } else { 30_000 },
            credential_refs: vec![],
            network_grant_ids: vec![],
        },
        headers,
        body,
    })
}
