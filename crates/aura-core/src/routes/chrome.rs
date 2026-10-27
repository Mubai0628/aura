//! 设置 → 网络与浏览器 → Chrome: the switch, a pairing code and forgetting the paired extension.
use crate::chrome_control::ChromeControl;
use aura_chrome_domain::ChromeSetEnabledRequest;
use aura_domain_common::CommandInvocationId;
use serde_json::Value;

pub(crate) async fn dispatch(
    chrome: &ChromeControl,
    method: &str,
    params: Value,
    command: Option<CommandInvocationId>,
) -> anyhow::Result<Value> {
    if !matches!(method, "chrome.status" | "chrome.shares") {
        anyhow::ensure!(command.is_some(), "ipc.command_invocation_id_required");
    }
    match method {
        "chrome.status" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            Ok(serde_json::to_value(chrome.status())?)
        }
        "chrome.set_enabled" => {
            let input: ChromeSetEnabledRequest = serde_json::from_value(params)?;
            let status = chrome.set_enabled(input.expected_revision, input.enabled).await?;
            Ok(serde_json::to_value(status)?)
        }
        "chrome.pair.begin" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            Ok(serde_json::to_value(chrome.begin_pairing()?)?)
        }
        "chrome.revoke" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            Ok(serde_json::to_value(chrome.revoke().await?)?)
        }
        // The user's Stop on a conversation's line: its shares end and, when it holds Chrome, its
        // commands still out fail; other conversations' shares go on.
        "chrome.stop" => {
            let input: aura_chrome_domain::ChromeSessionRequest = serde_json::from_value(params)?;
            chrome.stop_session(input.session_id);
            Ok(serde_json::to_value(chrome.session_shares(input.session_id))?)
        }
        "chrome.shares" => {
            let input: aura_chrome_domain::ChromeSessionRequest = serde_json::from_value(params)?;
            Ok(serde_json::to_value(chrome.session_shares(input.session_id))?)
        }
        // The user's 完成 in Aura for a tab the model handed them.
        "chrome.assist.finish" => {
            let input: aura_chrome_domain::ChromeSessionRequest = serde_json::from_value(params)?;
            chrome.finish_assist(input.session_id)?;
            Ok(serde_json::to_value(chrome.session_shares(input.session_id))?)
        }
        _ => anyhow::bail!("chrome.unknown_method"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// The settings page reaches Chrome control through Core's own route table.
    #[tokio::test]
    async fn the_settings_page_reads_chrome_control_and_cannot_pair_while_it_is_off() {
        let host = crate::r03_tests::Fixture::new();
        let runtime = host.start().await;
        let status = crate::r03_tests::success(&runtime, "chrome.status", json!({})).await;
        assert_eq!(status["enabled"], json!(false));
        assert_eq!(status["connection"], json!("off"));
        assert_eq!(status["paired"], json!(null));
        let refused = crate::r03_tests::request(&runtime, "chrome.pair.begin", json!({})).await;
        assert!(refused.to_string().contains("chrome.off"), "{refused}");
    }
}
