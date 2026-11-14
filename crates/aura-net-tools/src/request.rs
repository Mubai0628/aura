use anyhow::{Result, bail, ensure};
use aura_domain_common::NetworkRequestId;
use aura_network_control::{GatewayRequest, NetworkControlError, SecretScanner};
use aura_permission_domain::{NetworkByteLimits, NetworkRequestPlan, RedirectPolicy};
use aura_security_contract::{InvocationIdentity, NetworkCallerKind};
use aura_web_domain::*;

// Each refusal below is Core's line for the model: the code, what was wrong with the request,
// that nothing was sent, and what to change. None repeats a header value or the URL.

pub fn request_url(value: &str) -> Result<url::Url> {
    let broken = match url::Url::parse(value) {
        Err(_) => "地址不是有效的 URL，请求没有发出。请写出完整的 http:// 或 https:// 地址。",
        Ok(url) if !matches!(url.scheme(), "http" | "https") => {
            "只支持 http:// 和 https:// 地址，请求没有发出。"
        }
        Ok(url) if url.host_str().is_none() => {
            "地址里没有主机名，请求没有发出。请写出完整的 http:// 或 https:// 地址。"
        }
        Ok(url) if !url.username().is_empty() || url.password().is_some() => {
            "地址里带了用户名或密码，Aura 不发送这样的地址，请求没有发出。需要认证时请用工具设置里绑定的凭据。"
        }
        Ok(url) if url.fragment().is_some() => {
            "地址里带了 # 片段（它不会发给服务器），请求没有发出。请去掉 # 及后面的部分再调用。"
        }
        Ok(url) => return Ok(url),
    };
    bail!("http.invalid_url: {broken}")
}

pub fn http_request(
    input: &HttpRequestInput,
    identity: InvocationIdentity,
) -> Result<GatewayRequest> {
    let method = input.method.to_ascii_uppercase();
    reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| {
        anyhow::anyhow!(
            "http.invalid_method: 请求方法 {} 无效，请求没有发出。请用 GET、POST、PUT、PATCH、DELETE 这类标准方法。",
            shown(&input.method)
        )
    })?;
    ensure!(
        method != "CONNECT",
        "http.unsupported_method: http.request 不支持 CONNECT 方法，请求没有发出。"
    );
    ensure!(
        input.body.is_none() || !matches!(method.as_str(), "GET" | "HEAD"),
        "http.body_not_supported: GET 和 HEAD 请求不能带 body，请求没有发出。请去掉 body，或改用 POST 等方法。"
    );
    ensure!(
        input.headers.len() <= 64,
        "http.header_limit: 请求头最多 64 个，请求没有发出。请减少请求头。"
    );
    let mut size = 0usize;
    let mut headers = Vec::new();
    for header in &input.headers {
        let name = reqwest::header::HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| {
            anyhow::anyhow!(
                "http.invalid_header_name: 请求头名 {} 不是有效的 HTTP 头名，请求没有发出。头名只能用字母、数字和 - 这类符号。",
                shown(&header.name)
            )
        })?;
        reqwest::header::HeaderValue::from_str(&header.value).map_err(|_| {
            anyhow::anyhow!(
                "http.invalid_header_value: 请求头 {} 的值不是有效的 HTTP 头值（含有控制字符或非 ASCII 字符），请求没有发出。",
                shown(&header.name)
            )
        })?;
        let name = name.as_str();
        SecretScanner.scan_header(name, &header.value).map_err(scanned)?;
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
            "http.header_controlled: 请求头 {} 由 Aura 或网络层自己设置，调用里不能指定，请求没有发出。请去掉这个请求头。",
            shown(name)
        );
        size = size.saturating_add(name.len() + header.value.len());
        ensure!(
            size <= 16 * 1024,
            "http.header_limit: 请求头加起来超过了 16 KB，请求没有发出。请减少或缩短请求头。"
        );
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
    ensure!(
        body.len() <= HTTP_BODY_LIMIT,
        "http.body_limit: 请求体超过了 1 MB 的上限，请求没有发出。请减小请求内容。"
    );
    if let Some(value) = content_type {
        ensure!(
            reqwest::header::HeaderValue::from_str(&value).is_ok() && !value.contains(['\r', '\n']),
            "http.invalid_content_type: content_type 不是有效的 HTTP 头值，请求没有发出。请写成 application/json 这样的媒体类型。"
        );
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
    SecretScanner.scan_http_request(&request, true).map_err(scanned)?;
    Ok(request)
}

/// The headers a download may send, for a site that serves its
/// files only to its own pages. Never a cookie or a credential.
const DOWNLOAD_HEADERS: &[&str] = &["user-agent", "referer", "accept", "accept-language"];

pub fn download_request(
    input: &HttpDownloadInput,
    identity: InvocationIdentity,
) -> Result<GatewayRequest> {
    ensure!(
        !input.path.trim().is_empty(),
        "http.path_required: 没有给出下载位置，文件没有下载。请在 path 里写出要保存到的文件路径。"
    );
    let mut headers: Vec<(String, String)> = Vec::new();
    for header in &input.headers {
        let name = header.name.trim().to_ascii_lowercase();
        ensure!(
            DOWNLOAD_HEADERS.contains(&name.as_str())
                && !headers.iter().any(|(sent, _)| *sent == name),
            "http.header_controlled: 下载只能带 User-Agent、Referer、Accept 和 Accept-Language 请求头，每个最多一次，{} 不行，请求没有发出。",
            shown(&header.name)
        );
        ensure!(
            reqwest::header::HeaderValue::from_str(&header.value).is_ok()
                && header.value.len() <= 1024
                && !header.value.contains(['\r', '\n']),
            "http.invalid_header_value: 请求头 {} 的值不是有效的 HTTP 头值（超过 1024 字节，或含有换行、控制字符或非 ASCII 字符），请求没有发出。",
            shown(&header.name)
        );
        SecretScanner.scan_header(&name, &header.value).map_err(scanned)?;
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

const BYTE_LIMIT: &str =
    "web.byte_limit: max_bytes must be between 1 and 8388608 (8 MiB). Nothing was fetched.";

/// One JSON request to a web service the user configured (DeepSeek's search or fetch), as
/// the web runtime; Core injects the service's key at the gateway.
pub fn web_service_request(
    url: &str,
    body: &serde_json::Value,
    identity: InvocationIdentity,
    limit: u64,
) -> Result<GatewayRequest> {
    ensure!(limit > 0 && limit <= HTTP_RESPONSE_LIMIT, BYTE_LIMIT);
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
    ensure!(limit > 0 && limit <= HTTP_RESPONSE_LIMIT, BYTE_LIMIT);
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

/// The scanner's refusal in Core's words: where the credential-like value is, never the value.
/// The typed error stays the source, so callers still tell a model-supplied credential apart.
fn scanned(error: NetworkControlError) -> anyhow::Error {
    let line = match &error {
        NetworkControlError::SecretDetected { location, .. } => format!(
            "http.model_authentication_forbidden: {} 里有像凭据的内容（认证头、令牌或密钥），请求没有发出。认证信息只能通过工具凭据提供：请去掉它，改用工具设置里绑定的凭据。",
            shown(location)
        ),
        other => aura_network_control::ordinary_tools::safe_transport_line(other),
    };
    anyhow::Error::new(error).context(line)
}

/// A name the model wrote, as the line repeats it: at most 64 characters, without control
/// characters or braces.
fn shown(text: &str) -> String {
    let mut shown: String = text
        .chars()
        .take(64)
        .map(|c| if c.is_control() || c == '{' || c == '}' { ' ' } else { c })
        .collect();
    if text.chars().count() > 64 {
        shown.push('…');
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{CorrelationId, SessionId, WorkspaceId};

    fn build(input: serde_json::Value) -> anyhow::Error {
        http_request(
            &serde_json::from_value(input).expect("input"),
            InvocationIdentity::runtime_user(
                WorkspaceId::new(),
                SessionId::new(),
                CorrelationId::new(),
            ),
        )
        .expect_err("refused")
    }

    /// Each refusal keeps its code, names what was wrong and says nothing was sent; a header's
    /// value never appears.
    #[test]
    fn a_refused_request_says_what_was_wrong() {
        let url = |value: &str| request_url(value).expect_err("refused").to_string();
        assert!(url("ftp://example.org/").starts_with("http.invalid_url: 只支持 http://"));
        assert!(url("https://u:p@example.org/").contains("用户名或密码"));
        assert!(url("https://example.org/#top").contains("# 片段"));
        assert!(url("not a url").contains("不是有效的 URL"));
        let header = build(serde_json::json!({
            "url": "http://localhost/",
            "headers": [{"name": "Host", "value": "elsewhere"}]
        }))
        .to_string();
        assert!(header.starts_with("http.header_controlled: 请求头 host "), "{header}");
        assert!(!header.contains("elsewhere"), "{header}");
        let value = build(serde_json::json!({
            "url": "http://localhost/",
            "headers": [{"name": "X-Test", "value": "a\u{7f}b"}]
        }))
        .to_string();
        assert!(value.starts_with("http.invalid_header_value: 请求头 X-Test "), "{value}");
        let body = build(serde_json::json!({
            "url": "http://localhost/", "method": "GET", "body": {"kind": "text", "text": "x"}
        }))
        .to_string();
        assert!(body.starts_with("http.body_not_supported: "), "{body}");
        let method = build(serde_json::json!({"url": "http://localhost/", "method": "G E T"}));
        assert!(method.to_string().starts_with("http.invalid_method: 请求方法 G E T "));
        let secret = build(serde_json::json!({
            "url": "http://localhost/",
            "headers": [{"name": "Authorization", "value": "Bearer synthetic"}]
        }));
        let line = secret.to_string();
        assert!(line.starts_with("http.model_authentication_forbidden: header.authorization 里"));
        assert!(!line.contains("synthetic"), "{line}");
        assert!(matches!(
            secret.downcast_ref::<NetworkControlError>(),
            Some(NetworkControlError::SecretDetected { .. })
        ));
        for line in [header, value, body, line] {
            assert!(line.len() <= 512 && !line.contains(['\n', '{', '}']), "{line}");
        }
    }
}
