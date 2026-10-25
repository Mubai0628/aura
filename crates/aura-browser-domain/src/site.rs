//! The sites the user allows or blocks for the built-in browser (capability decisions §2.3).
//!
//! A rule is a host name, `*.` and a host name (every subdomain at any depth, never the host
//! itself), an IPv4 address or a bracketed IPv6 address, with an optional port. Without a port a
//! rule covers every port and scheme of that host. A pasted `http(s)://` address keeps only its
//! host and a port written in it. Host names are normalized the way a browser normalizes an
//! address (lower case, punycode, the IPv4 spellings a URL accepts), then must be plain
//! letters, digits and hyphens. The browser host runs the same cases
//! (`fixtures/browser/site-rules.v1.json`).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The network site of a page or a request: the scheme, the host as a browser writes it (IPv6 in
/// brackets) and the port, the scheme's default when none is written.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSiteV1 {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl fmt::Display for BrowserSiteV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}:{}", self.scheme, self.host, self.port)
    }
}

/// The site of an `http`, `https`, `ws` or `wss` address. Other pages (a workspace file,
/// `about:blank`, `data:`) have none, and site rules do not apply to them.
#[must_use]
pub fn site_of_url(address: &str) -> Option<BrowserSiteV1> {
    let url = url::Url::parse(address).ok()?;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") {
        return None;
    }
    let host = match url.host()? {
        url::Host::Domain(domain) => domain.trim_end_matches('.').to_ascii_lowercase(),
        url::Host::Ipv4(address) => address.to_string(),
        url::Host::Ipv6(address) => format!("[{address}]"),
    };
    Some(BrowserSiteV1 {
        scheme: url.scheme().to_owned(),
        host,
        port: url.port_or_known_default()?,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BrowserSiteRuleError {
    #[error("写一个网站，例如 example.com、*.example.com 或 localhost:5173")]
    Empty,
    #[error("网站名无效：只能是域名、*.域名、IPv4 地址或 [IPv6] 地址")]
    Host,
    #[error("端口无效：应为 1–65535")]
    Port,
    #[error("只支持 http 和 https 网址")]
    Scheme,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserSiteRule {
    subdomains: bool,
    host: String,
    port: Option<u16>,
}

impl BrowserSiteRule {
    pub fn parse(input: &str) -> Result<Self, BrowserSiteRuleError> {
        let text = input.trim();
        if text.is_empty() {
            return Err(BrowserSiteRuleError::Empty);
        }
        if let Some((scheme, _)) = text.split_once("://") {
            return Self::from_address(scheme, text);
        }
        let (subdomains, text) = match text.strip_prefix("*.") {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        if text.contains(['@', '/', '\\', '?', '#']) {
            return Err(BrowserSiteRuleError::Host);
        }
        let (host, port) = if let Some(rest) = text.strip_prefix('[') {
            let (inside, after) = rest.split_once(']').ok_or(BrowserSiteRuleError::Host)?;
            let port = match after {
                "" => None,
                _ => Some(port(after.strip_prefix(':').ok_or(BrowserSiteRuleError::Host)?)?),
            };
            (format!("[{inside}]"), port)
        } else {
            match text.rsplit_once(':') {
                Some((host, _)) if host.contains(':') => return Err(BrowserSiteRuleError::Host),
                Some((host, written)) => (host.to_owned(), Some(port(written)?)),
                None => (text.to_owned(), None),
            }
        };
        Ok(Self { subdomains, host: normalized_host(&host, subdomains)?, port })
    }

    /// A pasted address: its host and a port written in it, nothing else.
    fn from_address(scheme: &str, text: &str) -> Result<Self, BrowserSiteRuleError> {
        if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
            return Err(BrowserSiteRuleError::Scheme);
        }
        let url = url::Url::parse(text).map_err(|_| BrowserSiteRuleError::Host)?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(BrowserSiteRuleError::Host);
        }
        let host = url.host_str().ok_or(BrowserSiteRuleError::Host)?;
        Ok(Self { subdomains: false, host: normalized_host(host, false)?, port: url.port() })
    }

    #[must_use]
    pub fn matches(&self, site: &BrowserSiteV1) -> bool {
        if self.port.is_some_and(|port| port != site.port) {
            return false;
        }
        if self.subdomains {
            site.host.len() > self.host.len() + 1
                && site.host.ends_with(&self.host)
                && site.host.as_bytes()[site.host.len() - self.host.len() - 1] == b'.'
        } else {
            site.host == self.host
        }
    }
}

impl fmt::Display for BrowserSiteRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.subdomains {
            f.write_str("*.")?;
        }
        f.write_str(&self.host)?;
        match self.port {
            Some(port) => write!(f, ":{port}"),
            None => Ok(()),
        }
    }
}

fn port(written: &str) -> Result<u16, BrowserSiteRuleError> {
    if written.is_empty() || !written.bytes().all(|b| b.is_ascii_digit()) {
        return Err(BrowserSiteRuleError::Port);
    }
    written.parse::<u16>().ok().filter(|port| *port != 0).ok_or(BrowserSiteRuleError::Port)
}

fn normalized_host(host: &str, subdomains: bool) -> Result<String, BrowserSiteRuleError> {
    match url::Host::parse(host).map_err(|_| BrowserSiteRuleError::Host)? {
        url::Host::Ipv4(_) | url::Host::Ipv6(_) if subdomains => Err(BrowserSiteRuleError::Host),
        url::Host::Ipv4(address) => Ok(address.to_string()),
        url::Host::Ipv6(address) => Ok(format!("[{address}]")),
        url::Host::Domain(domain) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            let label = |label: &str| {
                (1..=63).contains(&label.len())
                    && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && !label.starts_with('-')
                    && !label.ends_with('-')
            };
            if domain.len() <= 253 && domain.split('.').all(label) {
                Ok(domain)
            } else {
                Err(BrowserSiteRuleError::Host)
            }
        }
    }
}

/// What the user's rules say about one site. A blocked site stays blocked even when a broader
/// rule also allows it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSiteDecision {
    Blocked,
    Allowed,
    Unlisted,
}

impl BrowserSiteDecision {
    #[must_use]
    pub fn of(
        site: &BrowserSiteV1,
        allowed: &[BrowserSiteRule],
        blocked: &[BrowserSiteRule],
    ) -> Self {
        if blocked.iter().any(|rule| rule.matches(site)) {
            Self::Blocked
        } else if allowed.iter().any(|rule| rule.matches(site)) {
            Self::Allowed
        } else {
            Self::Unlisted
        }
    }
}
