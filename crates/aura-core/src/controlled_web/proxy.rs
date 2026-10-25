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
