use anyhow::{Result, ensure};
use aura_domain_common::jcs::canonical_sha256;
use aura_permission_domain::{ToolCredentialPlacementV1 as Placement, ToolCredentialUseV1};
use aura_security_contract::{CredentialUseRef, InvocationIdentity, SecuritySourceRef};
use aura_tool_domain::{
    ToolCredentialManagementTargetV1, ToolCredentialPlacementV1, ToolCredentialStateV1,
};
use aura_tool_service::tool_credential_service;

pub(crate) fn credential(
    persistence: &aura_persistence::PersistenceCoordinator,
    action: &serde_json::Value,
    identity: &InvocationIdentity,
) -> Result<Option<(ToolCredentialUseV1, CredentialUseRef, ToolCredentialManagementTargetV1)>> {
    let input: aura_web_domain::HttpRequestInput =
        serde_json::from_value(super::model_action(action))?;
    let Some(selector) = input.credential else { return Ok(None) };
    let entry = aura_tool_registry::agent_release::release()?.entry("http.request").ok_or_else(
        || {
            anyhow::anyhow!(
                "http.unavailable: 这个版本的 Aura 里找不到 http.request 的凭据设置，请求没有发出。可以不带 credential 再调用。"
            )
        },
    )?;
    let mut target = ToolCredentialManagementTargetV1 {
        tool_instance_id: entry.instance.tool_instance_id,
        expected_instance_revision: entry.instance.entity_revision,
        credential_slot_key: aura_tool_contract::CredentialSlotKey::new("api_key")?,
        placement: ToolCredentialPlacementV1::HttpBearerAuthorization,
        workspace_id: identity.workspace_id,
        session_id: identity.session_id,
    };
    let authority = persistence.tool_authority_snapshot(
        identity.workspace_id,
        identity.session_id,
        identity.turn_task_id,
    )?;
    if let Some(instance) =
        authority.instances.iter().find(|i| i.tool_instance_id == target.tool_instance_id)
    {
        target.expected_instance_revision = instance.entity_revision;
    }
    if let Some(reason) = persistence.builtin_credential_rebind_reason(target.tool_instance_id)? {
        let message = match reason.as_str() {
            "credential_slot_changed" => {
                "凭据要求已变化。请在工具设置中重新绑定凭据，并核对允许网站。"
            }
            _ => "凭据绑定需要核对。请在工具设置中重新绑定凭据，并核对允许网站。",
        };
        return Err(anyhow::anyhow!("http.credential_rebind_required: {message}"));
    }
    let binding = authority
        .instance_credential_bindings
        .iter()
        .find(|binding| {
            binding.tool_instance_id == target.tool_instance_id
                && binding.credential_slot_key == target.credential_slot_key
                && binding.placement == target.placement
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "http.credential_unbound: http.request 还没有绑定凭据，请求没有发出。请用户在工具设置里给 http.request 绑定凭据并设好允许网站，或者不带 credential 再调用。"
            )
        })?;
    binding.validate()?;
    // A display name may select only the user's explicit slot binding. Creating a
    // compatible credential never implicitly binds it to model-selected requests.
    let matches = persistence
        .list_managed_tool_credentials_v1(target.clone())?
        .into_iter()
        .filter(|record| {
            record.state == ToolCredentialStateV1::Configured
                && record.credential_id == binding.credential_id
                && record.credential_revision == binding.credential_revision
                && (selector == "api_key" || selector == record.display_name)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        let why = if selector == "api_key" {
            "绑定的凭据现在不可用（可能已被删除或需要重新填写）".to_owned()
        } else {
            format!("绑定的凭据不叫 {}，或者现在不可用", shown(&selector))
        };
        anyhow::bail!(
            "http.credential_unbound_or_ambiguous: {why}，请求没有发出。credential 写 api_key 会用绑定的那个凭据；也可以请用户在工具设置里核对它。"
        );
    }
    let record = &matches[0];
    record.validate()?;
    let origin = aura_net_tools::request_url(&input.url)?.origin().ascii_serialization();
    ensure!(
        record.allowed_origins.binary_search(&origin).is_ok(),
        "http.credential_origin_denied: 绑定的凭据不允许发往 {}，请求没有发出。请用户在工具设置里把这个网站加进凭据的允许网站，或者不带 credential 再调用。",
        shown(&origin)
    );
    let usage = ToolCredentialUseV1 {
        credential_id: record.credential_id,
        credential_revision: record.credential_revision,
        slot_key: target.credential_slot_key.clone(),
        placement: Placement::BearerAuthorization,
    };
    let bound = CredentialUseRef {
        credential_id: record.credential_id,
        credential_revision: record.credential_revision,
        authority: SecuritySourceRef::SystemComponent {
            component_id: "tool-credential-authority-v1".into(),
        },
        use_fingerprint: canonical_sha256(&(record, &target, binding))?,
    };
    Ok(Some((usage, bound, target)))
}

const CREDENTIAL_CHANGED: &str = "http.credential_changed: 这次调用批准之后，http.request 的凭据绑定变了，请求没有发出。可以重新调用，会按现在的绑定再确认。";

/// A name the model wrote or an origin, as a line repeats it: at most 80 characters, without
/// control characters or braces.
fn shown(text: &str) -> String {
    let mut shown: String = text
        .chars()
        .take(80)
        .map(|c| if c.is_control() || c == '{' || c == '}' { ' ' } else { c })
        .collect();
    if text.chars().count() > 80 {
        shown.push('…');
    }
    shown
}

pub(crate) fn load_secret(
    persistence: &aura_persistence::PersistenceCoordinator,
    content: &aura_execution_contract::OperationContent,
) -> Result<Option<tool_credential_service::execution::HttpSecret>> {
    if content.action_kind != "http.request" {
        return Ok(None);
    }
    let Some((usage, bound, target)) = credential(persistence, &content.action, &content.identity)?
    else {
        ensure!(content.credentials.is_empty(), CREDENTIAL_CHANGED);
        return Ok(None);
    };
    ensure!(content.credentials == [bound], CREDENTIAL_CHANGED);
    let url = aura_net_tools::request_url(content.action["url"].as_str().ok_or_else(|| {
        anyhow::anyhow!(
            "http.url_required: 没有给出 url，请求没有发出。请在 url 里写出完整的 http:// 或 https:// 地址。"
        )
    })?)?;
    // `request_url` already refused an address without a host; a scheme without a known port
    // is not http or https.
    let origin = aura_permission_domain::NormalizedOrigin {
        scheme: url.scheme().into(),
        host: url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("http.host: 地址里没有主机名，请求没有发出。"))?
            .into(),
        port: url.port_or_known_default().ok_or_else(|| {
            anyhow::anyhow!("http.port: 地址的端口不明，请求没有发出。请在地址里写出端口。")
        })?,
    };
    Ok(Some(tool_credential_service::execution::HttpSecret::load_managed(
        persistence,
        target,
        usage,
        origin,
    )?))
}

#[cfg(test)]
mod tests {
    /// A changed binding keeps its code and says nothing was sent; a repeated name is bounded.
    #[test]
    fn a_credential_refusal_is_one_bounded_line() {
        assert!(super::CREDENTIAL_CHANGED.starts_with("http.credential_changed: "));
        assert!(super::CREDENTIAL_CHANGED.contains("请求没有发出"));
        let long = super::shown(&format!("{{a}}{}", "x".repeat(100)));
        assert!(long.chars().count() == 81 && !long.contains(['{', '}']), "{long}");
    }
}
