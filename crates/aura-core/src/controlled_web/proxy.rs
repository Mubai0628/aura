//! Frozen step settings decide the proxy. Live settings and the process environment do not.
use super::*;
use aura_network_control::{
    GatewayHopRouteV1, NetworkControlError, ordinary_tools::OrdinaryToolRequestGrant,
};
use aura_permission_domain::CoreRequestGrant;

impl ControlledWeb {
    pub(super) fn remember_route(
        &self,
        operation: impl std::fmt::Display,
        routing: &[GatewayHopRouteV1],
    ) {
        let Ok(value) = serde_json::to_value(routing) else {
            return;
        };
        if let Ok(mut routes) = self.network_routes.lock() {
            routes.insert(operation.to_string(), value);
        }
    }

    pub(super) fn take_route(
        &self,
        operation: impl std::fmt::Display,
    ) -> Option<serde_json::Value> {
        self.network_routes.lock().ok()?.remove(&operation.to_string())
    }

    /// Copies a valid frozen proxy onto an already issued ordinary grant.
    ///
    /// A missing step fact, or a snapshot that no longer validates, leaves the grant direct.
    /// An invalid proxy must not fail the tool or the rest of its settings.
    pub(super) async fn apply_frozen_proxy(
        &self,
        operation: aura_domain_common::SecurityOperationId,
        run: Option<aura_domain_common::AgentRunId>,
        mut grant: CoreRequestGrant<OrdinaryToolRequestGrant>,
    ) -> Result<CoreRequestGrant<OrdinaryToolRequestGrant>> {
        let web = self.clone();
        let settings = self
            .section(store_step(run, "agent.http.frozen_proxy"), StoreAccess::Read, move || {
                let store = web.journal.lock()?;
                let db = store.database.connection();
                let Some((batch, _)) =
                    aura_persistence_v2::tool::execution::operation_call(db, operation)?
                else {
                    return Ok(None);
                };
                match aura_persistence_v2::agent::application::step_tool_settings(
                    db,
                    batch.invocation_identity.model_step_id,
                ) {
                    Ok(settings) => Ok(Some(settings)),
                    Err(aura_persistence_core::PersistenceCoreError::Conflict) => Ok(None),
                    Err(error) => Err(anyhow::anyhow!(error.to_string())),
                }
            })
            .await?;
        if let Some(settings) = settings
            && settings.validate().is_ok()
            && settings.network_proxy.is_some()
        {
            grant.purpose = grant
                .purpose
                .with_frozen_tool_settings(&settings)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        Ok(grant)
    }

    /// Points an already running test gateway at loopback fixtures. Production startup never calls this.
    #[cfg(test)]
    pub(crate) fn install_l01_fixtures(
        &self,
        records: Vec<aura_network_control::TestFixtureDnsRecord>,
        certificate_pem: Vec<u8>,
    ) -> Result<()> {
        self.gateway
            .install_test_fixture_dns(records)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        self.gateway.install_test_fixture_tls(certificate_pem);
        Ok(())
    }
}

/// Model-facing text. A proved pre-HTTP proxy failure names the proxy and says the direct
/// attempt is what happened next. A later failure does not claim the request was unsent.
pub(super) fn explain_transport(error: NetworkControlError) -> anyhow::Error {
    let request_not_started =
        matches!(&error, NetworkControlError::RoutedTransport { request_not_started: true, .. });
    let text = match &error {
        NetworkControlError::RoutedTransport { error, routing, request_not_started } => {
            match routing.iter().find_map(|hop| hop.proxy_failure.as_ref()) {
                Some(failure) if *request_not_started => format!(
                    "{}: 无法连接工具设置中的代理 {}:{}，尚未向目标发出 HTTP 请求。已改走直连，直连失败：{}。请确认代理软件已运行，或在工具设置中关闭代理。",
                    failure.kind.code(),
                    failure.host,
                    failure.port,
                    aura_network_control::ordinary_tools::safe_transport_code(error)
                ),
                Some(failure) => format!(
                    "{}: 本机代理 {}:{} 的转发失败（{}）。不能确认目标请求是否已经发出，没有自动重放。",
                    aura_network_control::ordinary_tools::safe_transport_code(error),
                    failure.host,
                    failure.port,
                    failure.reason
                ),
                None => aura_network_control::ordinary_tools::safe_transport_code(error).to_owned(),
            }
        }
        NetworkControlError::UserProxyFailure(failure) => failure.to_string(),
        other => aura_network_control::ordinary_tools::safe_transport_code(other).to_owned(),
    };
    anyhow::Error::new(TransportFailure { message: text, request_not_started })
}

/// The same text for a request to `url`. A target that could not be reached directly is named
/// (host and port, never the path or the system's own words), with what to do next.
pub(super) fn explain_transport_to(error: NetworkControlError, url: &url::Url) -> anyhow::Error {
    let direct = match &error {
        NetworkControlError::RoutedTransport { error, routing, .. }
            if routing.iter().all(|hop| hop.proxy_failure.is_none()) =>
        {
            Some(error.as_ref())
        }
        NetworkControlError::RoutedTransport { .. } => None,
        other => Some(other),
    };
    if let Some(NetworkControlError::Transport(reason)) = direct
        && let Some(message) = unreachable_target(reason, url)
    {
        let request_not_started = matches!(
            &error,
            NetworkControlError::RoutedTransport { request_not_started: true, .. }
        );
        return anyhow::Error::new(TransportFailure { message, request_not_started });
    }
    explain_transport(error)
}

fn unreachable_target(reason: &str, url: &url::Url) -> Option<String> {
    use aura_network_control::connect_failure as kind;
    let host = url.host_str()?;
    let target = match url.port_or_known_default() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    let code = kind::code(reason);
    Some(match reason {
        kind::REFUSED => format!(
            "{code}: {target} 没有程序在监听（连接被拒绝）。如果它是本机的开发服务器或其他服务，先启动它（例如重新运行启动命令），再重试这个请求。"
        ),
        kind::HOST_NOT_FOUND => format!(
            "{code}: 找不到主机 {host}（域名解析失败）。请检查地址是否写对，确认网络可用后再重试。"
        ),
        kind::TLS_FAILED => format!(
            "{code}: 与 {target} 的 TLS 握手失败（证书或协议不被接受）。如果它只提供 HTTP，请改用 http://；否则检查它的证书后再重试。"
        ),
        kind::RESET => format!(
            "{code}: 与 {target} 的连接被对方断开，不能确认请求是否已经送达。可以稍后重试；如果反复出现，检查目标服务是否正常。"
        ),
        kind::CONNECT_FAILED => {
            format!("{code}: 无法连接 {target}。请确认目标服务已启动、地址和端口正确，再重试。")
        }
        _ => return None,
    })
}

/// Preserve the gateway's typed proof while keeping unsafe transport diagnostics private.
#[derive(Debug)]
pub(super) struct TransportFailure {
    message: String,
    request_not_started: bool,
}
impl std::fmt::Display for TransportFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for TransportFailure {}

pub(super) fn request_not_started(error: &anyhow::Error) -> bool {
    error.downcast_ref::<TransportFailure>().is_some_and(|failure| failure.request_not_started)
}
