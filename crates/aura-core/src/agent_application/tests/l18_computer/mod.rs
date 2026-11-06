//! Programs on the user's own desktop: a model's `computer.*` call goes through the same
//! admission, permission and recording as any tool, and reaches Aura's desktop host as the one
//! command it was resolved to. The test plays the host; the user's answers go through Aura's
//! approval routes, and the settings, 总是允许 and Stop through the computer routes.
use super::*;
use aura_desktop_domain::*;
use aura_desktop_service::test_host::{Played, ScriptedHost};
use std::sync::Arc;

mod arranging;
mod asking;
mod calls;
mod child_session;
mod choosing;
mod giving_back;
mod holding;
mod icons;
mod offering;
mod safety;

const NOTEPAD: &str = "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App";
/// The first bytes of a PNG: enough for Core to take it for one, nothing decodes them.
const PNG_BASE64: &str = "iVBORw0KGgo=";

fn notepad() -> AppEntry {
    AppEntry { id: NOTEPAD.into(), name: "记事本".into(), kind: AppKind::Packaged, path: None }
}

fn window() -> WindowInfo {
    WindowInfo {
        window: "w1312".into(),
        title: "无标题 - 记事本".into(),
        app_id: Some(NOTEPAD.into()),
        process_id: 7,
        process_name: "Notepad.exe".into(),
        rect: Rect { x: 100, y: 100, width: 800, height: 600 },
        minimized: false,
        foreground: false,
    }
}

fn element(name: &str, role: Role, depth: u32, id: i32, abilities: Vec<Ability>) -> Element {
    Element {
        key: ElementKey {
            runtime_id: vec![42, id],
            control_type: 50_000,
            automation_id: String::new(),
            name: name.into(),
        },
        role,
        depth,
        rect: Rect { x: 100 + id, y: 120, width: 40, height: 20 },
        value: None,
        enabled: true,
        focused: false,
        password: false,
        checked: None,
        expanded: None,
        selected: None,
        abilities,
        more: false,
    }
}

/// Notepad's window as the played host reads it: the window, its edit and a button.
fn elements() -> Vec<Element> {
    vec![
        element("无标题 - 记事本", Role::Window, 0, 1, vec![]),
        element("文字", Role::Edit, 1, 2, vec![Ability::SetValue, Ability::Focus]),
        element("保存", Role::Button, 1, 3, vec![Ability::Click]),
    ]
}

/// What the played host answers unless the test's own script answers first.
fn ordinary(command: &HostCommand) -> HostOutcome {
    HostOutcome::Done(match command {
        HostCommand::Guarded { command, .. } => return ordinary(command),
        HostCommand::Release { .. } => HostResult::Released,
        HostCommand::Keep { .. } => HostResult::Kept { kept: true },
        HostCommand::Inspect { .. } => HostResult::Inspected { safety: "clear".into() },
        HostCommand::Catalog { .. } => HostResult::Catalog { apps: vec![notepad()] },
        HostCommand::Windows { .. } => HostResult::Windows { windows: vec![window()] },
        HostCommand::Launch { .. } => {
            HostResult::Launched { windows: vec![window()], already_running: true }
        }
        HostCommand::Displays => HostResult::Displays { monitors: vec![] },
        HostCommand::Icons { apps, .. } => HostResult::Icons {
            icons: apps
                .iter()
                .map(|app| AppIcon { app_id: app.id.clone(), png: PNG_BASE64.into() })
                .collect(),
        },
        HostCommand::Screenshot { .. } => HostResult::Screenshot {
            png: PNG_BASE64.into(),
            map: ScreenMap {
                left: 0,
                top: 0,
                width: 960,
                height: 540,
                screen_width: 1920,
                screen_height: 1080,
            },
            monitors: vec![],
            windows: vec![window()],
        },
        HostCommand::Input { steps, .. } => {
            HostResult::Input { steps_done: u32::try_from(steps.len()).expect("steps") }
        }
        HostCommand::Elements { .. } => {
            HostResult::Elements { window: window(), elements: elements(), complete: true }
        }
        HostCommand::Act { steps, .. } => HostResult::Acted {
            after: steps
                .iter()
                .map(|step| elements().into_iter().find(|element| element.key == *step.element()))
                .collect(),
        },
        HostCommand::Window { .. } => {
            HostResult::Window { window: "w1312".into(), changed: true, windows: vec![window()] }
        }
    })
}

type Script = fn(&HostCommand) -> Option<HostOutcome>;

fn as_is(_: &HostCommand) -> Option<HostOutcome> {
    None
}

struct Desktop {
    _host: crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
    provider: Provider,
    played: Arc<ScriptedHost>,
    session: SessionId,
    model: ModelProfileId,
    request: Option<AgentRunRequest>,
}

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    services
}

fn control(runtime: &crate::RuntimeState) -> Arc<aura_desktop_service::DesktopControl> {
    Arc::clone(
        services(runtime).agent_required().execution.desktop.get().expect("computer control"),
    )
}

/// Turns computer control on or off from the settings page.
async fn switch(runtime: &crate::RuntimeState, on: bool) {
    let snapshot = crate::r03_tests::success(runtime, "computer.settings.get", json!({})).await;
    let mut settings = snapshot["settings"].clone();
    settings["enabled"] = json!(on);
    crate::r03_tests::success(
        runtime,
        "computer.settings.update",
        json!({"expected_revision": snapshot["revision"], "settings": settings}),
    )
    .await;
}

async fn always_allow(runtime: &crate::RuntimeState) {
    crate::r03_tests::success(
        runtime,
        "computer.always_allow",
        json!({"app_id": NOTEPAD, "name": "记事本"}),
    )
    .await;
}

impl Desktop {
    /// A session whose model makes the scenario's calls, with the host played as `script` says.
    async fn new(scenario: Scenario, mode: PermissionMode, script: Script) -> Self {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(scenario).await;
        let (runtime, session, model) = waiting::prepare_in_mode(&host, &provider, mode).await;
        let played =
            ScriptedHost::new(move |command| script(command).unwrap_or_else(|| ordinary(command)));
        control(&runtime).use_launcher(Arc::new(Played(Arc::clone(&played)))).await;
        Self { _host: host, runtime, provider, played, session, model, request: None }
    }

    async fn start(&mut self) {
        let started =
            waiting::start(&self.runtime, self.session, self.model, AgentTaskMode::Normal).await;
        self.request = Some(AgentRunRequest {
            session_id: self.session,
            agent_run_id: started.run.agent_run_id,
        });
    }

    async fn wait(&self, predicate: impl Fn(&AgentRunProjection) -> bool) -> AgentRunProjection {
        waiting::wait(&self.runtime, self.request.as_ref().expect("a turn"), predicate).await
    }

    /// The text the model got back for its `ordinal`th call (from 0), as the latest request
    /// carries it.
    fn returned(&self, ordinal: usize) -> String {
        let requests = self.provider.requests.lock().expect("requests").clone();
        requests
            .iter()
            .rev()
            .find_map(|request| {
                let messages = request["messages"].as_array()?;
                let result =
                    messages.iter().filter(|message| message["role"] == "tool").nth(ordinal)?;
                result["content"].as_str().map(str::to_owned)
            })
            .unwrap_or_else(|| panic!("call {ordinal}'s result reached the model"))
    }

    async fn question(&self) -> (ApprovalRequestId, Value) {
        let view = self.wait(asking).await;
        let id = view.pending_approval_ids[0];
        let approval = crate::r03_tests::success(
            &self.runtime,
            "agent.approval.get",
            json!({"session_id": self.session, "approval_id": id}),
        )
        .await;
        (id, approval)
    }

    async fn answer(&self, id: ApprovalRequestId, approval: &Value, decision: &str) {
        crate::r03_tests::success(
            &self.runtime,
            "agent.approval.resolve",
            json!({
                "session_id": self.session,
                "approval_id": id,
                "expected_revision": approval["approval"]["revision"],
                "decision": decision,
            }),
        )
        .await;
    }

    /// What reached the desktop: the host's commands other than listing programs and windows,
    /// keeping the desktop while the model thinks and giving it back when the turn ends.
    fn acted(&self) -> Vec<HostCommand> {
        self.played
            .sent()
            .into_iter()
            .filter(|command| {
                !matches!(
                    command,
                    HostCommand::Catalog { .. }
                        | HostCommand::Windows { .. }
                        | HostCommand::Inspect { .. }
                        | HostCommand::Keep { .. }
                        | HostCommand::Release { .. }
                )
            })
            .collect()
    }

    fn side_effects(&self, action: &str) -> Vec<String> {
        let journal = services(&self.runtime).broker.lock().expect("journal");
        let mut query = journal
            .database
            .connection()
            .prepare(
                "SELECT r.side_effect FROM execution_results r \
                 JOIN operations o ON o.id = r.operation_id \
                 WHERE json_extract(o.payload,'$.content.action_kind') = ?1",
            )
            .expect("query");
        query
            .query_map([action], |row| row.get(0))
            .expect("rows")
            .collect::<Result<_, _>>()
            .expect("effects")
    }

    fn allowed_in_session(&self) -> std::collections::BTreeSet<String> {
        let journal = services(&self.runtime).broker.lock().expect("journal");
        aura_persistence_v2::desktop_apps::authorized(journal.database.connection(), self.session)
            .expect("answers")
    }
}

fn completed(view: &AgentRunProjection) -> bool {
    matches!(view.turn.state, TurnTaskState::Completed { .. })
}

fn asking(view: &AgentRunProjection) -> bool {
    !view.pending_approval_ids.is_empty()
}

fn decisions(approval: &Value) -> Vec<String> {
    approval["approval"]["allowed_decisions"]
        .as_array()
        .expect("decisions")
        .iter()
        .map(|decision| decision.as_str().expect("a decision").to_owned())
        .collect()
}

fn offered(request: &Value) -> Vec<String> {
    let release = aura_tool_registry::agent_release::release().expect("release");
    release
        .manifest()
        .entries
        .iter()
        .filter(|entry| entry.handler.name().starts_with("computer."))
        .filter(|entry| {
            let wire = aura_model_domain::provider_wire_tool_name(
                entry.handler.name(),
                entry.definition.tool_id,
            );
            request["tools"].as_array().is_some_and(|tools| {
                tools.iter().any(|tool| tool["function"]["name"] == wire || tool["name"] == wire)
            })
        })
        .map(|entry| entry.handler.name().to_owned())
        .collect()
}
