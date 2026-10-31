use anyhow::{Result, ensure};
use aura_execution_contract::OperationContent;
use aura_network_control::GatewayRequest;

/// Only Core adds these fields, after the model DTO and its deny-unknown schema passed.
pub(crate) fn model_action(action: &serde_json::Value) -> serde_json::Value {
    let mut action = action.clone();
    if let Some(fields) = action.as_object_mut() {
        fields.remove("network_request_hash");
        fields.remove("download_target");
        fields.remove(crate::controlled_web::web_service::WEB_SERVICE_FIELD);
    }
    action
}

pub(crate) fn gateway_request(content: &OperationContent) -> Result<GatewayRequest> {
    if aura_security_contract::package_http(
        &content.source,
        &content.action_kind,
        &content.capabilities,
        &content.resources,
    ) {
        return super::super::package_tools::http::gateway_request(content);
    }
    let action = model_action(&content.action);
    // A call planned for DeepSeek's search or fetch.
    if matches!(content.action_kind.as_str(), "web.search" | "web.fetch")
        && let Some(service) = crate::controlled_web::web_service::frozen(&content.action)?
    {
        let url = crate::controlled_web::web_service::service_url(&service, &content.action_kind)?;
        let body = crate::controlled_web::web_service::request_body(&content.action_kind, &action)?;
        return aura_net_tools::web_service_request(
            url.as_str(),
            &body,
            content.identity.clone(),
            8 * 1024 * 1024,
        );
    }
    match content.action_kind.as_str() {
        "http.request" => {
            aura_net_tools::http_request(&serde_json::from_value(action)?, content.identity.clone())
        }
        "http.download" => aura_net_tools::download_request(
            &serde_json::from_value(action)?,
            content.identity.clone(),
        ),
        "web.fetch" => {
            let input: aura_web_domain::WebFetchInput = serde_json::from_value(action)?;
            aura_net_tools::web_request(
                &input.url,
                content.identity.clone(),
                u64::from(input.max_bytes),
            )
        }
        "web.search" => {
            let input: aura_web_domain::WebSearchInput = serde_json::from_value(action)?;
            let url = aura_web_runtime::search_url(&input.query, input.max_results)?;
            aura_net_tools::web_request(url.as_str(), content.identity.clone(), 2 * 1024 * 1024)
        }
        _ => anyhow::bail!("http.unsupported_action"),
    }
}

pub(crate) fn bind(
    persistence: &aura_persistence::PersistenceCoordinator,
    content: &mut OperationContent,
) -> Result<()> {
    let request = gateway_request(content)?;
    let binding = if content.action_kind == "http.request" {
        super::credential(persistence, &content.action, &content.identity)?
            .map(|(usage, reference, _)| (usage, reference))
    } else if let Some(service) = crate::controlled_web::web_service::frozen(&content.action)? {
        // The DeepSeek connection's own key.
        crate::controlled_web::web_service::credential_use(&service)
    } else {
        None
    };
    content.credentials =
        binding.as_ref().map(|(_, reference)| vec![reference.clone()]).unwrap_or_default();
    let hash = aura_network_control::ordinary_tools::ordinary_request_hash(
        &request,
        binding.as_ref().map(|(value, _)| value),
    )?;
    ensure!(content.action.is_object(), "http.action_object_required");
    content.action["network_request_hash"] = serde_json::to_value(hash)?;
    Ok(())
}
