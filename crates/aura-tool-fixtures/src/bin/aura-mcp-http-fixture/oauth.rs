//! `--oauth`: a fake OAuth 2.1 authorization server in front of `/mcp`, on the same address.
//! Protected-resource metadata names it; its metadata lists the endpoints; clients register
//! themselves; the authorization page agrees at once (it redirects back with a code); the
//! token endpoint checks the PKCE verifier, the redirect and the `resource`, and issues access
//! tokens (`--token-lifetime <seconds>`, default 3600) with refresh tokens that rotate.
//! Test controls: `POST /auth/revoke-all` forgets every token, `POST /auth/expire-access`
//! only the access tokens; `--no-pkce` leaves S256 out of the metadata; `--metadata-elsewhere`
//! names metadata on another origin (`localhost` instead of `127.0.0.1`).
use super::{Request, answer_json, respond};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::TcpStream,
    time::{Duration, Instant},
};

pub(super) const SCOPE: &str = "mcp.read";

struct Code {
    client_id: String,
    redirect_uri: String,
    challenge: String,
}

/// How the fake server misbehaves on purpose.
#[derive(Clone, Copy, Default)]
pub(super) struct Quirks {
    pub no_pkce: bool,
    pub metadata_elsewhere: bool,
}

pub(super) struct OAuth {
    base: String,
    quirks: Quirks,
    lifetime: Duration,
    clients: HashMap<String, Vec<String>>,
    codes: HashMap<String, Code>,
    access: HashMap<String, Instant>,
    refresh: HashMap<String, String>,
    next: u32,
}

/// What one request did, for the marker file.
pub(super) enum Event {
    Metadata,
    Registered(String),
    Authorized,
    Issued,
    Refreshed,
    Refused(&'static str),
    Revoked,
}

impl OAuth {
    pub(super) fn new(base: &str, lifetime: Duration, quirks: Quirks) -> Self {
        Self {
            base: base.to_owned(),
            quirks,
            lifetime,
            clients: HashMap::new(),
            codes: HashMap::new(),
            access: HashMap::new(),
            refresh: HashMap::new(),
            next: 0,
        }
    }

    fn resource(&self) -> String {
        format!("{}/mcp", self.base)
    }

    fn issuer(&self) -> String {
        format!("{}/auth", self.base)
    }

    fn token(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}-{}-{:08x}", self.next, self.next.wrapping_mul(2_654_435_761))
    }

    /// Whether `authorization` carries a live access token.
    pub(super) fn allows(&self, authorization: Option<&str>) -> bool {
        authorization
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|token| self.access.get(token))
            .is_some_and(|expires| Instant::now() < *expires)
    }

    /// The challenge an MCP request without a live token gets.
    pub(super) fn challenge(&self) -> String {
        let base = if self.quirks.metadata_elsewhere {
            self.base.replace("127.0.0.1", "localhost")
        } else {
            self.base.clone()
        };
        format!(
            "Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\", scope=\"{SCOPE}\""
        )
    }

    /// Answers a sign-in request; `None` when `request` is not one.
    pub(super) fn serve(&mut self, stream: &mut TcpStream, request: &Request) -> Option<Event> {
        let (path, query) = request.path.split_once('?').unwrap_or((request.path.as_str(), ""));
        let form = |text: &str| -> HashMap<String, String> {
            url::form_urlencoded::parse(text.as_bytes()).into_owned().collect()
        };
        match (request.method.as_str(), path) {
            ("GET", "/.well-known/oauth-protected-resource/mcp") => {
                let metadata = json!({
                    "resource": self.resource(),
                    "authorization_servers": [self.issuer()],
                    "scopes_supported": [SCOPE],
                    "bearer_methods_supported": ["header"],
                });
                answer_json(stream, None, &metadata);
                Some(Event::Metadata)
            }
            ("GET", "/.well-known/oauth-authorization-server/auth") => {
                let mut metadata = json!({
                    "issuer": self.issuer(),
                    "authorization_endpoint": format!("{}/authorize", self.issuer()),
                    "token_endpoint": format!("{}/token", self.issuer()),
                    "registration_endpoint": format!("{}/register", self.issuer()),
                    "response_types_supported": ["code"],
                    "grant_types_supported": ["authorization_code", "refresh_token"],
                    "code_challenge_methods_supported": ["S256"],
                    "token_endpoint_auth_methods_supported": ["none"],
                    "scopes_supported": [SCOPE],
                });
                if self.quirks.no_pkce
                    && let Some(object) = metadata.as_object_mut()
                {
                    object.remove("code_challenge_methods_supported");
                }
                answer_json(stream, None, &metadata);
                Some(Event::Metadata)
            }
            ("POST", "/auth/expire-access") => {
                self.access.clear();
                respond(stream, "200 OK", &[], b"");
                Some(Event::Revoked)
            }
            ("POST", "/auth/register") => Some(self.register(stream, request)),
            ("GET", "/auth/authorize") => Some(self.authorize(stream, &form(query))),
            ("POST", "/auth/token") => {
                Some(self.exchange(stream, &form(&String::from_utf8_lossy(&request.body))))
            }
            ("POST", "/auth/revoke-all") => {
                self.access.clear();
                self.refresh.clear();
                respond(stream, "200 OK", &[], b"");
                Some(Event::Revoked)
            }
            _ => None,
        }
    }

    fn register(&mut self, stream: &mut TcpStream, request: &Request) -> Event {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        let redirects: Vec<String> = body["redirect_uris"]
            .as_array()
            .map(|uris| uris.iter().filter_map(|uri| uri.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        if redirects.is_empty() || body["token_endpoint_auth_method"] != "none" {
            let error = json!({"error": "invalid_client_metadata"});
            respond(
                stream,
                "400 Bad Request",
                &[("Content-Type", "application/json")],
                error.to_string().as_bytes(),
            );
            return Event::Refused("register");
        }
        let client_id = self.token("client");
        self.clients.insert(client_id.clone(), redirects.clone());
        let answer = json!({"client_id": client_id, "redirect_uris": redirects, "token_endpoint_auth_method": "none"});
        respond(
            stream,
            "201 Created",
            &[("Content-Type", "application/json")],
            answer.to_string().as_bytes(),
        );
        Event::Registered(client_id)
    }

    fn authorize(&mut self, stream: &mut TcpStream, query: &HashMap<String, String>) -> Event {
        let field = |name: &str| query.get(name).map(String::as_str).unwrap_or_default();
        let client = field("client_id");
        let redirect = field("redirect_uri");
        let known =
            self.clients.get(client).is_some_and(|uris| uris.iter().any(|uri| uri == redirect));
        if !known
            || field("response_type") != "code"
            || field("code_challenge_method") != "S256"
            || field("code_challenge").is_empty()
            || field("state").is_empty()
            || field("resource") != self.resource()
        {
            respond(stream, "400 Bad Request", &[], b"invalid authorization request");
            return Event::Refused("authorize");
        }
        let code = self.token("code");
        self.codes.insert(
            code.clone(),
            Code {
                client_id: client.to_owned(),
                redirect_uri: redirect.to_owned(),
                challenge: field("code_challenge").to_owned(),
            },
        );
        let state: String =
            url::form_urlencoded::byte_serialize(field("state").as_bytes()).collect();
        let location = format!("{redirect}?code={code}&state={state}");
        respond(stream, "302 Found", &[("Location", location.as_str())], b"");
        Event::Authorized
    }

    fn exchange(&mut self, stream: &mut TcpStream, form: &HashMap<String, String>) -> Event {
        let field = |name: &str| form.get(name).map(String::as_str).unwrap_or_default();
        let refuse = |stream: &mut TcpStream, error: &str| {
            let body = json!({"error": error});
            respond(
                stream,
                "400 Bad Request",
                &[("Content-Type", "application/json")],
                body.to_string().as_bytes(),
            );
        };
        if field("resource") != self.resource() {
            refuse(stream, "invalid_target");
            return Event::Refused("resource");
        }
        let client = field("client_id").to_owned();
        let refreshed = match field("grant_type") {
            "authorization_code" => {
                let Some(code) = self.codes.remove(field("code")) else {
                    refuse(stream, "invalid_grant");
                    return Event::Refused("code");
                };
                let verified =
                    URL_SAFE_NO_PAD.encode(Sha256::digest(field("code_verifier").as_bytes()));
                if code.client_id != client
                    || code.redirect_uri != field("redirect_uri")
                    || code.challenge != verified
                {
                    refuse(stream, "invalid_grant");
                    return Event::Refused("verifier");
                }
                false
            }
            "refresh_token" => {
                if self.refresh.remove(field("refresh_token")).as_deref() != Some(client.as_str()) {
                    refuse(stream, "invalid_grant");
                    return Event::Refused("refresh");
                }
                true
            }
            _ => {
                refuse(stream, "unsupported_grant_type");
                return Event::Refused("grant");
            }
        };
        let access = self.token("access");
        let refresh = self.token("refresh");
        self.access.insert(access.clone(), Instant::now() + self.lifetime);
        self.refresh.insert(refresh.clone(), client);
        let answer = json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": self.lifetime.as_secs(),
            "refresh_token": refresh,
            "scope": SCOPE,
        });
        answer_json(stream, None, &answer);
        if refreshed { Event::Refreshed } else { Event::Issued }
    }
}
