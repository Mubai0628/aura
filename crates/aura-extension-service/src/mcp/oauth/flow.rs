//! The authorization code flow with PKCE (RFC 7636) for a public client: a random verifier
//! and state, a one-shot callback on 127.0.0.1 (RFC 8252), dynamic registration (RFC 7591)
//! when no client ID was configured, and the token requests with the `resource` the tokens
//! are for (RFC 8707).
use super::discovery::AuthorizationServer;
use super::web;
use crate::mcp::McpNetwork;
use aura_tool_contract::ToolNetworkProxyV1;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

/// 32 random bytes, base64url: a PKCE verifier or a state.
pub(super) fn random_token() -> Result<Zeroizing<String>, String> {
    let mut bytes = Zeroizing::new([0_u8; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|_| "系统随机数不可用".to_owned())?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_ref())))
}

pub(super) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn page(stream: &mut TcpStream, status: &str, text: &str) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Aura</title><p style=\"font:16px sans-serif\">{text}</p>"
    );
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

/// The browser comes back here once, with the code for exactly this sign-in's state.
pub(super) struct Callback {
    listener: TcpListener,
    port: u16,
}

impl Callback {
    pub(super) fn open() -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("本机回调端口无法打开：{error}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(Self { listener, port })
    }

    pub(super) fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// Waits until `deadline` for the browser's return with `state`; a request for another
    /// path or with another state is answered and ignored.
    pub(super) fn wait(&self, state: &str, deadline: Instant) -> Result<Zeroizing<String>, String> {
        while Instant::now() < deadline {
            let (mut stream, _) = match self.listener.accept() {
                Ok(accepted) => accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(error) => return Err(format!("本机回调出错：{error}")),
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            // The request head may arrive in more than one segment: read until its blank line,
            // a full buffer, the end of the stream or the deadline.
            let mut buffer = [0_u8; 8192];
            let mut read = 0;
            while read < buffer.len() && Instant::now() < deadline {
                match stream.read(&mut buffer[read..]) {
                    Ok(0) | Err(_) => break,
                    Ok(more) => read += more,
                }
                if buffer[..read].windows(4).any(|end| end == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let target = head.split_whitespace().nth(1).unwrap_or_default().to_owned();
            let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
                page(&mut stream, "400 Bad Request", "无法识别的请求。");
                continue;
            };
            if !head.starts_with("GET ") || url.path() != "/callback" {
                page(&mut stream, "404 Not Found", "这里只接收 Aura 的登录回调。");
                continue;
            }
            let query: std::collections::HashMap<String, String> =
                url.query_pairs().into_owned().collect();
            if query.get("state").map(String::as_str) != Some(state) {
                page(
                    &mut stream,
                    "400 Bad Request",
                    "这个登录回调不属于 Aura 正在进行的登录，已忽略。",
                );
                continue;
            }
            if let Some(error) = query.get("error") {
                page(&mut stream, "200 OK", "登录没有完成，可以关闭这个页面，回到 Aura 查看原因。");
                let detail = query.get("error_description").cloned().unwrap_or_default();
                return Err(format!("授权服务器拒绝了登录：{error} {detail}").trim().to_owned());
            }
            if let Some(code) = query.get("code").filter(|code| !code.is_empty()) {
                page(&mut stream, "200 OK", "Aura 已收到登录结果，可以关闭这个页面。");
                return Ok(Zeroizing::new(code.clone()));
            }
            page(&mut stream, "400 Bad Request", "登录回调缺少授权码。");
        }
        Err("登录超时（5 分钟内没有完成浏览器里的登录）".into())
    }
}

/// Registers Aura as a public client for `redirect_uri`; returns its client ID.
pub(super) fn register(
    network: &McpNetwork,
    proxy: &Option<ToolNetworkProxyV1>,
    endpoint: &str,
    redirect_uri: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "client_name": "Aura",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let answer = web::send(
        network,
        proxy.clone(),
        "POST",
        endpoint,
        Some(("application/json", body.to_string().into_bytes())),
    )?;
    if !(200..300).contains(&answer.status) {
        return Err(format!("授权服务器拒绝了 Aura 的注册（HTTP {}）", answer.status));
    }
    answer.json()?["client_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "注册结果缺少 client_id".to_owned())
}

/// The address the browser opens.
pub(super) fn authorization_url(
    server: &AuthorizationServer,
    client_id: &str,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    resource: &str,
    scopes: &[String],
) -> Result<String, String> {
    let mut url =
        url::Url::parse(&server.authorization_endpoint).map_err(|_| "授权地址不合法".to_owned())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state)
            .append_pair("resource", resource);
        if !scopes.is_empty() {
            query.append_pair("scope", &scopes.join(" "));
        }
    }
    Ok(url.into())
}

/// What a token endpoint answered: the access token, maybe a refresh token, its lifetime.
pub(super) struct Granted {
    pub access_token: Zeroizing<String>,
    pub refresh_token: Option<Zeroizing<String>>,
    pub expires_in: Option<u64>,
}

/// Why a token request gave no token.
pub(super) enum TokenError {
    /// The authorization server answered and refused: the grant (code or refresh token) is
    /// no good any more.
    Refused(String),
    /// It could not be asked, or its answer could not be used; the grant may still be good.
    Unreachable(String),
}

impl TokenError {
    pub(super) fn reason(self) -> String {
        match self {
            Self::Refused(reason) | Self::Unreachable(reason) => reason,
        }
    }
}

/// One token request (`grant_type` and its fields); the answer must be a bearer token.
pub(super) fn token(
    network: &McpNetwork,
    proxy: &Option<ToolNetworkProxyV1>,
    endpoint: &str,
    form: &[(&str, &str)],
) -> Result<Granted, TokenError> {
    let body = Zeroizing::new(
        url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish(),
    );
    let answer = web::send(
        network,
        proxy.clone(),
        "POST",
        endpoint,
        Some(("application/x-www-form-urlencoded", body.as_bytes().to_vec())),
    )
    .map_err(TokenError::Unreachable)?;
    let value: Value = answer.json().unwrap_or(Value::Null);
    if (400..500).contains(&answer.status) {
        let error = value["error"].as_str().unwrap_or("unknown_error");
        return Err(TokenError::Refused(format!(
            "授权服务器拒绝了令牌请求（HTTP {}，{error}）",
            answer.status
        )));
    }
    if !(200..300).contains(&answer.status) {
        return Err(TokenError::Unreachable(format!("授权服务器出错（HTTP {}）", answer.status)));
    }
    if !value["token_type"].as_str().is_some_and(|kind| kind.eq_ignore_ascii_case("bearer")) {
        return Err(TokenError::Unreachable("授权服务器给的不是 Bearer 令牌".into()));
    }
    let access_token = value["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| TokenError::Unreachable("令牌回应缺少 access_token".into()))?;
    Ok(Granted {
        access_token: Zeroizing::new(access_token.to_owned()),
        refresh_token: value["refresh_token"].as_str().map(|t| Zeroizing::new(t.to_owned())),
        expires_in: value["expires_in"].as_u64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verifier_is_random_and_its_challenge_is_s256() {
        let first = random_token().expect("random");
        let second = random_token().expect("random");
        assert_ne!(*first, *second);
        assert_eq!(first.len(), 43);
        // RFC 7636 appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_callback_takes_only_its_own_state() {
        let callback = Callback::open().expect("callback");
        let address = callback.redirect_uri().replace("http://", "").replace("/callback", "");
        let waiter = std::thread::spawn(move || {
            callback.wait("mine", Instant::now() + Duration::from_secs(10))
        });
        let send = |target: &str| {
            let mut stream = TcpStream::connect(&address).expect("connect");
            let _ = write!(stream, "GET {target} HTTP/1.1\r\nHost: x\r\n\r\n");
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
            answer
        };
        assert!(send("/callback?code=stolen&state=other").starts_with("HTTP/1.1 400"));
        assert!(send("/elsewhere?code=x&state=mine").starts_with("HTTP/1.1 404"));
        assert!(send("/callback?code=the-code&state=mine").starts_with("HTTP/1.1 200"));
        let code = waiter.join().expect("waiter").expect("the code");
        assert_eq!(code.as_str(), "the-code");
    }

    #[test]
    fn a_request_that_arrives_in_pieces_is_read_whole() {
        let callback = Callback::open().expect("callback");
        let address = callback.redirect_uri().replace("http://", "").replace("/callback", "");
        let waiter = std::thread::spawn(move || {
            callback.wait("mine", Instant::now() + Duration::from_secs(10))
        });
        // A browser's request may reach the listener in more than one segment.
        let mut stream = TcpStream::connect(&address).expect("connect");
        stream.write_all(b"GET /callback?code=the-code").expect("first part");
        stream.flush().expect("flush");
        std::thread::sleep(Duration::from_millis(200));
        stream.write_all(b"&state=mine HTTP/1.1\r\nHost: x\r\n\r\n").expect("second part");
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        let code = waiter.join().expect("waiter").expect("the code");
        assert_eq!(code.as_str(), "the-code");
    }
}
