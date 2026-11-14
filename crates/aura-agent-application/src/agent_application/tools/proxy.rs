//! Field-scoped proxy projection and recovery; never reads credentials.
use aura_config::{ConfigModule, NetworkProxyConfigState};
use aura_tool_contract::*;

/// The proxy part of the settings view.
pub(super) struct ProxyView {
    /// The proxy typed in, as stored. Model requests and tools take it while it is switched on.
    pub own: Option<ToolNetworkProxyV1>,
    pub diagnostic: Option<String>,
    pub draft: Option<AgentNetworkProxyDraftV1>,
    /// What Windows names, followed while the typed-in proxy is switched off.
    pub system: SystemNetworkProxyV1,
}

pub(super) fn proxy_view(
    settings: &aura_config::SettingsFacade,
    document: &aura_config::ModuleConfigDocument,
) -> ProxyView {
    use aura_config::ConfigValidationError;
    let draft = document
        .values
        .get(aura_config::NETWORK_PROXY_FIELD)
        .and_then(aura_config::network_proxy_draft_from_value);
    let (own, diagnostic, draft) = match aura_config::network_proxy_state_from_document(document) {
        NetworkProxyConfigState::Disabled => (None, None, None),
        NetworkProxyConfigState::Configured(proxy) => (Some(proxy), None, draft),
        NetworkProxyConfigState::Unavailable(error) => {
            let message = match error {
                ConfigValidationError::Semantic(message) => message,
                other => other.to_string(),
            };
            (None, Some(message), draft)
        }
    };
    ProxyView { own, diagnostic, draft, system: settings.system_proxy().view() }
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
