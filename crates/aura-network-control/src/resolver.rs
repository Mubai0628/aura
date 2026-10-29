use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use aura_domain_common::UtcTimestamp;
use aura_permission_domain::NormalizedOrigin;
use aura_security_contract::{NetworkDestinationClass, ResolvedNetworkDestination};
use tokio::{net::lookup_host, time::timeout};
use url::{Host, Url};

use crate::NetworkControlError;

#[derive(Clone, Copy, Debug)]
pub struct ProxyDnsPolicy {
    pub minimum_ttl: Duration,
    pub maximum_ttl: Duration,
}

impl Default for ProxyDnsPolicy {
    fn default() -> Self {
        Self { minimum_ttl: Duration::from_secs(1), maximum_ttl: Duration::from_secs(60) }
    }
}

#[derive(Clone)]
pub struct ProxyDnsResolver {
    policy: ProxyDnsPolicy,
    cache: Arc<Mutex<BTreeMap<String, CachedDnsSet>>>,
}

#[derive(Clone)]
struct CachedDnsSet {
    addresses: Vec<IpAddr>,
    expires_at: std::time::Instant,
    wall_expires_at: UtcTimestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyResolvedAddressSet {
    pub addresses: Vec<IpAddr>,
    pub expires_at: UtcTimestamp,
}

impl ProxyDnsResolver {
    #[must_use]
    pub fn new(policy: ProxyDnsPolicy) -> Self {
        Self { policy, cache: Arc::new(Mutex::new(BTreeMap::new())) }
    }

    pub async fn resolve_compliant(
        &self,
        host: &str,
        allowed_classes: &BTreeSet<NetworkDestinationClass>,
    ) -> Result<Vec<IpAddr>, NetworkControlError> {
        self.resolve_compliant_set(host, allowed_classes).await.map(|set| set.addresses)
    }

    pub async fn resolve_compliant_set(
        &self,
        host: &str,
        allowed_classes: &BTreeSet<NetworkDestinationClass>,
    ) -> Result<ProxyResolvedAddressSet, NetworkControlError> {
        let normalized = host.trim_end_matches('.').to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(NetworkControlError::DestinationBlocked);
        }
        if let Some(cached) = self
            .cache
            .lock()
            .map_err(|_| NetworkControlError::Transport("DNS cache poisoned".to_owned()))?
            .get(&normalized)
            .filter(|entry| entry.expires_at > std::time::Instant::now())
            .cloned()
        {
            return Ok(ProxyResolvedAddressSet {
                addresses: validate_dns_set(cached.addresses, allowed_classes)?,
                expires_at: cached.wall_expires_at,
            });
        }

        let query_host = normalized.clone();
        let records = tokio::task::spawn_blocking(move || {
            aura_windows_platform::resolve_dns_addresses(&query_host)
        })
        .await
        .map_err(|error| NetworkControlError::Transport(error.to_string()))?
        .map_err(|_| host_not_found())?;
        let addresses = records.iter().map(|record| record.address).collect::<BTreeSet<_>>();
        let addresses = validate_dns_set(addresses.into_iter().collect(), allowed_classes)?;
        let ttl = records
            .iter()
            .map(|record| Duration::from_secs(u64::from(record.ttl_seconds)))
            .min()
            .unwrap_or(self.policy.minimum_ttl)
            .max(self.policy.minimum_ttl)
            .min(self.policy.maximum_ttl);
        let wall_expires_at = UtcTimestamp::from_value(
            UtcTimestamp::now().value()
                + time::Duration::seconds(i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX)),
        );
        self.cache
            .lock()
            .map_err(|_| NetworkControlError::Transport("DNS cache poisoned".to_owned()))?
            .insert(
                normalized,
                CachedDnsSet {
                    addresses: addresses.clone(),
                    expires_at: std::time::Instant::now() + ttl,
                    wall_expires_at,
                },
            );
        Ok(ProxyResolvedAddressSet { addresses, expires_at: wall_expires_at })
    }

    pub fn invalidate(&self, host: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(&host.trim_end_matches('.').to_ascii_lowercase());
        }
    }
}

impl Default for ProxyDnsResolver {
    fn default() -> Self {
        Self::new(ProxyDnsPolicy::default())
    }
}

fn validate_dns_set(
    addresses: Vec<IpAddr>,
    allowed_classes: &BTreeSet<NetworkDestinationClass>,
) -> Result<Vec<IpAddr>, NetworkControlError> {
    if addresses.is_empty()
        || allowed_classes.len() != 1
        || !allowed_classes.contains(&NetworkDestinationClass::Public)
    {
        return Err(NetworkControlError::DestinationBlocked);
    }
    for address in &addresses {
        let classes = classify_ip(address);
        if classes.as_slice() != [NetworkDestinationClass::Public] {
            return Err(NetworkControlError::DestinationBlocked);
        }
    }
    Ok(addresses)
}

pub async fn normalize_and_resolve(
    requested_url: &str,
    dns_timeout: Duration,
) -> Result<(Url, NormalizedOrigin, ResolvedNetworkDestination), NetworkControlError> {
    normalize_and_resolve_inner(
        requested_url,
        dns_timeout,
        #[cfg(feature = "test-fixture-dns")]
        None,
    )
    .await
}

pub(crate) async fn normalize_with_options(
    requested_url: &str,
    options: &crate::NetworkControlOptions,
) -> Result<(Url, NormalizedOrigin, ResolvedNetworkDestination), NetworkControlError> {
    #[cfg(feature = "test-fixture-dns")]
    let fixture = options.fixture_dns();
    normalize_and_resolve_inner(
        requested_url,
        options.dns_timeout,
        #[cfg(feature = "test-fixture-dns")]
        fixture.as_ref(),
    )
    .await
}

async fn normalize_and_resolve_inner(
    requested_url: &str,
    dns_timeout: Duration,
    #[cfg(feature = "test-fixture-dns")] fixture: Option<&crate::fixture_dns::TestFixtureDns>,
) -> Result<(Url, NormalizedOrigin, ResolvedNetworkDestination), NetworkControlError> {
    let url = Url::parse(requested_url).map_err(|_| NetworkControlError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(NetworkControlError::SchemeBlocked);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(NetworkControlError::UrlCredentialsForbidden);
    }
    let host = url.host().ok_or(NetworkControlError::InvalidUrl)?;
    let port = url.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
    let normalized_host = match host {
        Host::Domain(value) => value.trim_end_matches('.').to_ascii_lowercase(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => value.to_string(),
    };
    if normalized_host.is_empty() {
        return Err(NetworkControlError::InvalidUrl);
    }
    #[cfg(feature = "test-fixture-dns")]
    let fixture_addresses =
        fixture.map(|fixture| fixture.classified(&normalized_host)).transpose()?;
    let addresses = {
        let resolve = async {
            let addresses = match host {
                Host::Ipv4(value) => vec![IpAddr::V4(value)],
                Host::Ipv6(value) => vec![IpAddr::V6(value)],
                Host::Domain(_) => {
                    let resolved =
                        timeout(dns_timeout, lookup_host((normalized_host.as_str(), port)))
                            .await
                            .map_err(|_| NetworkControlError::Timeout)?
                            .map_err(|_| host_not_found())?;
                    resolved.map(|address| address.ip()).collect::<Vec<_>>()
                }
            };
            Ok::<_, NetworkControlError>(addresses)
        };
        #[cfg(feature = "test-fixture-dns")]
        let addresses =
            if let Some(addresses) = fixture_addresses { addresses } else { resolve.await? };
        #[cfg(not(feature = "test-fixture-dns"))]
        let addresses = resolve.await?;
        addresses
    };
    if addresses.is_empty() {
        return Err(NetworkControlError::DestinationBlocked);
    }
    let addresses = addresses.into_iter().collect::<BTreeSet<_>>();
    let classes = addresses.iter().flat_map(classify_ip).collect::<BTreeSet<_>>();
    let origin =
        NormalizedOrigin { scheme: url.scheme().to_owned(), host: normalized_host.clone(), port };
    let destination = ResolvedNetworkDestination {
        scheme: origin.scheme.clone(),
        host: normalized_host,
        port,
        addresses: addresses.iter().map(ToString::to_string).collect(),
        classes: classes.into_iter().collect(),
    };
    Ok((url, origin, destination))
}

#[must_use]
pub fn classify_ip(address: &IpAddr) -> Vec<NetworkDestinationClass> {
    match address {
        IpAddr::V4(value) => classify_ipv4(*value),
        IpAddr::V6(value) => classify_ipv6(*value),
    }
}

fn classify_ipv4(address: Ipv4Addr) -> Vec<NetworkDestinationClass> {
    let octets = address.octets();
    if address == Ipv4Addr::new(169, 254, 169, 254) {
        return vec![NetworkDestinationClass::LinkLocal, NetworkDestinationClass::Metadata];
    }
    if address.is_loopback() {
        return vec![NetworkDestinationClass::Loopback];
    }
    if address.is_private() {
        return vec![NetworkDestinationClass::Private];
    }
    if address.is_link_local() {
        return vec![NetworkDestinationClass::LinkLocal];
    }
    if address.is_unspecified()
        || address.is_broadcast()
        || address.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && matches!(octets[1], 18 | 19 | 51))
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 240
    {
        vec![NetworkDestinationClass::Reserved]
    } else {
        vec![NetworkDestinationClass::Public]
    }
}

fn classify_ipv6(address: Ipv6Addr) -> Vec<NetworkDestinationClass> {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return classify_ipv4(mapped);
    }
    if address == Ipv6Addr::LOCALHOST {
        return vec![NetworkDestinationClass::Loopback];
    }
    if address == Ipv6Addr::UNSPECIFIED {
        return vec![NetworkDestinationClass::Reserved];
    }
    let segments = address.segments();
    if address == Ipv6Addr::from_str("fd00:ec2::254").expect("constant IPv6")
        || address == Ipv6Addr::from_str("fe80::a9fe:a9fe").expect("constant IPv6")
    {
        return vec![NetworkDestinationClass::Private, NetworkDestinationClass::Metadata];
    }
    if (segments[0] & 0xfe00) == 0xfc00 {
        return vec![NetworkDestinationClass::Private];
    }
    if (segments[0] & 0xffc0) == 0xfe80 {
        return vec![NetworkDestinationClass::LinkLocal];
    }
    if (segments[0] & 0xffc0) == 0xfec0
        || (segments[0] & 0xff00) == 0xff00
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
    {
        return vec![NetworkDestinationClass::Reserved];
    }
    vec![NetworkDestinationClass::Public]
}

/// A name the resolver could not answer for: the model hears that, not the resolver's text.
fn host_not_found() -> NetworkControlError {
    NetworkControlError::Transport(crate::connect_failure::HOST_NOT_FOUND.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct AddressClassificationFixture {
        schema_version: u32,
        cases: Vec<AddressClassificationCase>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct AddressClassificationCase {
        address: IpAddr,
        expected_classes: Vec<NetworkDestinationClass>,
    }

    #[test]
    fn classifies_ipv4_ipv6_and_metadata_variants() {
        assert_eq!(
            classify_ip(&"127.1.2.3".parse().expect("IP")),
            vec![NetworkDestinationClass::Loopback]
        );
        assert_eq!(
            classify_ip(&"10.0.0.1".parse().expect("IP")),
            vec![NetworkDestinationClass::Private]
        );
        assert!(
            classify_ip(&"169.254.169.254".parse().expect("IP"))
                .contains(&NetworkDestinationClass::Metadata)
        );
        assert_eq!(
            classify_ip(&"::ffff:127.0.0.1".parse().expect("IP")),
            vec![NetworkDestinationClass::Loopback]
        );
        assert!(
            classify_ip(&"fd00:ec2::254".parse().expect("IP"))
                .contains(&NetworkDestinationClass::Metadata)
        );
        assert_eq!(
            classify_ip(&"8.8.8.8".parse().expect("IP")),
            vec![NetworkDestinationClass::Public]
        );
    }

    #[test]
    fn stable_address_fixture_matches_ssrf_classification() {
        let fixture: AddressClassificationFixture = serde_json::from_str(include_str!(
            "../../../fixtures/network/address-classification.json"
        ))
        .expect("network address fixture must stay compatible with the canonical contract");
        assert_eq!(fixture.schema_version, 1);

        for case in fixture.cases {
            assert_eq!(
                classify_ip(&case.address),
                case.expected_classes,
                "unexpected destination classes for {}",
                case.address
            );
        }
    }

    #[tokio::test]
    async fn proxy_dns_rejects_direct_non_public_ip_literals() {
        let resolver = ProxyDnsResolver::default();
        let public_only = BTreeSet::from([NetworkDestinationClass::Public]);
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "169.254.169.254",
            "::1",
            "fd00::1",
            "fe80::1",
            "fd00:ec2::254",
        ] {
            assert!(matches!(
                resolver.resolve_compliant(address, &public_only).await,
                Err(NetworkControlError::DestinationBlocked)
            ));
        }
    }

    #[test]
    fn proxy_dns_rejects_mixed_public_and_non_public_answer_sets() {
        let public_only = BTreeSet::from([NetworkDestinationClass::Public]);
        let mixed =
            vec!["8.8.8.8".parse().expect("public IP"), "127.0.0.1".parse().expect("loopback IP")];
        assert!(matches!(
            validate_dns_set(mixed, &public_only),
            Err(NetworkControlError::DestinationBlocked)
        ));
    }

    #[test]
    fn proxy_dns_never_accepts_a_policy_that_names_non_public_classes() {
        let private_only = BTreeSet::from([NetworkDestinationClass::Private]);
        let mixed_policy =
            BTreeSet::from([NetworkDestinationClass::Public, NetworkDestinationClass::Private]);
        let public_address = vec!["8.8.8.8".parse().expect("public IP")];
        assert!(matches!(
            validate_dns_set(public_address.clone(), &private_only),
            Err(NetworkControlError::DestinationBlocked)
        ));
        assert!(matches!(
            validate_dns_set(public_address, &mixed_policy),
            Err(NetworkControlError::DestinationBlocked)
        ));
    }
}
