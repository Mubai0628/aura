//! The approval card names the frozen proxy before any request is sent.
use aura_tool_contract::{ToolNetworkProxyProtocolV1, ToolSettingsSnapshotV1};
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub(crate) fn network_proxy_notice(
    settings: &ToolSettingsSnapshotV1,
    action_kind: &str,
    action: &Value,
) -> Option<String> {
    let proxy = settings
        .network_proxy
        .as_ref()
        .filter(|proxy| proxy.enabled && proxy.validate().is_ok())?;
    if !public_https(action_kind, action) {
        return None;
    }
    let protocol = match proxy.protocol {
        ToolNetworkProxyProtocolV1::Http => "HTTP",
        ToolNetworkProxyProtocolV1::Socks5 => "SOCKS5",
    };
    Some(format!("访问公网地址时将经本机代理 {}:{}（{protocol}）发出", proxy.host, proxy.port))
}

fn public_https(action_kind: &str, action: &Value) -> bool {
    match action_kind {
        "web.search" => true,
        "http.request" | "http.download" | "web.fetch" => action
            .get("url")
            .and_then(Value::as_str)
            .and_then(host_of_https)
            .is_some_and(|host| public_host(&host)),
        _ => false,
    }
}

fn host_of_https(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = authority.rsplit_once('@').map(|(_, host)| host).unwrap_or(authority);
    let host = if let Some(inner) = hostport.strip_prefix('[') {
        inner.split_once(']')?.0
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    if host.is_empty() { None } else { Some(host.to_ascii_lowercase()) }
}

fn public_host(host: &str) -> bool {
    if host == "localhost"
        || host == "metadata.google.internal"
        || host.ends_with(".local")
        || host.ends_with(".localhost")
    {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => public_ip(ip),
        Err(_) => true,
    }
}

fn public_ip(ip: IpAddr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    match ip {
        IpAddr::V4(address) => public_v4(address),
        IpAddr::V6(address) => public_v6(address),
    }
}

fn public_v4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    if address.is_private()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_documentation()
        || octets[0] == 0
        || octets[0] >= 240
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
    {
        return false;
    }
    true
}

fn public_v6(address: Ipv6Addr) -> bool {
    !address.is_unicast_link_local() && !address.is_unique_local()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::Revision;
    use aura_tool_contract::{
        ToolNetworkProxyProtocolV1, ToolNetworkProxyV1, ToolSettingsSnapshotV1,
    };
    use serde_json::json;

    fn settings(protocol: ToolNetworkProxyProtocolV1) -> ToolSettingsSnapshotV1 {
        ToolSettingsSnapshotV1::new(Revision::new(1), 1, 1, 1_000, 0)
            .expect("settings")
            .with_network_proxy(Some(
                ToolNetworkProxyV1::new(true, protocol, "127.0.0.1".into(), 10808).expect("proxy"),
            ))
            .expect("proxy settings")
    }

    #[test]
    fn the_card_names_the_proxy_only_for_public_https() {
        let socks = settings(ToolNetworkProxyProtocolV1::Socks5);
        assert_eq!(
            network_proxy_notice(&socks, "http.request", &json!({"url":"https://example.com/a"})),
            Some("访问公网地址时将经本机代理 127.0.0.1:10808（SOCKS5）发出".into())
        );
        assert_eq!(
            network_proxy_notice(&socks, "web.search", &json!({})),
            Some("访问公网地址时将经本机代理 127.0.0.1:10808（SOCKS5）发出".into())
        );
        assert!(
            network_proxy_notice(&socks, "http.request", &json!({"url":"http://example.com"}))
                .is_none()
        );
        assert!(
            network_proxy_notice(&socks, "web.read", &json!({"url":"https://example.com"}))
                .is_none()
        );
        assert!(
            network_proxy_notice(&socks, "http.request", &json!({"url":"https://127.0.0.1/local"}))
                .is_none()
        );
        assert!(
            network_proxy_notice(
                &socks,
                "http.download",
                &json!({"url":"https://192.168.1.8/file"})
            )
            .is_none()
        );
        let mut disabled = socks.clone();
        disabled.network_proxy.as_mut().expect("proxy").enabled = false;
        assert!(
            network_proxy_notice(&disabled, "web.fetch", &json!({"url":"https://example.com"}))
                .is_none()
        );
    }
}
