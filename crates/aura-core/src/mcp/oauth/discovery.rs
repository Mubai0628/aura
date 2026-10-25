//! Finding where a server's sign-in happens: its challenge (RFC 6750 / RFC 9728), its
//! protected-resource metadata (RFC 9728) and its authorization server's metadata (RFC 8414,
//! with OpenID Connect discovery as the fallback). Every address must follow the same rules
//! as the server's own (https; http only on this computer).
use super::web::{self, Answer};
use crate::mcp::McpNetwork;
use aura_tool_contract::ToolNetworkProxyV1;
use serde_json::Value;

/// What a 401 says: where the metadata is and which scope to ask for.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
}

/// Reads `Bearer key="value", key=value` (the scheme in any case).
pub(super) fn challenge(header: &str) -> Challenge {
    let header = header.trim();
    let Some(rest) = header
        .get(..6)
        .filter(|scheme| scheme.eq_ignore_ascii_case("bearer"))
        .and_then(|_| header.get(6..))
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    else {
        return Challenge::default();
    };
    let mut found = Challenge::default();
    let mut text = rest.trim_start();
    while !text.is_empty() {
        let Some((key, after)) = text.split_once('=') else { break };
        let key = key.trim().trim_start_matches(',').trim().to_ascii_lowercase();
        let after = after.trim_start();
        let (value, remainder) = if let Some(quoted) = after.strip_prefix('"') {
            match quoted.split_once('"') {
                Some((value, remainder)) => (value.to_owned(), remainder),
                None => (quoted.to_owned(), ""),
            }
        } else {
            match after.split_once(',') {
                Some((value, remainder)) => (value.trim().to_owned(), remainder),
                None => (after.trim().to_owned(), ""),
            }
        };
        match key.as_str() {
            "resource_metadata" => found.resource_metadata = Some(value),
            "scope" => found.scope = Some(value),
            _ => {}
        }
        text = remainder.trim_start().trim_start_matches(',').trim_start();
    }
    found
}

/// https anywhere, http only for this computer: the rule the server's own address follows.
pub(super) fn allowed_address(address: &str) -> bool {
    let Ok(url) = url::Url::parse(address) else { return false };
    let local = match url.host() {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => return false,
    };
    (url.scheme() == "https" || url.scheme() == "http" && local)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn origin(address: &str) -> Result<(url::Url, String), String> {
    let url = url::Url::parse(address).map_err(|_| "地址不合法".to_owned())?;
    let origin = url.origin().ascii_serialization();
    Ok((url, origin))
}

/// Fetches the first candidate that answers 200 with JSON; a 404 tries the next.
fn first_json(
    network: &McpNetwork,
    proxy: &Option<ToolNetworkProxyV1>,
    candidates: &[String],
    what: &str,
) -> Result<(String, Value), String> {
    for candidate in candidates {
        let answer: Answer = web::send(network, proxy.clone(), "GET", candidate, None)?;
        match answer.status {
            200 => return Ok((candidate.clone(), answer.json()?)),
            404 | 405 | 410 => continue,
            status => return Err(format!("读取{what}失败（HTTP {status}）")),
        }
    }
    Err(format!("服务器没有提供{what}"))
}

/// The protected resource a server names: its identifier, its authorization server and the
/// scopes it lists.
pub(super) struct ProtectedResource {
    pub resource: String,
    pub authorization_server: String,
    pub scopes: Vec<String>,
}

pub(super) fn protected_resource(
    network: &McpNetwork,
    proxy: &Option<ToolNetworkProxyV1>,
    endpoint: &str,
    hinted: Option<&str>,
) -> Result<ProtectedResource, String> {
    let (url, origin) = origin(endpoint)?;
    let path = url.path().trim_end_matches('/');
    let mut candidates = Vec::new();
    if let Some(hint) = hinted {
        // Only metadata on the server's own origin is read.
        let (_, hinted_origin) = self::origin(hint)?;
        if hinted_origin != origin || !allowed_address(hint) {
            return Err("服务器指向的资源元数据不在它自己的地址上，Aura 不读取".into());
        }
        candidates.push(hint.to_owned());
    }
    candidates.push(format!("{origin}/.well-known/oauth-protected-resource{path}"));
    candidates.push(format!("{origin}/.well-known/oauth-protected-resource"));
    candidates.dedup();
    let (_, metadata) = first_json(network, proxy, &candidates, "资源元数据")?;
    let resource = metadata["resource"].as_str().ok_or("资源元数据缺少 resource")?.to_owned();
    let trimmed = resource.trim_end_matches('/');
    if !(endpoint.trim_end_matches('/') == trimmed || endpoint.starts_with(&format!("{trimmed}/")))
    {
        return Err("资源元数据说的不是这个服务器".into());
    }
    let authorization_server = metadata["authorization_servers"]
        .as_array()
        .and_then(|servers| servers.first())
        .and_then(Value::as_str)
        .ok_or("资源元数据没有给出授权服务器")?
        .to_owned();
    if !allowed_address(&authorization_server) {
        return Err("授权服务器的地址不符合规则（需要 https；只有本机可以用 http）".into());
    }
    let scopes = metadata["scopes_supported"]
        .as_array()
        .map(|scopes| scopes.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    Ok(ProtectedResource { resource, authorization_server, scopes })
}

/// An authorization server's endpoints, checked to follow the address rules.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(super) struct AuthorizationServer {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
}

pub(super) fn authorization_server(
    network: &McpNetwork,
    proxy: &Option<ToolNetworkProxyV1>,
    issuer: &str,
) -> Result<AuthorizationServer, String> {
    let (url, origin) = origin(issuer)?;
    let path = url.path().trim_end_matches('/');
    let mut candidates = vec![
        format!("{origin}/.well-known/oauth-authorization-server{path}"),
        format!("{origin}/.well-known/openid-configuration{path}"),
    ];
    if !path.is_empty() {
        candidates.push(format!("{origin}{path}/.well-known/openid-configuration"));
    }
    let (_, metadata) = first_json(network, proxy, &candidates, "授权服务器元数据")?;
    let text = |key: &str| metadata[key].as_str().map(str::to_owned);
    if text("issuer").as_deref().map(|i| i.trim_end_matches('/'))
        != Some(issuer.trim_end_matches('/'))
    {
        return Err("授权服务器元数据的 issuer 与它的地址不符".into());
    }
    let pkce = metadata["code_challenge_methods_supported"]
        .as_array()
        .is_some_and(|methods| methods.iter().any(|method| method == "S256"));
    if !pkce {
        return Err("授权服务器没有声明支持 PKCE（S256），Aura 不在这里登录".into());
    }
    let server = AuthorizationServer {
        issuer: issuer.to_owned(),
        authorization_endpoint: text("authorization_endpoint").ok_or("授权服务器缺少授权地址")?,
        token_endpoint: text("token_endpoint").ok_or("授权服务器缺少令牌地址")?,
        registration_endpoint: text("registration_endpoint"),
    };
    let addresses = [Some(&server.authorization_endpoint), Some(&server.token_endpoint)]
        .into_iter()
        .chain([server.registration_endpoint.as_ref()]);
    if !addresses.flatten().all(|address| allowed_address(address)) {
        return Err("授权服务器的地址不符合规则（需要 https；只有本机可以用 http）".into());
    }
    Ok(server)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_challenge_names_the_metadata_and_the_scope_in_any_spelling() {
        let read = challenge(
            r#"Bearer realm="x", resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource/mcp", scope="repo read:org""#,
        );
        assert_eq!(
            read.resource_metadata.as_deref(),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource/mcp")
        );
        assert_eq!(read.scope.as_deref(), Some("repo read:org"));
        assert_eq!(challenge("bearer scope=files").scope.as_deref(), Some("files"));
        assert_eq!(challenge("Basic realm=x"), Challenge::default());
    }

    #[test]
    fn sign_in_addresses_follow_the_server_rules() {
        assert!(allowed_address("https://auth.example.com/authorize"));
        assert!(allowed_address("http://127.0.0.1:7000/auth"));
        assert!(allowed_address("http://localhost:7000/auth"));
        assert!(!allowed_address("http://auth.example.com/authorize"));
        assert!(!allowed_address("https://user:pw@auth.example.com/"));
        assert!(!allowed_address("javascript:alert(1)"));
    }
}
