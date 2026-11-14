//! What Aura's own pages ask of Chrome control: 设置 → 网络与浏览器 → Chrome (the switch, a pairing
//! code, forgetting the paired extension), a conversation's shared tabs with the user's 完成 and
//! Stop, and the user's tabs for the @ menu with the one handed to a conversation there. A model
//! never reaches them.
use super::ChromeControl;
use aura_chrome_domain::{ChromeSessionRequest, ChromeSetEnabledRequest, ChromeTabHandOverRequest};
use aura_domain_common::CommandInvocationId;
use serde_json::Value;

pub async fn dispatch(
    chrome: &ChromeControl,
    method: &str,
    params: Value,
    command: Option<CommandInvocationId>,
) -> anyhow::Result<Value> {
    if !matches!(method, "chrome.status" | "chrome.shares" | "chrome.tabs.list") {
        anyhow::ensure!(command.is_some(), "ipc.command_invocation_id_required");
    }
    match method {
        "chrome.status" => {
            no_input(params)?;
            Ok(serde_json::to_value(chrome.status())?)
        }
        "chrome.set_enabled" => {
            let input: ChromeSetEnabledRequest = serde_json::from_value(params)?;
            let status = chrome.set_enabled(input.expected_revision, input.enabled).await?;
            Ok(serde_json::to_value(status)?)
        }
        "chrome.pair.begin" => {
            no_input(params)?;
            Ok(serde_json::to_value(chrome.begin_pairing()?)?)
        }
        "chrome.revoke" => {
            no_input(params)?;
            Ok(serde_json::to_value(chrome.revoke().await?)?)
        }
        // The user's Stop on a conversation's line: its shares end and, when it holds Chrome, its
        // commands still out fail; other conversations' shares go on.
        "chrome.stop" => {
            let input: ChromeSessionRequest = serde_json::from_value(params)?;
            chrome.stop_session(input.session_id);
            Ok(serde_json::to_value(chrome.session_shares(input.session_id))?)
        }
        "chrome.shares" => {
            let input: ChromeSessionRequest = serde_json::from_value(params)?;
            Ok(serde_json::to_value(chrome.session_shares(input.session_id))?)
        }
        // The user's 完成 in Aura for a tab the model handed them.
        "chrome.assist.finish" => {
            let input: ChromeSessionRequest = serde_json::from_value(params)?;
            chrome.finish_assist(input.session_id)?;
            Ok(serde_json::to_value(chrome.session_shares(input.session_id))?)
        }
        "chrome.tabs.list" => {
            no_input(params)?;
            Ok(serde_json::to_value(chrome.tab_choices().await)?)
        }
        "chrome.tabs.hand_over" => {
            let input: ChromeTabHandOverRequest = serde_json::from_value(params)?;
            Ok(serde_json::to_value(chrome.hand_over_tab(input.session_id, input.tab_id).await?)?)
        }
        _ => anyhow::bail!("chrome.unknown_method"),
    }
}

/// A call that takes nothing: an empty object, as the route's `EmptyToolInput` says.
fn no_input(params: Value) -> anyhow::Result<()> {
    match params {
        Value::Object(fields) if fields.is_empty() => Ok(()),
        _ => anyhow::bail!("ipc.invalid_params: this call takes no parameters"),
    }
}
