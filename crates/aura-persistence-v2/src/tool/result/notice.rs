use aura_domain_common::{ErrorCategory, ErrorCode};
use aura_tool_contract::ToolRuntimeErrorCode as R;

/// Called only with a trusted Core refusal; model arguments are never a notice code.
pub(super) fn classification(message: &str) -> (ErrorCode, ErrorCategory) {
    if message.starts_with("http.model_authentication_forbidden:") {
        (R::HttpModelAuthenticationForbidden.error_code(), ErrorCategory::Validation)
    } else if message.starts_with("network.control_endpoint_blocked:") {
        (R::HttpControlEndpointDenied.error_code(), ErrorCategory::Permission)
    } else if message.starts_with("network.timeout:") {
        (R::Timeout.error_code(), ErrorCategory::Unavailable)
    } else if message.starts_with("network.request_refused:") {
        (R::HttpGatewayFailed.error_code(), ErrorCategory::Unavailable)
    } else if message.starts_with("tool.dispatch_refused:") {
        // Core refused the call before any backend ran it.
        (R::DispatchFailed.error_code(), ErrorCategory::Conflict)
    } else {
        (
            ErrorCode::new("runner.core_binding_mismatch").expect("fixed existing runner code"),
            ErrorCategory::Unavailable,
        )
    }
}
