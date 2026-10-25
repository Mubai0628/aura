//! User-owned proxy settings. Only literal loopback IPs and the fixed localhost alias are valid.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolNetworkProxyProtocolV1 {
    Http,
    Socks5,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolNetworkProxyV1 {
    pub enabled: bool,
    pub protocol: ToolNetworkProxyProtocolV1,
    #[schemars(regex(
        pattern = r"^(localhost|127(\.(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])){3}|::1)$"
    ))]
    pub host: String,
    #[schemars(range(min = 1, max = 65535))]
    pub port: u16,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolNetworkProxyErrorV1 {
    #[error("network_proxy.host: 只允许本机回环地址（127.0.0.0/8、::1 或 localhost）")]
    InvalidHost,
    #[error("network_proxy.port: 端口必须为 1–65535")]
    InvalidPort,
    #[error("network_proxy.host: 代理地址 {0} 是 Aura 已登记的控制端点")]
    ControlEndpoint(SocketAddr),
}

impl ToolNetworkProxyV1 {
    pub fn new(
        enabled: bool,
        protocol: ToolNetworkProxyProtocolV1,
        host: String,
        port: u16,
    ) -> Result<Self, ToolNetworkProxyErrorV1> {
        let host = if host.eq_ignore_ascii_case("localhost") {
            "localhost".into()
        } else {
            host.parse::<IpAddr>().map_err(|_| ToolNetworkProxyErrorV1::InvalidHost)?.to_string()
        };
        let value = Self { enabled, protocol, host, port };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ToolNetworkProxyErrorV1> {
        if self.port == 0 {
            return Err(ToolNetworkProxyErrorV1::InvalidPort);
        }
        self.loopback_endpoints().map(|_| ())
    }

    /// `localhost` is pinned to these two loopback literals; no system DNS or proxy settings
    /// can turn this alias into a remote proxy. The transport checks each actual endpoint again.
    pub fn loopback_endpoints(&self) -> Result<Vec<SocketAddr>, ToolNetworkProxyErrorV1> {
        let addresses = if self.host.eq_ignore_ascii_case("localhost") {
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST), IpAddr::V6(Ipv6Addr::LOCALHOST)]
        } else {
            let ip: IpAddr = self.host.parse().map_err(|_| ToolNetworkProxyErrorV1::InvalidHost)?;
            if !ip.is_loopback() || self.host != ip.to_string() {
                return Err(ToolNetworkProxyErrorV1::InvalidHost);
            }
            vec![ip]
        };
        Ok(addresses.into_iter().map(|ip| SocketAddr::new(ip, self.port)).collect())
    }

    pub fn validate_control_endpoints(
        &self,
        endpoints: &[SocketAddr],
    ) -> Result<(), ToolNetworkProxyErrorV1> {
        self.validate()?;
        for proxy in self.loopback_endpoints()? {
            if endpoints.iter().any(|endpoint| {
                let ip = match endpoint.ip() {
                    IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(IpAddr::V6(ip), IpAddr::V4),
                    ip => ip,
                };
                proxy == SocketAddr::new(ip, endpoint.port())
            }) {
                return Err(ToolNetworkProxyErrorV1::ControlEndpoint(proxy));
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ToolNetworkProxyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            enabled: bool,
            protocol: ToolNetworkProxyProtocolV1,
            host: String,
            port: u16,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.enabled, wire.protocol, wire.host, wire.port).map_err(D::Error::custom)
    }
}
