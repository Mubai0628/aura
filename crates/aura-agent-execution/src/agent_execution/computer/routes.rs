//! 设置 → 电脑操控 and a conversation's 正在操控: the settings, the programs for the composer's
//! 「@」 and their icons, the programs a conversation is controlling now, the user's Stop, giving
//! the desktop back after taking it over, and the approval card's 总是允许. Only Aura's own pages
//! reach them; a model never does.
use super::super::*;
use anyhow::{anyhow, ensure};
use aura_config::{ComputerApp, ComputerControlSettingsUpdate, SettingsFacadeError};
use aura_desktop_domain::*;
use aura_desktop_service::DesktopControl;
use serde_json::Value;

/// The most programs the composer's list gets.
const CATALOG_LISTED: usize = 200;

fn controlling(desktop: &DesktopControl, session: SessionId) -> ComputerControllingResult {
    ComputerControllingResult {
        pip: desktop.pip_status(),
        apps: desktop
            .controlling(session)
            .into_iter()
            .map(|app| ComputerControlledV1 {
                mode: app.mode,
                app_id: app.app_id,
                name: app.name,
                foreground: app.foreground,
                taken_over: app.taken_over,
            })
            .collect(),
    }
}

/// The user's Stop: the conversation's answer for each program is withdrawn in the approval
/// records, then its calls end, an always-allowed program asks again in this conversation, and
/// the desktop its turn keeps is given back.
pub(super) async fn stop(
    execution: &Arc<AgentExecution>,
    desktop: &DesktopControl,
    session: SessionId,
    apps: Vec<String>,
) -> anyhow::Result<()> {
    let journal = Arc::clone(&execution.journal);
    let withdrawn = apps.clone();
    execution
        .store
        .run_critical(StoreStep::operation("computer.stop"), StoreAccess::Write, move || {
            Ok(journal.lock()?.database.unit_of_work(|tx| {
                let now = UtcTimestamp::now();
                for app in &withdrawn {
                    aura_persistence_v2::desktop_apps::revoke(tx, session, app, now)?;
                }
                Ok::<(), aura_persistence_core::PersistenceCoreError>(())
            })?)
        })
        .await?;
    for app in &apps {
        desktop.stop(session, app);
    }
    desktop.release(session).await;
    Ok(())
}

fn not_saved(error: SettingsFacadeError) -> anyhow::Error {
    match error {
        SettingsFacadeError::RevisionConflict { .. } => {
            anyhow!("computer_settings.stale: 设置已在别处更改，请刷新后再改。")
        }
        SettingsFacadeError::Validation {
            source: aura_config::ConfigValidationError::Semantic(message),
            ..
        } => anyhow!("computer_settings.invalid: {message}"),
        SettingsFacadeError::Validation { source, .. } => {
            anyhow!("computer_settings.invalid: {source}")
        }
        other => anyhow!("computer_settings.not_saved: 设置没有保存：{other}"),
    }
}

pub async fn dispatch(
    execution: &Arc<AgentExecution>,
    method: &str,
    params: Value,
    command: Option<CommandInvocationId>,
) -> anyhow::Result<Value> {
    if !matches!(
        method,
        "computer.settings.get" | "computer.catalog" | "computer.icons" | "computer.controlling"
    ) {
        ensure!(command.is_some(), "ipc.command_invocation_id_required");
    }
    let desktop =
        execution.desktop.get().ok_or_else(|| anyhow!("computer.unavailable: 电脑操控不可用"))?;
    match method {
        "computer.settings.get" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            Ok(serde_json::to_value(desktop.settings())?)
        }
        "computer.settings.update" => {
            let input: ComputerControlSettingsUpdate = serde_json::from_value(params)?;
            let was_on = desktop.settings().settings.enabled;
            desktop
                .settings_facade()
                .update_computer_settings(input.expected_revision, &input.settings)
                .map_err(not_saved)?;
            desktop.refresh_catalog();
            desktop.update_stop_keys(&input.settings.pip_stop_keys);
            // Turned off: the host ends now, and nothing reaches the desktop until it is on again.
            if was_on && !input.settings.enabled {
                desktop.stop_host().await;
            }
            Ok(serde_json::to_value(desktop.settings())?)
        }
        "computer.catalog" => {
            let input: ComputerCatalogRequest = serde_json::from_value(params)?;
            let settings = desktop.settings();
            if !settings.settings.enabled || settings.issue.is_some() {
                return Ok(serde_json::to_value(ComputerCatalogResult {
                    enabled: false,
                    apps: vec![],
                    notice: settings.issue,
                })?);
            }
            let wanted = input.query.as_deref().map(str::trim).unwrap_or_default().to_lowercase();
            let result = match desktop.catalog().await {
                Ok(apps) => ComputerCatalogResult {
                    enabled: true,
                    apps: apps
                        .into_iter()
                        .filter(|app| {
                            wanted.is_empty() || app.name.to_lowercase().contains(&wanted)
                        })
                        .take(CATALOG_LISTED)
                        .map(|app| ComputerCatalogEntryV1 { app_id: app.id, name: app.name })
                        .collect(),
                    notice: None,
                },
                Err(error) => ComputerCatalogResult {
                    enabled: true,
                    apps: vec![],
                    notice: Some(error.message),
                },
            };
            Ok(serde_json::to_value(result)?)
        }
        "computer.icons" => {
            let input: ComputerIconsRequest = serde_json::from_value(params)?;
            let settings = desktop.settings();
            if !settings.settings.enabled || settings.issue.is_some() {
                return Ok(serde_json::to_value(ComputerIconsResult::default())?);
            }
            let ids: Vec<String> = input.app_ids.into_iter().take(MAX_ICON_APPS).collect();
            // An icon only stands beside a program's name: one that cannot be read leaves the
            // name with the plain program icon.
            let icons = desktop.icons(&ids).await.unwrap_or_default();
            Ok(serde_json::to_value(ComputerIconsResult {
                icons: icons
                    .into_iter()
                    .map(|icon| ComputerIconV1 { app_id: icon.app_id, png: icon.png })
                    .collect(),
            })?)
        }
        "computer.controlling" => {
            let input: ComputerSessionRequest = serde_json::from_value(params)?;
            Ok(serde_json::to_value(controlling(desktop, input.session_id))?)
        }
        "computer.stop" => {
            let input: ComputerStopRequest = serde_json::from_value(params)?;
            let apps = match input.app_id {
                Some(app) => vec![app],
                None => desktop
                    .controlling(input.session_id)
                    .into_iter()
                    .map(|app| app.app_id)
                    .collect(),
            };
            stop(execution, desktop, input.session_id, apps).await?;
            Ok(serde_json::to_value(controlling(desktop, input.session_id))?)
        }
        "computer.give_back" => {
            let input: ComputerSessionRequest = serde_json::from_value(params)?;
            desktop.give_back(input.session_id).await;
            Ok(serde_json::to_value(controlling(desktop, input.session_id))?)
        }
        "computer.always_allow" => {
            let input: ComputerAlwaysAllowRequest = serde_json::from_value(params)?;
            ensure!(
                !input.app_id.trim().is_empty() && !input.name.trim().is_empty(),
                "computer.always_allow: 给出程序的 id 和名字"
            );
            desktop
                .always_allow(ComputerApp { app_id: input.app_id, name: input.name })
                .map_err(|message| anyhow!("computer_settings.not_saved: {message}"))?;
            Ok(serde_json::to_value(desktop.settings())?)
        }
        _ => anyhow::bail!("computer.unknown_method"),
    }
}
