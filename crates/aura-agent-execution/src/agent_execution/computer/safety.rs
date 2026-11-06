use super::{Frozen, resolve};
use aura_desktop_domain::{HostCommand, HostResult};
use aura_desktop_service::DesktopControl;
use aura_domain_common::{SessionId, ToolCallId};
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;
use std::time::Duration;

pub(super) async fn inspect(
    desktop: &DesktopControl,
    session: SessionId,
    calls: &[aura_persistence_v2::hooks::PendingToolCall],
) {
    for call in calls {
        let handler = match call.tool_name.as_str() {
            "computer.act" => H::ComputerAct,
            "computer.input" => H::ComputerInput,
            _ => continue,
        };
        let settings = desktop.settings();
        let Ok((frozen, _)) =
            resolve::command(desktop, handler, session, call.arguments.clone(), &settings.settings)
        else {
            continue;
        };
        let inspected = desktop
            .run(
                HostCommand::Inspect { command: Box::new(frozen.command.clone()) },
                Duration::from_secs(10),
            )
            .await;
        let safety = match inspected {
            Ok(HostResult::Inspected { safety }) => safety,
            _ => "unavailable".into(),
        };
        desktop.remember_inspection(call.tool_call_id, frozen.command, safety);
    }
}

pub(super) fn bind(
    desktop: &DesktopControl,
    call: ToolCallId,
    frozen: &mut Frozen,
) -> anyhow::Result<()> {
    if !matches!(frozen.command, HostCommand::Act { .. } | HostCommand::Input { .. }) {
        return Ok(());
    }
    let safety = desktop.inspection(call, &frozen.command).unwrap_or_else(|| "unavailable".into());
    match safety.as_str() {
        "clear" => frozen.safety = Some(safety),
        "password" => anyhow::bail!("computer.refused: 密码要用户自己输入，这一步没有执行"),
        "payment" => anyhow::bail!(
            "computer.refused: 付款、转账、交易和购买须由用户自己操作，任何批准都不能放开，这一步没有执行"
        ),
        _ => anyhow::bail!(
            "computer.refused: 未能完成目标安全检查，这一步没有执行；请重新读取目标再决定"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_desktop_domain::{AppEntry, AppKind, InputStep};
    use std::sync::Arc;

    #[test]
    fn core_requires_a_matching_clean_inspection_and_explains_password_refusal() {
        let temp = tempfile::tempdir().expect("TEMP");
        let settings = aura_config::SettingsFacade::open(aura_config::AuraPaths::from_roots(
            temp.path().join("config"),
            temp.path().join("data"),
        ))
        .expect("settings");
        let desktop = DesktopControl::new(
            settings,
            Arc::new(aura_desktop_service::ProgramHost {
                program: temp.path().join("unused.exe"),
                folder: temp.path().to_owned(),
            }),
        );
        let call = ToolCallId::new();
        let mut frozen = Frozen {
            mode: Default::default(),
            command: HostCommand::Input {
                app: AppEntry {
                    id: "fixture".into(),
                    name: "fixture".into(),
                    kind: AppKind::Manual,
                    path: None,
                },
                steps: vec![InputStep::Type { text: "example".into() }],
                window: None,
                system_keys: false,
                clipboard: false,
            },
            after: None,
            refs: vec![],
            safety: None,
        };
        assert!(
            bind(&desktop, call, &mut frozen)
                .expect_err("missing inspection")
                .to_string()
                .contains("没有执行")
        );
        desktop.remember_inspection(call, frozen.command.clone(), "password".into());
        assert!(
            bind(&desktop, call, &mut frozen)
                .expect_err("password")
                .to_string()
                .contains("密码要用户自己输入")
        );
        desktop.remember_inspection(call, frozen.command.clone(), "clear".into());
        bind(&desktop, call, &mut frozen).expect("clean inspection");
        assert_eq!(frozen.safety.as_deref(), Some("clear"));
    }
}
