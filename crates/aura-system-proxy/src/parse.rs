//! WinINet's proxy text, read the way WinINet routes an https request.
use crate::{InternetSettingsValues, SystemProxyLookup};
use aura_tool_contract::{
    ToolNetworkProxyErrorV1, ToolNetworkProxyProtocolV1 as Protocol, ToolNetworkProxyV1,
};

/// What the current user's values mean for a tool's https request.
///
/// A manual proxy that is switched on is used even beside a configuration script: Aura runs no
/// scripts, and proxy programs that set both point them at the same local port.
#[must_use]
pub fn from_values(values: &InternetSettingsValues) -> SystemProxyLookup {
    let server = values.proxy_server.as_deref().map(str::trim).filter(|text| !text.is_empty());
    if values.proxy_enable.is_some_and(|flag| flag != 0)
        && let Some(server) = server
    {
        return from_server(server);
    }
    if values.auto_config_url.as_deref().is_some_and(|url| !url.trim().is_empty()) {
        return SystemProxyLookup::AutoConfig;
    }
    SystemProxyLookup::NotSet
}

/// `ProxyServer` holds one address for every scheme, or `scheme=address` entries split by `;`.
/// An https request takes the `https` entry, else the one address, else the `socks` entry.
fn from_server(server: &str) -> SystemProxyLookup {
    let (mut https, mut every, mut socks) = (None, None, None);
    for entry in server.split(';').map(str::trim).filter(|entry| !entry.is_empty()) {
        match entry.split_once('=') {
            Some((scheme, address)) if scheme.trim().eq_ignore_ascii_case("https") => {
                https = https.or(Some(address.trim()));
            }
            Some((scheme, address)) if scheme.trim().eq_ignore_ascii_case("socks") => {
                socks = socks.or(Some(address.trim()));
            }
            Some(_) => {}
            None => every = every.or(Some(entry)),
        }
    }
    let chosen = https
        .map(|address| (address, Protocol::Http))
        .or(every.map(|address| (address, Protocol::Http)))
        .or(socks.map(|address| (address, Protocol::Socks5)));
    chosen
        .and_then(|(address, protocol)| proxy_at(address, protocol))
        .unwrap_or_else(|| SystemProxyLookup::Unusable { server: server.to_owned() })
}

/// One address, optionally behind `http://` or `socks5://`, which name how the proxy is spoken
/// to. None when Aura cannot speak to it or the address has no usable port.
fn proxy_at(address: &str, protocol: Protocol) -> Option<SystemProxyLookup> {
    let (protocol, rest) = match address.split_once("://") {
        None => (protocol, address),
        Some((scheme, rest)) => {
            let protocol = match scheme.to_ascii_lowercase().as_str() {
                "http" => Protocol::Http,
                "socks" | "socks5" | "socks5h" => Protocol::Socks5,
                _ => return None,
            };
            (protocol, rest.trim_end_matches('/'))
        }
    };
    let (host, port) = host_and_port(rest)?;
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    match ToolNetworkProxyV1::new(true, protocol, host.to_owned(), port) {
        Ok(proxy) => Some(SystemProxyLookup::Found(proxy)),
        Err(ToolNetworkProxyErrorV1::InvalidHost) => {
            Some(SystemProxyLookup::NotLocal { server: rest.to_owned() })
        }
        Err(_) => None,
    }
}

/// `host:port` or `[ipv6]:port`.
fn host_and_port(text: &str) -> Option<(&str, &str)> {
    if let Some(bracketed) = text.strip_prefix('[') {
        let (host, rest) = bracketed.split_once(']')?;
        return Some((host, rest.strip_prefix(':')?));
    }
    let (host, port) = text.rsplit_once(':')?;
    (!host.is_empty() && !host.contains(':')).then_some((host, port))
}
