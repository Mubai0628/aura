//! Process-local test input only. This module is absent from production builds.
use crate::{NetworkControlError, NetworkControlOptions};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
};

#[derive(Clone, Debug)]
pub struct TestFixtureDnsRecord {
    pub host: String,
    pub classified_addresses: Vec<IpAddr>,
    pub connect_addresses: Vec<IpAddr>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TestFixtureDns {
    records: BTreeMap<String, TestFixtureDnsRecord>,
}

impl NetworkControlOptions {
    /// No system DNS is queried while this fixture map is installed. Actual direct
    /// connections are allowed only to explicitly supplied loopback addresses.
    #[doc(hidden)]
    pub fn with_test_fixture_dns(
        self,
        records: Vec<TestFixtureDnsRecord>,
    ) -> Result<Self, NetworkControlError> {
        let mut dns = TestFixtureDns::default();
        for mut record in records {
            record.host = record.host.trim_end_matches('.').to_ascii_lowercase();
            if record.host.is_empty()
                || record.host.parse::<IpAddr>().is_ok_and(|ip| !ip.is_loopback())
                || record.classified_addresses.is_empty()
                || record.connect_addresses.is_empty()
                || record.connect_addresses.iter().any(|ip| !ip.is_loopback())
                || dns.records.contains_key(&record.host)
            {
                return Err(NetworkControlError::DestinationBlocked);
            }
            dns.records.insert(record.host.clone(), record);
        }
        *self.test_fixture_dns.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(dns);
        Ok(self)
    }

    pub(crate) fn fixture_dns(&self) -> Option<TestFixtureDns> {
        self.test_fixture_dns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// Installs records on a gateway that is already running. Production builds omit this module.
    #[doc(hidden)]
    pub fn install_test_fixture_dns(
        &self,
        records: Vec<TestFixtureDnsRecord>,
    ) -> Result<(), NetworkControlError> {
        let mut installed = Self {
            dns_timeout: self.dns_timeout,
            rate_limits: self.rate_limits.clone(),
            test_fixture_dns: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(feature = "test-fixture-tls")]
            test_fixture_tls: std::sync::Arc::clone(&self.test_fixture_tls),
        };
        installed = installed.with_test_fixture_dns(records)?;
        *self.test_fixture_dns.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            installed.fixture_dns();
        Ok(())
    }
}

impl TestFixtureDns {
    pub(crate) fn classified(&self, host: &str) -> Result<Vec<IpAddr>, NetworkControlError> {
        self.records
            .get(host)
            .map(|record| record.classified_addresses.clone())
            .ok_or(NetworkControlError::DestinationBlocked)
    }

    pub(crate) fn pinned(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Vec<SocketAddr>, NetworkControlError> {
        self.records
            .get(host)
            .map(|record| {
                record.connect_addresses.iter().map(|ip| SocketAddr::new(*ip, port)).collect()
            })
            .ok_or(NetworkControlError::DestinationBlocked)
    }
}
