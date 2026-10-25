//! 设置 → 网络与浏览器 — the browser switch, the allowed and blocked sites and the debugging
//! switch. Read and written through the settings facade like every setting; the browser host
//! reads them again after a save.
use aura_config::{BrowserSettingsUpdate, SettingsFacade, SettingsFacadeError};
use aura_domain_common::CommandInvocationId;
use serde_json::Value;

pub(crate) fn dispatch(
    settings: &SettingsFacade,
    method: &str,
    params: Value,
    command: Option<CommandInvocationId>,
) -> anyhow::Result<Value> {
    match method {
        "browser.settings.get" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            Ok(serde_json::to_value(settings.browser_settings())?)
        }
        "browser.settings.update" => {
            anyhow::ensure!(command.is_some(), "ipc.command_invocation_id_required");
            let input: BrowserSettingsUpdate = serde_json::from_value(params)?;
            settings.update_browser_settings(input.expected_revision, &input.settings).map_err(
                |error| match error {
                    SettingsFacadeError::RevisionConflict { .. } => {
                        anyhow::anyhow!("browser_settings.stale: 设置已在别处更改，请刷新后再改。")
                    }
                    // The page shows which list and which site, in the user's words.
                    SettingsFacadeError::Validation {
                        source: aura_config::ConfigValidationError::Semantic(message),
                        ..
                    } => anyhow::anyhow!("browser_settings.invalid: {message}"),
                    SettingsFacadeError::Validation { source, .. } => {
                        anyhow::anyhow!("browser_settings.invalid: {source}")
                    }
                    other => anyhow::anyhow!("browser_settings.not_saved: 设置没有保存：{other}"),
                },
            )?;
            Ok(serde_json::to_value(settings.browser_settings())?)
        }
        _ => anyhow::bail!("browser_settings.unknown_method"),
    }
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use aura_config::{AuraPaths, SettingsFacade};
    use serde_json::json;

    #[test]
    fn reads_and_saves_at_the_revision_shown_and_names_a_bad_site() {
        let temp = tempfile::tempdir().expect("TEMP");
        let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
        let settings = SettingsFacade::open(paths).expect("settings");
        let shown = dispatch(&settings, "browser.settings.get", json!({}), None).expect("read");
        assert_eq!(
            shown,
            json!({"revision":1,"settings":{"enabled":true,"allowed_sites":[],"blocked_sites":[],"debugging":false},"issue":null})
        );
        let update = json!({"expected_revision":1,"settings":{"enabled":true,"allowed_sites":["Docs.rs"],"blocked_sites":[],"debugging":false}});
        // Saving is a command: without its invocation it is refused.
        assert!(dispatch(&settings, "browser.settings.update", update.clone(), None).is_err());
        let command = Some("civ_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().expect("command id"));
        let saved =
            dispatch(&settings, "browser.settings.update", update.clone(), command).expect("save");
        assert_eq!(saved["revision"], 2);
        assert_eq!(saved["settings"]["allowed_sites"], json!(["docs.rs"]));
        let stale =
            dispatch(&settings, "browser.settings.update", update, command).expect_err("stale");
        assert!(stale.to_string().starts_with("browser_settings.stale"), "{stale}");
        let bad = json!({"expected_revision":2,"settings":{"enabled":true,"allowed_sites":[],"blocked_sites":["a b"],"debugging":false}});
        let refused =
            dispatch(&settings, "browser.settings.update", bad, command).expect_err("invalid");
        assert!(
            refused.to_string().starts_with("browser_settings.invalid: blocked_sites"),
            "{refused}"
        );
    }
}
