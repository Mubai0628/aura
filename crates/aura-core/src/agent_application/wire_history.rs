//! Provider IDs are reusable only within their exact Endpoint domain and while
//! unique. Rebinding changes the outbound protocol alias, never Core Tool IDs.
use aura_model_domain::*;
pub(super) fn bind(
    mut values: Vec<ProviderToolCallWireBinding>,
    current: &ProviderToolCallReuseDomain,
) -> Vec<ProviderToolCallWireBinding> {
    let generated = values
        .iter()
        .map(|b| ProviderWireToolCallId::from_tool_call_id(b.tool_call_id).to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let mut counts = std::collections::BTreeMap::new();
    for b in &values {
        *counts.entry(b.wire_identity.wire_id().to_owned()).or_insert(0usize) += 1;
    }
    for b in &mut values {
        let reusable = matches!(&b.wire_identity,ProviderToolCallWireIdentity::ProviderOpaque{reuse_domain,..} if reuse_domain==current)
            && counts[b.wire_identity.wire_id()] == 1
            && !generated.contains(b.wire_identity.wire_id());
        if !reusable {
            b.wire_identity = ProviderToolCallWireIdentity::AuraGenerated {
                provider_wire_tool_call_id: ProviderWireToolCallId::from_tool_call_id(
                    b.tool_call_id,
                ),
            };
        }
    }
    values
}
