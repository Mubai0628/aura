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
        let limit = if content.action_kind == "web.fetch" {
            u64::from(input::<aura_web_domain::WebFetchInput>("web.fetch", action)?.max_bytes)
        } else {
            u64::from(aura_web_domain::MAX_FETCH_BYTES)
        };
        return aura_net_tools::web_service_request(
            url.as_str(),
            &body,
            content.identity.clone(),
            limit,
        );
    }
    let kind = content.action_kind.as_str();
    match kind {
        "http.request" => {
            aura_net_tools::http_request(&input(kind, action)?, content.identity.clone())
        }
        "http.download" => {
            aura_net_tools::download_request(&input(kind, action)?, content.identity.clone())
        }
        "web.fetch" => {
            let input: aura_web_domain::WebFetchInput = input(kind, action)?;
            aura_net_tools::web_request(
                &input.url,
                content.identity.clone(),
                u64::from(input.max_bytes),
            )
        }
        "web.search" => {
            let input: aura_web_domain::WebSearchInput = input(kind, action)?;
            let url = aura_web_runtime::search_url(&input.query, input.max_results)?;
            aura_net_tools::web_request(url.as_str(), content.identity.clone(), 2 * 1024 * 1024)
        }
        _ => {
            anyhow::bail!("http.unsupported_action: Aura 的网络工具不处理这种调用，请求没有发出。")
        }
    }
}

/// The model's arguments as the tool's input. The schema check before the call refuses
/// arguments that do not fit first; serde's words about the field follow Core's sentence, in the
/// language of the family's other lines.
fn input<T: serde::de::DeserializeOwned>(kind: &str, action: serde_json::Value) -> Result<T> {
    serde_json::from_value(action).map_err(|error| {
        let text = error.to_string();
        let mut detail = String::new();
        for c in text.lines().next().unwrap_or_default().chars() {
            if detail.len() + c.len_utf8() > 160 {
                detail.push('…');
                break;
            }
            detail.push(if c.is_control() || c == '{' || c == '}' { ' ' } else { c });
        }
        if kind.starts_with("web.") {
            anyhow::anyhow!(
                "{kind}_input: the arguments do not fit {kind}: {detail}. The call did not run; \
                 correct the arguments and call it again."
            )
        } else {
            anyhow::anyhow!(
                "{kind}_input: 参数和 {kind} 的格式对不上（{detail}），请求没有发出。请按工具说明改好参数再调用。"
            )
        }
    })
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
    ensure!(
        content.action.is_object(),
        "http.action_object_required: 这次调用的参数不是一个对象，请求没有发出。请按工具说明以对象的形式给出参数。"
    );
    content.action["network_request_hash"] = serde_json::to_value(hash)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Arguments that do not fit name the tool and serde's words about the field.
    #[test]
    fn arguments_that_do_not_fit_name_the_field() {
        let fetch = super::input::<aura_web_domain::WebFetchInput>(
            "web.fetch",
            serde_json::json!({"max_bytes": 1}),
        )
        .expect_err("no url")
        .to_string();
        assert!(fetch.starts_with("web.fetch_input: the arguments do not fit web.fetch: "));
        assert!(fetch.contains("url"), "{fetch}");
        let download = super::input::<aura_web_domain::HttpDownloadInput>(
            "http.download",
            serde_json::json!({"url": "https://example.org/a.zip"}),
        )
        .expect_err("no path")
        .to_string();
        assert!(download.starts_with("http.download_input: 参数和 http.download 的格式对不上"));
        assert!(download.contains("path") && download.len() <= 512, "{download}");
    }
}
