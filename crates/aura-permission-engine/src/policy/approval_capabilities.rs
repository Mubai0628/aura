use super::*;

pub(super) fn requested_capabilities(input: &PolicyInput<'_>) -> Vec<ResolvedCapability> {
    let c = &input.operation.content;
    let protected = c.resources.iter().any(|r| {
        matches!(
            r,
            ResourceDescriptor::SystemPath { .. } | ResourceDescriptor::SensitiveUserPath { .. }
        )
    });
    let missing = c
        .capabilities
        .iter()
        .copied()
        .filter(|capability| {
            !input.verified_external_capabilities.contains(capability)
                && (protected
                    || !git::mode_permits(
                        input,
                        &[*capability],
                        c.network != OperationNetwork::None,
                        c.action_kind.starts_with("web."),
                    ))
        })
        .collect::<Vec<_>>();
    if missing.is_empty() { c.capabilities.clone() } else { missing }
}
