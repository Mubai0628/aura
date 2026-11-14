use aura_domain_common::{ErrorCategory, ErrorCode};
use aura_tool_contract::ToolRuntimeErrorCode as R;

/// Called only with a trusted Core refusal; model arguments are never a notice code.
pub(super) fn classification(message: &str) -> (ErrorCode, ErrorCategory) {
    if message.starts_with("http.model_authentication_forbidden:") {
        (R::HttpModelAuthenticationForbidden.error_code(), ErrorCategory::Validation)
    } else if message.starts_with("network.control_endpoint_blocked:") {
        (R::HttpControlEndpointDenied.error_code(), ErrorCategory::Permission)
    } else if message.starts_with("network.timeout:") {
        // The tool is there; the target took too long.
        (R::Timeout.error_code(), ErrorCategory::Transport)
    } else if message.starts_with("network.request_refused:") {
        (R::HttpGatewayFailed.error_code(), ErrorCategory::Unavailable)
    } else if message.starts_with("approval.expired:") {
        // Nobody answered in time; the user did not refuse.
        (aura_tool_contract::ToolApprovalErrorCode::Expired.error_code(), ErrorCategory::Cancelled)
    } else if message.starts_with("approval.denied:") {
        (aura_tool_contract::ToolApprovalErrorCode::Denied.error_code(), ErrorCategory::Permission)
    } else if message.starts_with("approval.invalidated:") {
        (
            aura_tool_contract::ToolApprovalErrorCode::Invalidated.error_code(),
            ErrorCategory::Cancelled,
        )
    } else if message.starts_with("turn.stopped:")
        || message.starts_with("shell.stopped_before_start:")
    {
        // The turn stopped before the call started; the tool itself was fine.
        (R::DispatchFailed.error_code(), ErrorCategory::Cancelled)
    } else if message.starts_with("permission.") || message.starts_with("agents.denied:") {
        // The permission policy, or a sub-agent's main agent answering for it, refused the call
        // before it ran; the message names the rule or the main agent's reason.
        (R::DispatchFailed.error_code(), ErrorCategory::Permission)
    } else if message.starts_with("runner.core_binding_mismatch:") {
        (
            ErrorCode::new("runner.core_binding_mismatch").expect("fixed existing runner code"),
            ErrorCategory::Unavailable,
        )
    } else {
        // Core refused the call before any backend ran it: a sub-agent's refusal, a desktop
        // notice, a dispatch refusal.
        (R::DispatchFailed.error_code(), ErrorCategory::Conflict)
    }
}
