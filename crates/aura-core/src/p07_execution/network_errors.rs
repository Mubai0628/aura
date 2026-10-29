pub(super) fn safe_network_gateway_error_code(
    error: &aura_network_control::NetworkControlError,
) -> &'static str {
    use aura_network_control::NetworkControlError;

    match error {
        NetworkControlError::UserProxyFailure(failure) => failure.kind.code(),
        NetworkControlError::RoutedTransport { error, .. } => {
            safe_network_gateway_error_code(error)
        }
        NetworkControlError::InvalidUrl => "network.invalid_url",
        NetworkControlError::UrlCredentialsForbidden => "network.url_credentials_forbidden",
        NetworkControlError::SchemeBlocked => "network.scheme_blocked",
        NetworkControlError::InvalidMethod => "network.invalid_method",
        NetworkControlError::DestinationBlocked => "network.destination_blocked",
        NetworkControlError::ControlEndpointBlocked => "network.control_endpoint_blocked",
        NetworkControlError::PrivateNetworkApprovalRequired => {
            "network.private_network_approval_required"
        }
        NetworkControlError::DataEgressApprovalRequired => "network.data_egress_approval_required",
        NetworkControlError::SecretDetected { .. } => "network.secret_detected",
        NetworkControlError::CredentialMaterialForbidden => "network.credential_material_forbidden",
        NetworkControlError::GrantExpired => "network.grant_expired",
        NetworkControlError::GrantNotApplicable => "network.grant_not_applicable",
        NetworkControlError::CoreGrantAlreadyConsumed => "network.core_grant_already_consumed",
        NetworkControlError::CoreGrantBindingMismatch => "network.core_grant_binding_mismatch",
        NetworkControlError::ProviderTransportAuthorityContextMismatch => {
            "provider_transport.authority_context_mismatch"
        }
        NetworkControlError::RedirectBlocked => "network.redirect_blocked",
        NetworkControlError::RedirectLimitExceeded => "network.redirect_limit_exceeded",
        NetworkControlError::RateLimited => "network.rate_limited",
        NetworkControlError::UploadLimitExceeded => "network.upload_limit_exceeded",
        NetworkControlError::DownloadLimitExceeded => "network.download_limit_exceeded",
        NetworkControlError::ContentEncodingUnsupported => "transport.content_encoding_unsupported",
        NetworkControlError::DecompressionFailed => "transport.decompression_failed",
        NetworkControlError::ResponseTooLarge => "transport.response_too_large",
        NetworkControlError::Timeout => "network.timeout",
        NetworkControlError::ProxyUnauthorized => "network.proxy_unauthorized",
        NetworkControlError::ProxyProtocol => "network.proxy_protocol_error",
        NetworkControlError::TlsServerNameMismatch => "network.tls_server_name_mismatch",
        NetworkControlError::ProxyInterrupted => "network.proxy_interrupted",
        NetworkControlError::CredentialBindingMismatch => "network.credential_binding_mismatch",
        NetworkControlError::CredentialUnavailable => "network.credential_unavailable",
        NetworkControlError::CredentialReflectionDetected { .. } => {
            "network.credential_reflection_detected"
        }
        NetworkControlError::CredentialReflectionScanIncomplete { .. } => {
            "network.credential_reflection_scan_incomplete"
        }
        NetworkControlError::Transport(reason) => {
            aura_network_control::connect_failure::code(reason)
        }
        NetworkControlError::QuarantineIo(_) => "network.quarantine_io_failed",
        NetworkControlError::QuarantineIntegrity => "network.quarantine_integrity_failed",
        NetworkControlError::QuarantineNotFound => "network.quarantine_not_found",
        NetworkControlError::QuarantineReleaseNotApproved => {
            "network.quarantine_release_not_approved"
        }
        NetworkControlError::Persistence(_) => "network.persistence_failed",
    }
}
