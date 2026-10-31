//! Field-scoped proxy projection and recovery; never reads credentials.
use aura_config::ConfigModule;
use aura_tool_contract::*;

pub(super) fn proxy_view(
    document: &aura_config::ModuleConfigDocument,
) -> (Option<ToolNetworkProxyV1>, Option<String>, Option<AgentNetworkProxyDraftV1>) {
    use aura_config::{ConfigValidationError, NetworkProxyConfigState};
    let draft = document
        .values
        .get(aura_config::NETWORK_PROXY_FIELD)
        .and_then(aura_config::network_proxy_draft_from_value);
    match aura_config::network_proxy_state_from_document(document) {
        NetworkProxyConfigState::Disabled => (None, None, None),
        NetworkProxyConfigState::Configured(proxy) => (Some(proxy), None, draft),
        NetworkProxyConfigState::Unavailable(error) => {
            let message = match error {
                ConfigValidationError::Semantic(message) => message,
                other => other.to_string(),
            };
            (None, Some(message), draft)
        }
    }
}

pub(super) fn proxy_unchanged(
    settings: &aura_config::SettingsFacade,
    edit: &Option<Option<ToolNetworkProxyV1>>,
) -> anyhow::Result<bool> {
    let Some(next) = edit else {
        return Ok(true);
    };
    let snapshot = settings.snapshot(ConfigModule::Tools)?;
    let current = snapshot.document.values.get(aura_config::NETWORK_PROXY_FIELD);
    Ok(match next {
        None => current.is_none(),
        Some(proxy) => current == Some(&serde_json::to_value(proxy).expect("proxy json")),
    })
}

pub(super) fn proxy_recovery(
    settings: &aura_config::SettingsFacade,
) -> anyhow::Result<Option<AgentNetworkProxyRecoveryPreviewV1>> {
    Ok(settings.previous_valid_network_proxy()?.map(|preview| AgentNetworkProxyRecoveryPreviewV1 {
        expected_revision: aura_domain_common::Revision::new(preview.expected_revision),
        source_revision: aura_domain_common::Revision::new(preview.source_revision),
        proxy: preview.proxy,
    }))
}
