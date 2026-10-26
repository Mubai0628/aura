//! OAuth sign-in for Streamable HTTP servers (the MCP authorization flow). The server's
//! challenge names its protected-resource metadata, that names the authorization server, and
//! Aura signs in there as a public client with PKCE through the user's browser. Every request
//! goes through Core's network gateway; the tokens are kept encrypted for the current Windows
//! user and are added to the server's requests by the gateway only. Nothing here runs before
//! the user trusted the server and asked to sign in.
mod discovery;
mod flow;
mod tokens;
mod web;

use super::McpNetwork;
use super::credentials::{CredentialSource, Placement, RequestCredential, origin};
use aura_config::SettingsFacade;
use aura_domain_common::{CredentialId, UtcTimestamp};
use aura_mcp::{McpHttpAuth, McpServerConfig, McpSignInView, McpTransport};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

/// How long the browser part may take.
const SIGN_IN_WINDOW: Duration = Duration::from_secs(300);
/// A token this close to its end is refreshed first.
const REFRESH_AHEAD: i64 = 60;

#[derive(Clone)]
enum Progress {
    Pending { authorization_server: String },
    Failed(String),
}

/// Whether `server_id` is signed in for `fingerprint`, read in `db` (the caller's own store
/// access; nothing is decrypted).
pub(crate) fn signed_in_in(
    db: &aura_persistence_core::Connection,
    server_id: &str,
    fingerprint: &str,
) -> bool {
    tokens::exists_in(db, server_id, fingerprint)
}

/// What the browser opens to sign in, and the authorization server's host the user sees.
pub(crate) struct Begun {
    pub authorization_url: String,
    pub authorization_server: String,
}

pub(crate) struct McpOAuth {
    journal: Arc<crate::broker::BrokerJournal>,
    network: Option<McpNetwork>,
    settings: Arc<SettingsFacade>,
    progress: Mutex<HashMap<String, Progress>>,
    refreshing: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl McpOAuth {
    pub(crate) fn new(
        journal: Arc<crate::broker::BrokerJournal>,
        network: Option<McpNetwork>,
        settings: Arc<SettingsFacade>,
    ) -> Arc<Self> {
        Arc::new(Self {
            journal,
            network,
            settings,
            progress: Mutex::new(HashMap::new()),
            refreshing: Mutex::new(HashMap::new()),
        })
    }

    fn proxy(&self) -> Option<aura_tool_contract::ToolNetworkProxyV1> {
        self.settings.tool_settings_snapshot().ok().and_then(|settings| settings.network_proxy)
    }

    fn set_progress(&self, server_id: &str, progress: Option<Progress>) {
        if let Ok(mut all) = self.progress.lock() {
            match progress {
                Some(progress) => all.insert(server_id.to_owned(), progress),
                None => all.remove(server_id),
            };
        }
    }

    /// Starts signing in to `server`, trusted at `fingerprint`: reads its challenge and the
    /// authorization server's metadata, registers Aura when no client ID was configured, and
    /// opens the one-shot callback. The browser's return completes the sign-in in the
    /// background; `done` runs after the tokens were stored.
    pub(crate) fn begin(
        self: &Arc<Self>,
        server: &McpServerConfig,
        fingerprint: &str,
        done: impl FnOnce() + Send + 'static,
    ) -> Result<Begun, String> {
        let network = self.network.clone().ok_or("Aura 的网络网关不可用")?;
        let McpTransport::Http {
            url,
            headers,
            auth: Some(McpHttpAuth::OAuth { client_id, scopes }),
        } = &server.transport
        else {
            return Err("这个服务器没有设置 OAuth 登录".into());
        };
        let proxy = self.proxy();
        let configured: Vec<(String, String)> =
            headers.iter().map(|header| (header.name.clone(), header.value.clone())).collect();
        let probe = web::probe(&network, proxy.clone(), url, &configured)?;
        if probe.status != 401 {
            return Err(format!("服务器没有要求登录（HTTP {}）", probe.status));
        }
        let challenge =
            probe.header("www-authenticate").map(discovery::challenge).unwrap_or_default();
        let resource = discovery::protected_resource(
            &network,
            &proxy,
            url,
            challenge.resource_metadata.as_deref(),
        )?;
        let metadata =
            discovery::authorization_server(&network, &proxy, &resource.authorization_server)?;
        if let Some(refusal) = discovery::sign_in_refusal(
            &metadata.authorization_endpoint,
            metadata.registration_endpoint.as_deref(),
            client_id.as_deref(),
        ) {
            return Err(refusal.into());
        }
        let callback = flow::Callback::open()?;
        let redirect_uri = callback.redirect_uri();
        let client_id = match client_id {
            Some(id) => id.clone(),
            None => {
                let endpoint =
                    metadata.registration_endpoint.as_deref().ok_or(discovery::CLIENT_ID_NEEDED)?;
                flow::register(&network, &proxy, endpoint, &redirect_uri)?
            }
        };
        let scopes: Vec<String> = if !scopes.is_empty() {
            scopes.clone()
        } else if let Some(scope) = &challenge.scope {
            scope.split_whitespace().map(str::to_owned).collect()
        } else {
            resource.scopes.clone()
        };
        let verifier = flow::random_token()?;
        let state = flow::random_token()?;
        let authorization_url = flow::authorization_url(
            &metadata,
            &client_id,
            &redirect_uri,
            &flow::challenge(&verifier),
            &state,
            &resource.resource,
            &scopes,
        )?;
        let host = url::Url::parse(&metadata.authorization_endpoint)
            .ok()
            .and_then(|address| address.host_str().map(str::to_owned))
            .unwrap_or_default();
        let authorization = tokens::Authorization {
            server: metadata,
            client_id,
            redirect_uri,
            resource: resource.resource,
            scopes,
        };
        self.set_progress(
            &server.id,
            Some(Progress::Pending { authorization_server: host.clone() }),
        );
        let (oauth, server_id, fingerprint) =
            (Arc::clone(self), server.id.clone(), fingerprint.to_owned());
        std::thread::Builder::new()
            .name("aura-mcp-oauth".into())
            .spawn(move || {
                let finished = (|| {
                    let code = callback.wait(&state, Instant::now() + SIGN_IN_WINDOW)?;
                    let granted = flow::token(
                        &network,
                        &oauth.proxy(),
                        &authorization.server.token_endpoint,
                        &[
                            ("grant_type", "authorization_code"),
                            ("code", code.as_str()),
                            ("redirect_uri", &authorization.redirect_uri),
                            ("client_id", &authorization.client_id),
                            ("code_verifier", verifier.as_str()),
                            ("resource", &authorization.resource),
                        ],
                    )
                    .map_err(flow::TokenError::reason)?;
                    let tokens = tokens::Tokens {
                        access_token: granted.access_token.to_string(),
                        refresh_token: granted.refresh_token.map(|token| token.to_string()),
                    };
                    tokens::save(
                        &oauth.journal,
                        &server_id,
                        &fingerprint,
                        (CredentialId::new(), 1),
                        &authorization,
                        &tokens,
                        granted.expires_in,
                    )
                })();
                match finished {
                    Ok(()) => {
                        oauth.set_progress(&server_id, None);
                        done();
                    }
                    Err(reason) => oauth.set_progress(&server_id, Some(Progress::Failed(reason))),
                }
            })
            .map_err(|error| format!("登录线程无法启动：{error}"))?;
        Ok(Begun { authorization_url, authorization_server: host })
    }

    /// Forgets the server's sign-in (the user signed out, or the server was untrusted, changed
    /// or removed).
    pub(crate) fn sign_out(&self, server_id: &str) -> Result<(), String> {
        self.set_progress(server_id, None);
        tokens::forget(&self.journal, server_id)
    }

    /// Whether the server is signed in for this configuration.
    pub(crate) fn signed_in(&self, server_id: &str, fingerprint: &str) -> bool {
        tokens::exists(&self.journal, server_id, fingerprint)
    }

    /// What the settings page shows about a server's sign-in.
    pub(crate) fn view(&self, server_id: &str, fingerprint: Option<&str>) -> McpSignInView {
        let progress = self.progress.lock().ok().and_then(|all| all.get(server_id).cloned());
        let signed_in = fingerprint.is_some_and(|f| self.signed_in(server_id, f));
        McpSignInView {
            signed_in,
            pending: matches!(progress, Some(Progress::Pending { .. })),
            authorization_server: match &progress {
                Some(Progress::Pending { authorization_server }) => {
                    Some(authorization_server.clone())
                }
                _ => None,
            },
            last_error: match progress {
                Some(Progress::Failed(reason)) => Some(reason),
                _ => None,
            },
        }
    }

    /// Removes the sign-ins of servers that are no longer configured.
    pub(crate) fn sweep(&self, configured: &[String]) {
        for server in tokens::servers(&self.journal) {
            if !configured.contains(&server) {
                let _ = tokens::forget(&self.journal, &server);
            }
        }
    }

    /// The access token for one request, refreshed first when it ends soon or the server just
    /// refused it (`stale`); one refresh at a time per server. A refused refresh ends the
    /// sign-in; an unreachable authorization server leaves it for the next try.
    fn access(
        &self,
        server_id: &str,
        fingerprint: &str,
        stale: bool,
    ) -> Result<(CredentialId, u64, Zeroizing<String>), String> {
        let gate = self
            .refreshing
            .lock()
            .map_err(|_| "登录状态损坏".to_owned())?
            .entry(server_id.to_owned())
            .or_default()
            .clone();
        let _one = gate.lock().map_err(|_| "登录状态损坏".to_owned())?;
        let signed = tokens::load(&self.journal, server_id, fingerprint)?
            .ok_or("这个服务器需要先在设置里登录")?;
        let soon = UtcTimestamp::from_value(
            UtcTimestamp::now().value() + time::Duration::seconds(REFRESH_AHEAD),
        );
        let expiring = signed.session.expires_at.is_some_and(|at| at <= soon);
        if !stale && !expiring {
            let token = Zeroizing::new(signed.tokens.access_token.clone());
            return Ok((signed.session.credential_id, signed.session.revision, token));
        }
        let network = self.network.clone().ok_or("Aura 的网络网关不可用")?;
        let Some(refresh) = signed.tokens.refresh_token.as_deref() else {
            let _ = self.sign_out(server_id);
            return Err("登录已过期，请在设置里重新登录".into());
        };
        let authorization = &signed.authorization;
        let granted = flow::token(
            &network,
            &self.proxy(),
            &authorization.server.token_endpoint,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("client_id", &authorization.client_id),
                ("resource", &authorization.resource),
            ],
        );
        let granted = match granted {
            Ok(granted) => granted,
            Err(flow::TokenError::Refused(reason)) => {
                let _ = self.sign_out(server_id);
                self.set_progress(server_id, Some(Progress::Failed(reason)));
                return Err("登录已失效，请在设置里重新登录".into());
            }
            Err(flow::TokenError::Unreachable(reason)) => {
                return Err(format!("刷新登录没有完成：{reason}"));
            }
        };
        let tokens = tokens::Tokens {
            access_token: granted.access_token.to_string(),
            refresh_token: granted
                .refresh_token
                .map(|token| token.to_string())
                .or_else(|| signed.tokens.refresh_token.clone()),
        };
        let revision = signed.session.revision.saturating_add(1);
        tokens::save(
            &self.journal,
            server_id,
            fingerprint,
            (signed.session.credential_id, revision),
            authorization,
            &tokens,
            granted.expires_in,
        )?;
        let token = Zeroizing::new(tokens.access_token.clone());
        Ok((signed.session.credential_id, revision, token))
    }

    /// What an HTTP connection to `server` (trusted at `fingerprint`) signs in with.
    pub(crate) fn source(
        self: &Arc<Self>,
        server: &McpServerConfig,
        fingerprint: &str,
    ) -> Result<Arc<dyn CredentialSource>, String> {
        let McpTransport::Http { url, .. } = &server.transport else {
            return Err("只有 HTTP 服务器可以登录".into());
        };
        Ok(Arc::new(OAuthSource {
            oauth: Arc::clone(self),
            server_id: server.id.clone(),
            fingerprint: fingerprint.to_owned(),
            origin: origin(url)?,
            stale: AtomicBool::new(false),
        }))
    }
}

struct OAuthSource {
    oauth: Arc<McpOAuth>,
    server_id: String,
    fingerprint: String,
    origin: aura_permission_domain::NormalizedOrigin,
    stale: AtomicBool,
}

impl CredentialSource for OAuthSource {
    fn current(&self) -> Result<RequestCredential, String> {
        let stale = self.stale.swap(false, Ordering::AcqRel);
        let (id, revision, token) = self.oauth.access(&self.server_id, &self.fingerprint, stale)?;
        super::credentials::request_credential(
            id,
            revision,
            &Placement::Bearer,
            self.origin.clone(),
            token.as_bytes(),
        )
    }

    fn rejected(&self) -> bool {
        self.stale.store(true, Ordering::Release);
        true
    }
}
