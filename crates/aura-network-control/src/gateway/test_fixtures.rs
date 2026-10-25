//! Test gateways install DNS and TLS fixtures after startup. Production builds compile this file
//! with both methods omitted.

#[cfg(feature = "test-fixture-dns")]
impl super::CoreHttpClientGateway {
    #[doc(hidden)]
    pub fn install_test_fixture_dns(
        &self,
        records: Vec<crate::fixture_dns::TestFixtureDnsRecord>,
    ) -> Result<(), crate::NetworkControlError> {
        self.options.install_test_fixture_dns(records)
    }
}

#[cfg(feature = "test-fixture-tls")]
impl super::CoreHttpClientGateway {
    #[doc(hidden)]
    pub fn install_test_fixture_tls(&self, root_certificate_pem: Vec<u8>) {
        self.options.install_test_fixture_tls(root_certificate_pem);
    }
}
