//! The page a `browser.open` url names. The same reading as the address bar
//! (electron-main/browser-host/address.ts): a full http(s) address as written, a host (http for
//! this computer and the local network, https otherwise), or about:blank. Nothing else opens.

/// Whether a host is this computer or the local network, as written (no name is resolved).
#[must_use]
pub fn local_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return inner.parse::<std::net::Ipv6Addr>().is_ok_and(|address| {
            let first = address.segments()[0];
            address.is_loopback()
                || address.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || address.to_ipv4_mapped().is_some_and(local_ipv4)
        });
    }
    host.parse::<std::net::Ipv4Addr>().is_ok_and(local_ipv4)
}

/// Whether a host is this computer itself (loopback), as written: not another machine on the
/// local network, such as a router or a NAS.
#[must_use]
pub fn this_computer(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return inner.parse::<std::net::Ipv6Addr>().is_ok_and(|address| {
            address.is_loopback() || address.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        });
    }
    host.parse::<std::net::Ipv4Addr>().is_ok_and(|address| address.is_loopback())
}

fn local_ipv4(address: std::net::Ipv4Addr) -> bool {
    let [a, b, ..] = address.octets();
    address.is_loopback()
        || a == 0
        || address.is_private()
        || address.is_link_local()
        || (a == 100 && (64..=127).contains(&b))
}

/// The address a model's `url` opens, or none when it is not a web address.
#[must_use]
pub fn browser_address(text: &str) -> Option<String> {
    let value = text.trim();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return None;
    }
    if value.eq_ignore_ascii_case("about:blank") {
        return Some("about:blank".into());
    }
    let scheme_end = value.find("://");
    if let Some(end) = scheme_end
        && value[..end].chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    {
        let url = url::Url::parse(value).ok()?;
        return matches!(url.scheme(), "http" | "https").then(|| url.to_string());
    }
    // "javascript:…", "mailto:…": a scheme without "//". "localhost:5173" is a host and a port.
    if let Some((scheme, rest)) = value.split_once(':')
        && !scheme.is_empty()
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        && !rest.starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    let guess = url::Url::parse(&format!("http://{value}")).ok()?;
    let host = guess.host_str()?;
    if local_host(host) {
        return Some(guess.to_string());
    }
    if !host.contains('.') {
        return None;
    }
    url::Url::parse(&format!("https://{value}")).ok().map(|url| url.to_string())
}
