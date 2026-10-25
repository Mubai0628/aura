use crate::{NetworkControlError, NetworkControlOptions};
use std::sync::Arc;
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, crypto::CryptoProvider},
};

pub(super) fn connector(
    options: &NetworkControlOptions,
) -> Result<TlsConnector, NetworkControlError> {
    let provider = CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()));
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|_| NetworkControlError::Transport("TLS configuration failed".into()))?;
    #[cfg(feature = "test-fixture-tls")]
    let verifier = if let Some(fixture) = options.test_fixture_tls() {
        use tokio_rustls::rustls::pki_types::{CertificateDer, pem::PemObject};
        let roots = CertificateDer::pem_slice_iter(&fixture.root_certificate_pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| NetworkControlError::Transport("fixture CA is invalid".into()))?;
        rustls_platform_verifier::Verifier::new_with_extra_roots(roots, provider)
    } else {
        rustls_platform_verifier::Verifier::new(provider)
    };
    #[cfg(not(feature = "test-fixture-tls"))]
    let verifier = rustls_platform_verifier::Verifier::new(provider);
    let _ = options;
    let verifier = verifier
        .map_err(|_| NetworkControlError::Transport("TLS verifier construction failed".into()))?;
    let mut config = builder
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    // Same platform trust and hostname verification as reqwest. Fixture roots never relax names.
    Ok(TlsConnector::from(Arc::new(config)))
}
