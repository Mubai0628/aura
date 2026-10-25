//! Whether an operation is the built-in browser's, as the policy reads it.
use crate::policy::PolicyInput;
use aura_execution_contract::OperationNetwork;
use aura_security_contract::{NetworkCallerKind, ResourceDescriptor, built_in_browser};

/// A built-in browser tool in its exact shape, from the browser's own caller. Opening a site
/// declares the browser's site-policy network and names the target; a page action declares the
/// site-policy network or none; nothing else may borrow either.
pub(crate) fn shape(input: &PolicyInput<'_>) -> bool {
    let c = &input.operation.content;
    let target = c.resources.iter().any(|r| matches!(r, ResourceDescriptor::NetworkOrigin { .. }));
    input.network_caller == NetworkCallerKind::AuraBrowser
        && built_in_browser(&c.source, &c.action_kind, &c.capabilities, &c.resources)
        && match c.network {
            OperationNetwork::BrowserSitePolicy => true,
            OperationNetwork::None => !target,
            OperationNetwork::ExactOrigins | OperationNetwork::BroadProcess => false,
        }
}
