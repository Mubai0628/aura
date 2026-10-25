mod handshake;
mod tls;
mod transport;
pub(super) use transport::ProxyClient;

use crate::NetworkControlError;
use aura_permission_domain::NormalizedOrigin;
use aura_tool_contract::{ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserProxyFailureKindV1 {
    Unavailable,
    Refused,
    Protocol,
}
impl UserProxyFailureKindV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "network.user_proxy_unavailable",
            Self::Refused => "network.user_proxy_refused",
            Self::Protocol => "network.user_proxy_protocol",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserProxyFailureV1 {
    pub kind: UserProxyFailureKindV1,
    pub protocol: ToolNetworkProxyProtocolV1,
    pub host: String,
    pub port: u16,
    /// Bounded handshake reason, never request URL, credential, headers or body.
    pub reason: String,
}
impl fmt::Display for UserProxyFailureV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: 本机代理 {}:{}：{}。代理阶段尚未向目标发出 HTTP 请求。",
            self.kind.code(),
            self.host,
            self.port,
            self.reason
        )?;
        if self.kind == UserProxyFailureKindV1::Protocol {
            write!(f, "请检查代理协议，常见设置为 SOCKS5 10808、HTTP 10809。")
        } else {
            write!(f, "请确认代理软件已运行，或在工具设置中关闭代理。")
        }
    }
}

pub(super) fn failure(
    proxy: &ToolNetworkProxyV1,
    kind: UserProxyFailureKindV1,
    reason: impl Into<String>,
) -> NetworkControlError {
    NetworkControlError::UserProxyFailure(Box::new(UserProxyFailureV1 {
        kind,
        protocol: proxy.protocol,
        host: proxy.host.clone(),
        port: proxy.port,
        reason: reason.into(),
    }))
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GatewayRouteV1 {
    Direct,
    UserProxy { protocol: ToolNetworkProxyProtocolV1, host: String, port: u16 },
}
impl GatewayRouteV1 {
    pub(super) fn from_proxy(proxy: Option<&ToolNetworkProxyV1>) -> Self {
        match proxy {
            Some(proxy) => Self::UserProxy {
                protocol: proxy.protocol,
                host: proxy.host.clone(),
                port: proxy.port,
            },
            None => Self::Direct,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayHopRouteV1 {
    pub origin: NormalizedOrigin,
    pub route: GatewayRouteV1,
    /// Present when this hop used direct transport because the proxy could not start a request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_failure: Option<UserProxyFailureV1>,
}

pub(super) fn check_proxy(proxy: &ToolNetworkProxyV1) -> Result<(), NetworkControlError> {
    let addresses =
        proxy.loopback_endpoints().map_err(|_| NetworkControlError::DestinationBlocked)?;
    for address in addresses {
        handshake::check_endpoint(address)?;
    }
    Ok(())
}

/// Classification and existing authority checks must precede this selection at each hop.
pub(super) fn select_proxy<'a>(
    ordinary: bool,
    frozen: Option<&'a ToolNetworkProxyV1>,
    destination: &aura_security_contract::ResolvedNetworkDestination,
) -> Option<&'a ToolNetworkProxyV1> {
    use aura_security_contract::NetworkDestinationClass;
    frozen.filter(|proxy| {
        ordinary
            && proxy.enabled
            && proxy.validate().is_ok()
            && destination.scheme == "https"
            && !destination.addresses.is_empty()
            && destination.classes.as_slice() == [NetworkDestinationClass::Public]
    })
}

pub(super) struct HopSendError {
    pub error: NetworkControlError,
    /// A fact about this hop only; a previous redirect may already have sent a request.
    pub request_not_started: bool,
}

pub async fn probe_user_proxy(proxy: &ToolNetworkProxyV1) -> Result<(), NetworkControlError> {
    handshake::probe(proxy, std::time::Duration::from_secs(5)).await
}

pub(super) fn routed_error(
    error: NetworkControlError,
    routing: &[GatewayHopRouteV1],
    request_not_started: bool,
) -> NetworkControlError {
    NetworkControlError::RoutedTransport {
        error: Box::new(error),
        routing: routing.to_vec(),
        request_not_started,
    }
}
