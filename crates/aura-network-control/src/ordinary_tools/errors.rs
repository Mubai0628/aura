use crate::NetworkControlError as E;

/// Model-visible diagnostics never include reqwest's URL, headers, body or OS details.
pub fn safe_transport_code(error: &E) -> &'static str {
    match error {
        E::UserProxyFailure(failure) => failure.kind.code(),
        E::RoutedTransport { error, .. } => safe_transport_code(error),
        E::InvalidUrl | E::UrlCredentialsForbidden | E::SchemeBlocked | E::InvalidMethod => {
            "http.invalid_url_or_method"
        }
        E::ControlEndpointBlocked => "network.control_endpoint_blocked",
        E::SecretDetected { .. } => "http.model_authentication_forbidden",
        E::Timeout => "network.timeout",
        E::ResponseTooLarge | E::DownloadLimitExceeded | E::UploadLimitExceeded => {
            "network.response_too_large"
        }
        E::RedirectBlocked | E::RedirectLimitExceeded => "network.redirect_blocked",
        E::CredentialReflectionDetected { .. } | E::CredentialReflectionScanIncomplete { .. } => {
            "network.credential_reflection_blocked"
        }
        E::CredentialBindingMismatch | E::CredentialUnavailable => "http.credential_unavailable",
        E::CoreGrantBindingMismatch
        | E::CoreGrantAlreadyConsumed
        | E::GrantExpired
        | E::GrantNotApplicable => "http.authority_changed",
        E::RateLimited => "network.rate_limited",
        E::Transport(_) => "network.transport_failed",
        _ => "network.request_refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_supplied_authentication_has_a_dedicated_transport_code() {
        let error = E::SecretDetected { location: "request.body".into(), kind: "api_token".into() };
        assert_eq!(safe_transport_code(&error), "http.model_authentication_forbidden");
    }
}
