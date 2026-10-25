use anyhow::{Result, ensure};
use aura_domain_common::jcs::canonical_sha256;
use aura_permission_domain::{ToolCredentialPlacementV1 as Placement, ToolCredentialUseV1};
use aura_security_contract::{CredentialUseRef, InvocationIdentity, SecuritySourceRef};
use aura_tool_domain::{
    ToolCredentialManagementTargetV1, ToolCredentialPlacementV1, ToolCredentialStateV1,
};

pub(crate) fn credential(
    persistence: &aura_persistence::PersistenceCoordinator,
    action: &serde_json::Value,
    identity: &InvocationIdentity,
) -> Result<Option<(ToolCredentialUseV1, CredentialUseRef, ToolCredentialManagementTargetV1)>> {
    let input: aura_web_domain::HttpRequestInput =
        serde_json::from_value(super::model_action(action))?;
    let Some(selector) = input.credential else { return Ok(None) };
    let entry = aura_tool_registry::agent_release::release()?
        .entry("http.request")
        .ok_or_else(|| anyhow::anyhow!("http.unavailable"))?;
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
        .ok_or_else(|| anyhow::anyhow!("http.credential_unbound"))?;
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
    ensure!(matches.len() == 1, "http.credential_unbound_or_ambiguous");
    let record = &matches[0];
    record.validate()?;
    let origin = aura_net_tools::request_url(&input.url)?.origin().ascii_serialization();
    ensure!(record.allowed_origins.binary_search(&origin).is_ok(), "http.credential_origin_denied");
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

pub(crate) fn load_secret(
    persistence: &aura_persistence::PersistenceCoordinator,
    content: &aura_execution_contract::OperationContent,
) -> Result<Option<crate::tool_credential_service::execution::HttpSecret>> {
    if content.action_kind != "http.request" {
        return Ok(None);
    }
    let Some((usage, bound, target)) = credential(persistence, &content.action, &content.identity)?
    else {
        ensure!(content.credentials.is_empty(), "http.credential_changed");
        return Ok(None);
    };
    ensure!(content.credentials == [bound], "http.credential_changed");
    let url = aura_net_tools::request_url(
        content.action["url"].as_str().ok_or_else(|| anyhow::anyhow!("http.url_required"))?,
    )?;
    let origin = aura_permission_domain::NormalizedOrigin {
        scheme: url.scheme().into(),
        host: url.host_str().ok_or_else(|| anyhow::anyhow!("http.host"))?.into(),
        port: url.port_or_known_default().ok_or_else(|| anyhow::anyhow!("http.port"))?,
    };
    Ok(Some(crate::tool_credential_service::execution::HttpSecret::load_managed(
        persistence,
        target,
        usage,
        origin,
    )?))
}
