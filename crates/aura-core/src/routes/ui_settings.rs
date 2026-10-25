//! 设置 → 环境 — whether the session shows its debugging information and which terminal
//! a new terminal tab opens. Read and written through the settings facade like every setting.
use aura_config::{SettingsFacade, SettingsFacadeError, UiSettingsUpdate};
use aura_domain_common::CommandInvocationId;
use serde_json::Value;

pub(crate) fn dispatch(
    settings: &SettingsFacade,
    method: &str,
    params: Value,
    command: Option<CommandInvocationId>,
) -> anyhow::Result<Value> {
    match method {
        "ui.settings.get" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            Ok(serde_json::to_value(settings.ui_settings())?)
        }
        "ui.settings.update" => {
            anyhow::ensure!(command.is_some(), "ipc.command_invocation_id_required");
            let input: UiSettingsUpdate = serde_json::from_value(params)?;
            settings.update_ui_settings(input.expected_revision, &input.settings).map_err(
                |error| match error {
                    SettingsFacadeError::RevisionConflict { .. } => {
                        anyhow::anyhow!("ui_settings.stale: 设置已在别处更改，请刷新后再改。")
                    }
                    other => anyhow::anyhow!("ui_settings.not_saved: 设置没有保存：{other}"),
                },
            )?;
            Ok(serde_json::to_value(settings.ui_settings())?)
        }
        _ => anyhow::bail!("ui_settings.unknown_method"),
    }
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use aura_config::{AuraPaths, SettingsFacade};
    use serde_json::json;

    #[test]
    fn reads_and_saves_at_the_revision_shown_and_refuses_a_stale_one() {
        let temp = tempfile::tempdir().expect("TEMP");
        let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
        let settings = SettingsFacade::open(paths).expect("settings");
        let shown = dispatch(&settings, "ui.settings.get", json!({}), None).expect("read");
        assert_eq!(
            shown,
            json!({"revision":1,"settings":{"show_debug_information":false,"default_terminal":"power_shell"},"issue":null})
        );
        let update = json!({"expected_revision":1,"settings":{"show_debug_information":true,"default_terminal":"cmd"}});
        // Saving is a command: without its invocation it is refused.
        assert!(dispatch(&settings, "ui.settings.update", update.clone(), None).is_err());
        let command = Some("civ_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().expect("command id"));
        let saved =
            dispatch(&settings, "ui.settings.update", update.clone(), command).expect("save");
        assert_eq!(saved["revision"], 2);
        assert_eq!(saved["settings"]["default_terminal"], "cmd");
        let stale = dispatch(&settings, "ui.settings.update", update, command).expect_err("stale");
        assert!(stale.to_string().starts_with("ui_settings.stale"), "{stale}");
    }
}
