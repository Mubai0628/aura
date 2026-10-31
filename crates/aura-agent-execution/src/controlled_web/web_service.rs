//! DeepSeek's own search and fetch for `web.search` and
//! `web.fetch`.
//!
//!Core decides the service when it plans a call and freezes it into the operation's action - the
//! connection, its origin and its key binding - so the request, its network resource and the key
//! never change after. The key is the connection's own, resolved through the same checks a model
//! request makes, and only ever sent to that connection's origin. A fetch of a loopback or private
//! address never goes to DeepSeek; it stays on this computer.
use super::*;
use aura_config::{ConfigModule, SettingsFacade};
use aura_domain_common::{EndpointId, jcs::canonical_sha256};
use aura_model_domain::EndpointAuthScheme;
use aura_model_service::{p05, p05_credential};
use aura_network_control::{
    ConfigurationRequestCredentialBindingV1, NetworkControlError, ProviderCredentialResolver,
    ResolvedToolCredential, ToolCredentialBinding, ToolCredentialResolver,
};
use aura_permission_domain::{ToolCredentialPlacementV1, ToolCredentialUseV1};
use aura_security_contract::CredentialUseRef;
use aura_tool_contract::WebServiceSettingV1;
use serde::{Deserialize, Serialize};

/// The Core-only action field the decision is frozen in.
pub(crate) const WEB_SERVICE_FIELD: &str = "web_service";
const SEARCH_SURFACE: &str = "deepseek-search-v1";
/// DeepSeek's search takes a query of at most this many characters.
const QUERY_CHARS: usize = 128;

/// The DeepSeek connection one planned call uses.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeepSeekWebService {
    pub endpoint_id: EndpointId,
    /// The connection's origin, such as `https://api.deepseek.com`.
    pub origin: String,
    pub credential: Option<ConfigurationRequestCredentialBindingV1>,
}

/// Which DeepSeek connection the settings choose now; set once the model services are open.
pub trait WebServiceSource: Send + Sync {
    fn deepseek(&self) -> Result<Option<DeepSeekWebService>>;
}

/// The settings' choice over the model configuration's connections.
pub struct ConfiguredWebService {
    settings: Arc<SettingsFacade>,
    p05: p05::P05Services,
}

impl ConfiguredWebService {
    pub fn new(settings: Arc<SettingsFacade>, p05: p05::P05Services) -> Self {
        Self { settings, p05 }
    }

    /// The connections the service can use: DeepSeek's own API, with a key.
    pub fn connections(&self) -> Result<Vec<(String, DeepSeekWebService)>> {
        let config = self.p05.model_config.snapshot()?;
        let mut found = Vec::new();
        for endpoint in config.registry.endpoints.values() {
            let Ok(url) = url::Url::parse(&endpoint.base_url) else { continue };
            let credential = if fixture(&url) && endpoint.auth_scheme == EndpointAuthScheme::None {
                None
            } else if deepseek_api(&url)
                && endpoint.auth_scheme == EndpointAuthScheme::OpenAiBearerApiKey
            {
                let Ok(Some(binding)) = self
                    .p05
                    .configuration_credential_preflight(endpoint)
                    .map(|preflight| preflight.credential_binding)
                else {
                    continue;
                };
                Some(binding)
            } else {
                continue;
            };
            found.push((
                endpoint.display_name.clone(),
                DeepSeekWebService {
                    endpoint_id: endpoint.id,
                    origin: url.origin().ascii_serialization(),
                    credential,
                },
            ));
        }
        found.sort_by_key(|(_, service)| service.endpoint_id);
        Ok(found)
    }
}

impl WebServiceSource for ConfiguredWebService {
    fn deepseek(&self) -> Result<Option<DeepSeekWebService>> {
        let snapshot = self.settings.snapshot(ConfigModule::Tools)?;
        let connections = self.connections()?.into_iter().map(|(_, service)| service);
        match aura_config::web_service_from_document(&snapshot.document)? {
            WebServiceSettingV1::Local => Ok(None),
            WebServiceSettingV1::Automatic => Ok(connections.into_iter().next()),
            // A chosen connection that can no longer be used is said so, never replaced.
            WebServiceSettingV1::DeepSeek { endpoint_id } => connections
                .into_iter()
                .find(|service| service.endpoint_id == endpoint_id)
                .map(Some)
                .ok_or_else(|| anyhow::anyhow!(CONNECTION_UNUSABLE)),
        }
    }
}

/// Why a chosen connection cannot carry the web service.
pub(crate) const CONNECTION_UNUSABLE: &str = "web.service_connection_unusable: 网页搜索与抓取选用的 DeepSeek 连接已不可用（不存在、不是 DeepSeek 官方 API 或没有密钥），请在工具设置里重新选择。";

/// DeepSeek's own API over HTTPS.
fn deepseek_api(url: &url::Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some("api.deepseek.com")
}

/// In Core's own tests only: the keyless loopback Provider fixture that stands in for it.
fn fixture(url: &url::Url) -> bool {
    cfg!(any(test, feature = "test-hooks")) && url.host_str() == Some("127.0.0.1")
}

impl ControlledWeb {
    pub fn set_web_service_source(&self, source: Arc<dyn WebServiceSource>) {
        let _ = self.web_service.set(source);
    }

    /// The DeepSeek service for one call being planned: `None` when the settings keep the web on
    /// this computer, and for a fetch of a loopback or private address.
    pub(crate) fn deepseek_service(
        &self,
        kind: &str,
        action: &serde_json::Value,
    ) -> Result<Option<DeepSeekWebService>> {
        let Some(source) = self.web_service.get() else { return Ok(None) };
        match kind {
            "web.search" => {}
            "web.fetch" => {
                let input: WebFetchInput =
                    serde_json::from_value(crate::agent_execution::network::model_action(action))?;
                if fetched_here(&input)? {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }
        source.deepseek()
    }

    /// One call's request to DeepSeek, its key put in by the gateway, and the result in the
    /// tool's own shape.
    pub(super) async fn invoke_deepseek(
        &self,
        dispatch: &AuthorizedDispatch,
        run: AgentRunId,
        service: DeepSeekWebService,
        cancel: &mut watch::Receiver<bool>,
    ) -> InvokeFinish {
        let failed = |error: anyhow::Error| InvokeFinish {
            value: Err(error),
            cancelled: false,
            stop_confirmed: true,
        };
        let content = &dispatch.request().operation.content;
        let Some(session) = content.identity.session_id else {
            return failed(anyhow::anyhow!("web.session_required"));
        };
        let kind = content.action_kind.clone();
        let action = crate::agent_execution::network::model_action(&content.action);
        let sent = or_cancel(cancel, async {
            let request = crate::agent_execution::network::gateway_request(content)?;
            let usage = credential_use(&service).map(|(usage, _)| usage);
            let grant = aura_network_control::ordinary_tools::OrdinaryToolRequestGrant::issue(
                dispatch,
                &request,
                usage.clone(),
                UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::seconds(30)),
            )?;
            let grant = self
                .apply_frozen_proxy(dispatch.request().operation.operation_id, Some(run), grant)
                .await?;
            let start = self.transport_start(dispatch, &request, "web.request_started").await?;
            let resolver = match (usage, &service.credential) {
                (Some(usage), Some(requested)) => Some(ConnectionKey {
                    usage,
                    requested: requested.clone(),
                    provider: p05_credential::PersistentProviderCredentialResolver::new(
                        Arc::clone(&self.persistence),
                    ),
                }),
                _ => None,
            };
            let operation_id = dispatch.request().operation.operation_id;
            let response = self
                .gateway
                .execute_ordinary(
                    grant,
                    request,
                    resolver.as_ref().map(|key| key as &dyn ToolCredentialResolver),
                    start,
                )
                .await
                .map_err(super::proxy::explain_transport)?;
            self.remember_route(operation_id, &response.routing);
            anyhow::Ok((response.response.status, response.response.body))
        })
        .await;
        let (status, body) = match sent {
            OrCancel::Cancelled { stop_confirmed } => {
                return InvokeFinish::cancelled(stop_confirmed);
            }
            OrCancel::Value(Err(error)) => return failed(error),
            OrCancel::Value(Ok(value)) => value,
        };
        if let Err(error) = refused(status, &body) {
            return failed(error);
        }
        let web = self.clone();
        let value = self
            .section(
                StoreStep::run(run, "agent.web.deepseek_result"),
                StoreAccess::Write,
                move || -> Result<serde_json::Value> {
                    if kind == "web.search" {
                        web.deepseek_search_result(session, &action, &body)
                    } else {
                        web.deepseek_fetch_result(session, &action, &body)
                    }
                },
            )
            .await;
        InvokeFinish { value, cancelled: false, stop_confirmed: true }
    }

    fn deepseek_search_result(
        &self,
        session: SessionId,
        action: &serde_json::Value,
        body: &[u8],
    ) -> Result<serde_json::Value> {
        let input: WebSearchInput = serde_json::from_value(action.clone())?;
        let answer: SearchAnswer = serde_json::from_slice(body)?;
        let hits = answer
            .results
            .into_iter()
            .map(|hit| {
                let source = hit.site_name.filter(|name| !name.is_empty()).unwrap_or_else(|| {
                    url::Url::parse(&hit.url)
                        .ok()
                        .and_then(|url| url.host_str().map(ToOwned::to_owned))
                        .unwrap_or_default()
                });
                let snippet = if hit.summary.is_empty() { hit.snippet } else { hit.summary };
                WebSearchHit {
                    title: hit.title,
                    url: hit.url,
                    snippet,
                    source,
                    published_at: hit.date,
                }
            })
            .collect();
        let id = ArtifactId::new();
        let at = UtcTimestamp::now();
        let result = WebSearchResult {
            query: input.query,
            surface: SEARCH_SURFACE.into(),
            hits,
            retrieved_at: at,
            live: true,
            evidence_artifact_id: id,
        };
        let bytes = serde_json::to_vec(&result)?;
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_core::artifact::put(
                tx,
                id,
                &format!("web-search:{session}"),
                "application/vnd.aura.web-search+json",
                &bytes,
                at,
            )
        })?;
        Ok(serde_json::to_value(result)?)
    }

    fn deepseek_fetch_result(
        &self,
        session: SessionId,
        action: &serde_json::Value,
        body: &[u8],
    ) -> Result<serde_json::Value> {
        let input: WebFetchInput = serde_json::from_value(action.clone())?;
        let page: FetchAnswer = serde_json::from_slice(body)?;
        let document = WebDocument {
            source_url: input.url,
            title: page.title,
            media_type: "text/markdown".into(),
            text: markdown_unescaped(&page.content),
            links: vec![],
            retrieved_at: UtcTimestamp::now(),
            response_hash: ContentHash::sha256(body),
            rendered: false,
        };
        let id = ArtifactId::new();
        let bytes = serde_json::to_vec(&document)?;
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_core::artifact::put(
                tx,
                id,
                &format!("web-document:{session}"),
                DOCUMENT_MEDIA_TYPE,
                &bytes,
                document.retrieved_at,
            )
        })?;
        let (excerpt, _) = aura_web_runtime::excerpt(&document.text, 0, 12000)?;
        Ok(serde_json::to_value(WebFetchResult {
            document_artifact_id: id,
            source_url: document.source_url,
            title: document.title,
            excerpt,
            total_chars: u32::try_from(document.text.chars().count()).unwrap_or(u32::MAX),
            rendered: false,
        })?)
    }
}

/// The decision frozen into a planned call, when it was DeepSeek.
pub(crate) fn frozen(action: &serde_json::Value) -> Result<Option<DeepSeekWebService>> {
    Ok(action
        .get(WEB_SERVICE_FIELD)
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?)
}

/// DeepSeek's search or fetch address on the connection's origin.
pub(crate) fn service_url(service: &DeepSeekWebService, kind: &str) -> Result<url::Url> {
    let path = if kind == "web.search" { "/v1/tools/search" } else { "/v1/tools/fetch" };
    Ok(url::Url::parse(&service.origin)?.join(path)?)
}

/// The request DeepSeek's search or fetch takes for the model's own input.
pub(crate) fn request_body(kind: &str, action: &serde_json::Value) -> Result<serde_json::Value> {
    if kind == "web.search" {
        let input: WebSearchInput = serde_json::from_value(action.clone())?;
        ensure!(
            input.query.chars().count() <= QUERY_CHARS,
            "web.search_query_too_long: DeepSeek 搜索的查询最多 {QUERY_CHARS} 个字符，请缩短后重试。"
        );
        Ok(serde_json::json!({ "query": input.query, "count": input.max_results.clamp(1, 50) }))
    } else {
        let input: WebFetchInput = serde_json::from_value(action.clone())?;
        Ok(serde_json::json!({ "url": input.url }))
    }
}

/// The key use one call binds: the connection's own key as a Bearer header.
pub(crate) fn credential_use(
    service: &DeepSeekWebService,
) -> Option<(ToolCredentialUseV1, CredentialUseRef)> {
    let binding = service.credential.as_ref()?;
    let usage = ToolCredentialUseV1 {
        credential_id: binding.credential_id,
        credential_revision: binding.credential_revision,
        slot_key: aura_tool_contract::CredentialSlotKey::new("api_key").ok()?,
        placement: ToolCredentialPlacementV1::BearerAuthorization,
    };
    let reference = CredentialUseRef {
        credential_id: binding.credential_id,
        credential_revision: binding.credential_revision,
        authority: SecuritySourceRef::SystemComponent {
            component_id: "model-connection-web-service-v1".into(),
        },
        use_fingerprint: canonical_sha256(service).ok()?,
    };
    Some((usage, reference))
}

/// Whether a fetch goes from this computer rather than through DeepSeek: an
/// address on this computer or its network, a fetch the model asked to make directly (`http_only`),
/// or a file a page is made of or data is kept in. DeepSeek's fetch reads pages; it answered a
/// three.js module with "404" and an API's JSON with escaped Markdown.
pub(crate) fn fetched_here(input: &WebFetchInput) -> Result<bool> {
    const FILES: &[&str] = &[
        "json", "js", "mjs", "cjs", "css", "txt", "csv", "tsv", "xml", "md", "yaml", "yml", "toml",
        "map", "svg", "ndjson",
    ];
    let url = aura_web_runtime::normalize_url(&input.url)?;
    let file = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(_, extension)| FILES.contains(&extension.to_ascii_lowercase().as_str()));
    Ok(local_address(&url) || input.mode == aura_web_domain::WebFetchMode::HttpOnly || file)
}

/// DeepSeek's fetch hands back a page as Markdown with the punctuation of its
/// text escaped; a JSON answer read `\{\"queryEnc\"\:…`. The escapes come off as a Markdown reader
/// takes them, outside code spans and fenced blocks.
pub(crate) fn markdown_unescaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let opening = line.trim_start();
        match fence {
            Some(marker) => {
                if opening.starts_with(marker) {
                    fence = None;
                }
                out.push_str(line);
            }
            None if opening.starts_with("```") || opening.starts_with("~~~") => {
                fence = Some(&opening[..3]);
                out.push_str(line);
            }
            None => unescape_line(line, &mut out),
        }
    }
    out
}

fn unescape_line(line: &str, out: &mut String) {
    let mut rest = line;
    while let Some(at) = rest.find(['\\', '`']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with('`') {
            // A code span runs to the next run of as many backticks; without one they are text.
            let ticks = rest.len() - rest.trim_start_matches('`').len();
            let fence = &rest[..ticks];
            let close = rest[ticks..].find(fence).map(|end| ticks + end + ticks);
            let span = close.map_or(ticks, |end| end);
            out.push_str(&rest[..span]);
            rest = &rest[span..];
            continue;
        }
        let mut chars = rest.chars();
        chars.next();
        match chars.next() {
            Some(escaped) if escaped.is_ascii_punctuation() => {
                out.push(escaped);
                rest = &rest[1 + escaped.len_utf8()..];
            }
            _ => {
                out.push('\\');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
}

fn local_address(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        Some(url::Host::Ipv6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
        }
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            !name.contains('.')
                || [".localhost", ".local", ".lan", ".internal", ".home.arpa"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
        }
        None => true,
    }
}

/// What a refusal by DeepSeek means for the user.
fn refused(status: u16, body: &[u8]) -> Result<()> {
    let detail = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(ToOwned::to_owned))
        .unwrap_or_default();
    let reason = match status {
        200..=299 => return Ok(()),
        401 => {
            "web.deepseek_key_invalid: DeepSeek 拒绝了这个连接的密钥，请在模型设置里检查 DeepSeek 连接的密钥。"
        }
        402 => {
            "web.deepseek_balance: DeepSeek 账户余额不足，网页搜索与抓取暂时不可用；充值后再试。"
        }
        403 => "web.deepseek_fetch_refused: DeepSeek 按它的安全策略拒绝抓取这个网址。",
        404 => {
            "web.deepseek_fetch_empty: DeepSeek 没能从这个网址取到可用的内容；接口的 JSON、脚本或其他文件可加 mode: http_only，由 Aura 直接抓取。"
        }
        429 => "web.deepseek_rate_limited: DeepSeek 网页搜索与抓取的请求太频繁，请稍后再试。",
        408 | 500..=599 => {
            "web.deepseek_unavailable: DeepSeek 的网页搜索与抓取服务暂时不可用，请稍后再试。"
        }
        _ => "web.deepseek_refused: DeepSeek 拒绝了这次请求。",
    };
    // The first line is Core's own and reaches the model and the user; DeepSeek's detail
    // stays on the second, in the receipt only.
    Err(anyhow::anyhow!("{reason}（HTTP {status}）\n{detail}"))
}

/// A refusal about the
/// requested page or query, not the service, is the operation's rejection with its own category;
/// a bad key, the balance, the rate limit or an outage stay failures of the tool.
pub(crate) fn rejection(error: &str) -> Option<aura_tool_contract::ModelSafeToolErrorV1> {
    let line = error.lines().next()?;
    let (code, _) = line.split_once(':')?;
    let category = match code {
        "web.deepseek_fetch_empty" => aura_domain_common::ErrorCategory::NotFound,
        "web.deepseek_fetch_refused" => aura_domain_common::ErrorCategory::Permission,
        "web.deepseek_refused" | "web.search_query_too_long" => {
            aura_domain_common::ErrorCategory::Validation
        }
        _ => return None,
    };
    Some(aura_tool_contract::ModelSafeToolErrorV1 {
        code: aura_domain_common::ErrorCode::new(code).ok()?,
        category,
        message: line.to_owned(),
        retryable: false,
    })
}

#[derive(Deserialize)]
struct SearchAnswer {
    #[serde(default)]
    results: Vec<SearchHit>,
}

#[derive(Deserialize)]
struct SearchHit {
    #[serde(default)]
    title: String,
    url: String,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    site_name: Option<String>,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Deserialize)]
struct FetchAnswer {
    #[serde(default)]
    title: String,
    content: String,
}

/// The connection's own key, put in only for its own origin by the model request's checks.
struct ConnectionKey {
    usage: ToolCredentialUseV1,
    requested: ConfigurationRequestCredentialBindingV1,
    provider: p05_credential::PersistentProviderCredentialResolver,
}

impl ToolCredentialResolver for ConnectionKey {
    fn resolve(
        &self,
        binding: &ToolCredentialUseV1,
    ) -> Result<ResolvedToolCredential, NetworkControlError> {
        if binding != &self.usage {
            return Err(NetworkControlError::CredentialBindingMismatch);
        }
        let resolved = self.provider.resolve(&self.requested)?;
        Ok(ResolvedToolCredential {
            binding: ToolCredentialBinding {
                credential_id: resolved.binding.credential_id,
                credential_revision: resolved.binding.credential_revision,
                origin: resolved.binding.origin.clone(),
                header_name: resolved.binding.header_name.clone(),
            },
            secret_header_value: resolved.secret_header_value.clone(),
        })
    }
}

#[cfg(test)]
#[path = "web_service_tests.rs"]
mod tests;
