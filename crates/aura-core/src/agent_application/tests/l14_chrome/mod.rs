//! The user's own Chrome: a model's `chrome.*` call goes through the same admission, permission
//! and recording as any tool and reaches the paired extension as one command for the shared tab.
//! The test plays the extension through the real relay and pipe; the user's answers go through
//! Aura's approval routes.
use super::*;
use crate::chrome_control::test_extension::{Answers, PlayedExtension};
use aura_chrome_domain::commands::*;
use aura_chrome_domain::protocol::{CommandOutcome, ExtensionMessage};
use aura_chrome_domain::*;

mod acting;
mod asking;
mod refusals;
mod user;

/// Chrome's own id for the tab the user shares; the model never sees it.
const TAB: u32 = 7;
const SHOP: &str = "https://shop.example/cart";

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    services
}

fn done(value: impl serde::Serialize) -> Option<CommandOutcome> {
    Some(CommandOutcome::Done { value: serde_json::to_value(value).expect("a report") })
}

fn page(url: &str) -> ChromePageReport {
    ChromePageReport {
        url: url.to_owned(),
        title: "Shop".to_owned(),
        snapshot: "- heading \"Cart\" [level=1]\n- button \"Checkout\" [ref=e2]".to_owned(),
        snapshot_truncated: false,
        console: vec![],
        dialogs: vec![],
        notice: None,
    }
}

/// What the extension finds before an action, and the page it reports after every command.
#[derive(Clone)]
struct Page {
    url: &'static str,
    refused: Option<&'static str>,
    sensitive: bool,
}

const ORDINARY: Page = Page { url: SHOP, refused: None, sensitive: false };

fn answers(found: Page) -> Answers {
    Box::new(move |command| match command {
        ChromeCommand::Inspect { .. } => done(ChromeInspection {
            refused: found.refused.map(str::to_owned),
            sensitive: found.sensitive,
            origin: crate::chrome_control::shares::origin_of(found.url),
        }),
        ChromeCommand::Open { url } => done(ChromeOpenReport {
            tab: ChromeOpenedTab {
                tab_id: 41,
                window_id: 1,
                url: url.clone(),
                title: "Shop".to_owned(),
            },
            page: page(url),
        }),
        ChromeCommand::Close { .. } => done(ChromeCloseReport { closed: true }),
        ChromeCommand::Release { .. } => None,
        _ => done(page(found.url)),
    })
}

/// Which conversation the user shared Chrome's tab `TAB` with, at `SHOP`.
#[derive(Clone, Copy, PartialEq)]
enum Shared {
    Nothing,
    Here,
    /// Another conversation of the same workspace.
    Elsewhere,
}

#[derive(Clone, Copy)]
struct Setup {
    mode: PermissionMode,
    shared: Shared,
    /// Where the shared tab went before the turn, without the user confirming the site.
    moved_to: Option<&'static str>,
    /// Whether the extension is still connected when the turn starts.
    connected: bool,
    scope: ChromeShareScope,
}

impl Setup {
    const fn sharing(mode: PermissionMode) -> Self {
        Self {
            mode,
            shared: Shared::Here,
            moved_to: None,
            connected: true,
            scope: ChromeShareScope::Session,
        }
    }
}

/// A turn whose model makes the scenario's calls, with the extension answering as `answers` says.
struct Chrome {
    _host: crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
    provider: Provider,
    extension: PlayedExtension,
    session: SessionId,
    request: AgentRunRequest,
}

async fn run(scenario: Scenario, setup: Setup, answers: Answers) -> Chrome {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(scenario).await;
    let (runtime, session, model) = waiting::prepare_in_mode(&host, &provider, setup.mode).await;
    let status: ChromeControlStatus = serde_json::from_value(
        crate::r03_tests::success(&runtime, "chrome.status", json!({})).await,
    )
    .expect("status");
    crate::r03_tests::success(
        &runtime,
        "chrome.set_enabled",
        json!({"expected_revision": status.revision.expect("readable"), "enabled": true}),
    )
    .await;
    let chrome = Arc::clone(&services(&runtime).chrome);
    let (paired, _) = chrome.pair_extension().await;
    let extension = PlayedExtension::play(paired, answers);
    let shared_with = match setup.shared {
        Shared::Nothing => None,
        Shared::Here => Some(session),
        Shared::Elsewhere => {
            let projection =
                services(&runtime).persistence.workspace_session_projection().expect("sessions");
            let workspace = projection
                .sessions
                .iter()
                .find(|known| known.session_id == session)
                .expect("this conversation")
                .workspace_id;
            Some(create_session(&runtime, workspace, setup.mode).await)
        }
    };
    if let Some(other) = shared_with {
        extension.send(ExtensionMessage::Share {
            tab_id: TAB,
            window_id: 1,
            url: SHOP.to_owned(),
            title: "Shop".to_owned(),
            session_id: other.to_string(),
            scope: setup.scope,
        });
        until(|| chrome.has_shares(other)).await;
    }
    if let Some(url) = setup.moved_to {
        extension.send(ExtensionMessage::TabChanged {
            tab_id: TAB,
            url: url.to_owned(),
            title: "Pay".to_owned(),
        });
        until(|| chrome.tabs(session).first().is_some_and(|tab| tab.url == url)).await;
    }
    if !setup.connected {
        extension.disconnect();
        chrome.until(ChromeConnectionState::Waiting).await;
    }
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    Chrome { _host: host, runtime, provider, extension, session, request }
}

async fn until(condition: impl Fn() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("in time");
}

fn completed(view: &AgentRunProjection) -> bool {
    matches!(view.turn.state, TurnTaskState::Completed { .. })
}

fn asking(view: &AgentRunProjection) -> bool {
    !view.pending_approval_ids.is_empty()
}

impl Chrome {
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

    async fn wait(&self, predicate: impl Fn(&AgentRunProjection) -> bool) -> AgentRunProjection {
        waiting::wait(&self.runtime, &self.request, predicate).await
    }

    /// The question asked now, as the approval card reads it.
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

    /// The commands that acted or read in the tab, without the inspections before them.
    fn sent(&self) -> Vec<ChromeCommand> {
        self.extension
            .commands()
            .into_iter()
            .filter(|command| !matches!(command, ChromeCommand::Inspect { .. }))
            .collect()
    }
}

fn decisions(approval: &Value) -> Vec<String> {
    approval["approval"]["allowed_decisions"]
        .as_array()
        .expect("decisions")
        .iter()
        .map(|decision| decision.as_str().expect("a decision").to_owned())
        .collect()
}
