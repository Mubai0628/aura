//! Pins the e2e Core at a local HTTPS target. Production builds omit this module.
//! The desktop test writes the file and sets `AURA_L01_NETWORK_FIXTURE` before launch.
use aura_network_control::{CoreHttpClientGateway, TestFixtureDnsRecord};
use std::net::IpAddr;

pub(crate) fn install(gateway: &CoreHttpClientGateway) {
    let Ok(path) = std::env::var("AURA_L01_NETWORK_FIXTURE") else {
        return;
    };
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    let Some(records) = value.get("records").and_then(serde_json::Value::as_array) else {
        return;
    };
    let parsed = records.iter().filter_map(record).collect::<Vec<_>>();
    if parsed.is_empty() || gateway.install_test_fixture_dns(parsed).is_err() {
        return;
    }
    if let Some(pem) = value.get("certificate_pem").and_then(serde_json::Value::as_str) {
        gateway.install_test_fixture_tls(pem.as_bytes().to_vec());
    }
}

fn record(value: &serde_json::Value) -> Option<TestFixtureDnsRecord> {
    let host = value.get("host")?.as_str()?.to_owned();
    Some(TestFixtureDnsRecord {
        host,
        classified_addresses: addresses(value.get("classified")?)?,
        connect_addresses: addresses(value.get("connect")?)?,
    })
}

fn addresses(value: &serde_json::Value) -> Option<Vec<IpAddr>> {
    let values = value.as_array()?;
    let addresses =
        values.iter().filter_map(|value| value.as_str()?.parse().ok()).collect::<Vec<_>>();
    (addresses.len() == values.len() && !addresses.is_empty()).then_some(addresses)
}
