//! The permission resource a web address is checked against: its network origin.
use anyhow::Result;
use aura_security_contract::ResourceDescriptor;

pub fn resource(url: &url::Url) -> Result<ResourceDescriptor> {
    let host = match url.host().ok_or_else(|| anyhow::anyhow!("web.host"))? {
        url::Host::Domain(v) => v.trim_end_matches('.').to_ascii_lowercase(),
        url::Host::Ipv4(v) => v.to_string(),
        url::Host::Ipv6(v) => v.to_string(),
    };
    Ok(ResourceDescriptor::NetworkOrigin {
        scheme: url.scheme().into(),
        host,
        port: url.port_or_known_default().ok_or_else(|| anyhow::anyhow!("web.port"))?,
    })
}
