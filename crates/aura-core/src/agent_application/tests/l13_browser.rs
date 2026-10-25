//! The built-in browser: a model's
//! browser call goes through the same admission, permission and recording as any tool and reaches
//! the browser host as one command. The test plays the host. A blocked site and a browser switched
//! off are refused before anything is asked; a ReadOnly session opens the user's allowed sites
//! without asking and asks for any other. Page debugging needs the user's switch; a script runs on
//! this computer's own pages like a click and asks on any other site; a wait for the user ends
//! with the user's answer, or is withdrawn when the turn stops.
use super::*;
use aura_browser_domain::*;
use aura_config::BrowserSettings;
use std::sync::{Arc, Mutex};

// A screenshot reaches a model that takes images with its result.
#[path = "l13_screenshots.rs"]
mod l13_screenshots;

type Seen = Arc<Mutex<Vec<BrowserHostCommandV1>>>;
type Withdrawn = Arc<Mutex<Vec<String>>>;

const OPEN: Scenario =
    Scenario::Browser { tool: "browser.open", arguments: r#"{"url":"127.0.0.1:9/game"}"# };
const EVALUATE: Scenario = Scenario::Browser {
    tool: "browser.evaluate",
    arguments: r#"{"expression":"window.game && window.game.score"}"#,
};
const NETWORK: Scenario =
    Scenario::Browser { tool: "browser.network", arguments: r#"{"failed_only":true}"# };
// An element by its ref, and the page's counters.
const INSPECT: Scenario =
    Scenario::Browser { tool: "browser.inspect", arguments: r#"{"element":"e3"}"# };
const PERFORMANCE: Scenario = Scenario::Browser { tool: "browser.performance", arguments: "{}" };
// A point on the page in whole CSS pixels, as DeepSeek wrote it in a real run.
const CLICK_AT_POINT: Scenario =
    Scenario::Browser { tool: "browser.click", arguments: r#"{"x":280,"y":330}"# };
const ASK_USER: Scenario = Scenario::Browser {
    tool: "browser.request_user",
    arguments: r#"{"reason":"请在页面上登录测试账号"}"#,
};

/// How the stand-in for Main answers.
#[derive(Clone, Copy, PartialEq)]
enum Host {
    Answers,
    /// Leaves a wait for the user unanswered, as a user who does nothing yet.
    KeepsTheUserWaiting,
}

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    services
}

fn page(url: &str) -> serde_json::Value {
    serde_json::to_value(BrowserPageResult {
        url: url.into(),
        title: "Game".into(),
        snapshot: "- heading \"Minesweeper\" [level=1]\n- button \"Start\" [ref=e2]".into(),
        snapshot_truncated: false,
        console: vec![BrowserConsoleEntryV1 {
            level: "error".into(),
            text: "Uncaught ReferenceError: board is not defined".into(),
            source: Some("http://127.0.0.1:9/game.js:12".into()),
        }],
        dialogs: vec![],
        notice: None,
    })
    .expect("page")
}

/// Plays Main: takes the browser channel, answers each command in its tool's shape, and records
/// the commands Core withdrew.
fn play_host(runtime: &crate::RuntimeState, host: Host) -> (Seen, Withdrawn) {
    let bridge = services(runtime).agent_required().browser();
    let queue = crate::AsyncPriorityQueue::new();
    bridge.pin_publisher(queue.clone());
    let seen: Seen = Arc::default();
    let withdrawn: Withdrawn = Arc::default();
    let (record, record_withdrawn) = (Arc::clone(&seen), Arc::clone(&withdrawn));
    tokio::spawn(async move {
        while let Ok(frame) = queue.recv().await {
            let crate::WireFrame::Control(bytes) = frame.frame else { continue };
            let Ok(notification) = serde_json::from_slice::<crate::JsonRpcNotification>(&bytes)
            else {
                continue;
            };
            if notification.method == "browser.cancel.v1" {
                let cancel: BrowserHostCancelV1 =
                    serde_json::from_value(notification.params).expect("a withdrawal");
                record_withdrawn.lock().expect("withdrawn").push(cancel.command_id);
                continue;
            }
            if notification.method != "browser.command.v1" {
                continue;
            }
            let command: BrowserHostCommandV1 =
                serde_json::from_value(notification.params).expect("a browser command");
            record.lock().expect("seen").push(command.clone());
            let url = match &command.action {
                BrowserHostActionV1::Open { url } => url.clone(),
                _ => bridge.page_url(command.session_id).unwrap_or_else(|| "about:blank".into()),
            };
            let mut image = None;
            let result = match &command.action {
                BrowserHostActionV1::RequestUser { .. } if host == Host::KeepsTheUserWaiting => {
                    continue;
                }
                BrowserHostActionV1::Screenshot { .. } => {
                    image = Some(BrowserImageV1 {
                        media_type: "image/jpeg".into(),
                        width: 2,
                        height: 2,
                        data_base64: l13_screenshots::JPEG_BASE64.into(),
                    });
                    json!({"url": url, "title": "Game"})
                }
                BrowserHostActionV1::RequestUser { .. } => {
                    json!({"outcome": "done", "page": page(&url)})
                }
                BrowserHostActionV1::Evaluate { .. } => {
                    json!({"url": url, "value": "42", "value_truncated": false, "exception": null})
                }
                BrowserHostActionV1::Inspect { .. } => json!({"url": url,
                    "html": "<div id=\"board\">…</div>", "html_truncated": false,
                    "box_model": {"x": 8, "y": 40, "width": 640, "height": 480},
                    "styles": ["display: flex", "z-index: 2"]}),
                BrowserHostActionV1::Performance => json!({"url": url,
                    "metrics": ["JSHeapUsedSize: 12.4 MB", "Nodes: 1520"]}),
                BrowserHostActionV1::Network { .. } => json!({"url": url, "entries": [{
                    "method": "GET", "url": "http://127.0.0.1:9/board.json", "kind": "fetch",
                    "status": 404, "mime_type": "text/html", "failure": null,
                    "request_headers": ["Cookie: （已隐藏）"], "response_headers": []
                }], "truncated": false}),
                _ => page(&url),
            };
            bridge.complete(BrowserHostResultV1 {
                command_id: command.command_id,
                page_url: url.clone(),
                outcome: BrowserHostOutcomeV1::Completed { result, image },
            });
        }
    });
    (seen, withdrawn)
}

fn set_browser(runtime: &crate::RuntimeState, change: impl FnOnce(&mut BrowserSettings)) {
    let settings = &services(runtime)._settings;
    let snapshot = settings.browser_settings();
    let mut next = snapshot.settings;
    change(&mut next);
    settings.update_browser_settings(snapshot.revision.expect("readable"), &next).expect("saved");
}

fn completed(view: &AgentRunProjection) -> bool {
    matches!(view.turn.state, TurnTaskState::Completed { .. })
}

/// The text the model got back for its browser call.
fn returned(provider: &Provider) -> String {
    let requests = provider.requests.lock().expect("requests").clone();
    requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == "fixture_0_0")
        .and_then(|message| message["content"].as_str().map(str::to_owned))
        .expect("the browser call's result reached the model")
}

/// The fixture holds the test's folders: keep it alive for as long as the runtime runs.
struct Browsing {
    _host: crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
    provider: Provider,
    seen: Seen,
    withdrawn: Withdrawn,
    request: AgentRunRequest,
}

async fn run(mode: PermissionMode, settings: impl FnOnce(&mut BrowserSettings)) -> Browsing {
    run_on(OPEN, mode, settings, None, Host::Answers).await
}

/// A turn whose model makes the scenario's browser call, with the page already at `page_url`.
async fn run_on(
    scenario: Scenario,
    mode: PermissionMode,
    settings: impl FnOnce(&mut BrowserSettings),
    page_url: Option<&str>,
    host_play: Host,
) -> Browsing {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(scenario).await;
    let (runtime, session, model) = waiting::prepare_in_mode(&host, &provider, mode).await;
    set_browser(&runtime, settings);
    if let Some(url) = page_url {
        services(&runtime).agent_required().browser().note_page(session, url);
    }
    let (seen, withdrawn) = play_host(&runtime, host_play);
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    Browsing { _host: host, runtime, provider, seen, withdrawn, request }
}

fn debugging(settings: &mut BrowserSettings) {
    settings.debugging = true;
}

fn asking(view: &AgentRunProjection) -> bool {
    matches!(view.run.state, AgentRunState::AwaitingApproval { .. })
}

#[tokio::test]
async fn a_working_session_opens_the_page_and_the_model_reads_the_snapshot_and_errors() {
    let browsing = run(PermissionMode::WorkspaceReadWrite, |_| ()).await;
    let Browsing { runtime, provider, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    let commands = seen.lock().expect("seen").clone();
    assert_eq!(commands.len(), 1, "one command for one call: {}", returned(provider));
    let command = &commands[0];
    assert_eq!(command.action, BrowserHostActionV1::Open { url: "http://127.0.0.1:9/game".into() });
    assert_eq!(command.session_id, request.session_id);
    assert!(!command.access.read_only);
    assert_eq!(command.expected_host, None, "opening replaces the page");
    assert!(command.access.file_root.as_deref().is_some_and(|root| root.starts_with("file:///")));
    let text = returned(provider);
    assert!(
        text.contains("button \\\"Start\\\" [ref=e2]")
            || text.contains("button \"Start\" [ref=e2]"),
        "{text}"
    );
    assert!(text.contains("board is not defined"), "{text}");
}

#[tokio::test]
async fn read_only_opens_an_allowed_site_without_asking() {
    let browsing =
        run(PermissionMode::ReadOnly, |settings| settings.allowed_sites = vec!["127.0.0.1".into()])
            .await;
    let Browsing { runtime, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    let commands = seen.lock().expect("seen").clone();
    assert_eq!(commands.len(), 1);
    assert!(commands[0].access.read_only);
    assert!(commands[0].access.approved_hosts.contains(&"127.0.0.1".to_owned()));
}

#[tokio::test]
async fn read_only_asks_before_opening_any_other_site() {
    let browsing = run(PermissionMode::ReadOnly, |_| ()).await;
    let Browsing { runtime, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, asking).await;
    assert!(seen.lock().expect("seen").is_empty(), "nothing reaches the browser before the answer");
}

#[tokio::test]
async fn a_blocked_site_is_refused_before_anything_is_asked_in_every_mode() {
    for mode in [PermissionMode::FullAccess, PermissionMode::ReadOnly] {
        let browsing =
            run(mode, |settings| settings.blocked_sites = vec!["127.0.0.1:9".into()]).await;
        let Browsing { runtime, provider, seen, request, .. } = &browsing;
        waiting::wait(runtime, request, completed).await;
        assert!(seen.lock().expect("seen").is_empty(), "{mode:?}");
        let text = returned(provider);
        assert!(text.contains("已在浏览器设置中被阻止"), "{mode:?}: {text}");
    }
}

#[tokio::test]
async fn a_browser_switched_off_is_refused_with_where_to_switch_it_on() {
    let browsing = run(PermissionMode::FullAccess, |settings| settings.enabled = false).await;
    let Browsing { runtime, provider, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    assert!(seen.lock().expect("seen").is_empty());
    let text = returned(provider);
    assert!(text.contains("内置浏览器已在设置中关闭"), "{text}");
}

#[tokio::test]
async fn page_debugging_is_refused_until_the_user_switches_it_on() {
    for scenario in [EVALUATE, NETWORK] {
        let browsing = run_on(
            scenario,
            PermissionMode::FullAccess,
            |_| (),
            Some("http://127.0.0.1:9/game"),
            Host::Answers,
        )
        .await;
        let Browsing { runtime, provider, seen, request, .. } = &browsing;
        waiting::wait(runtime, request, completed).await;
        assert!(seen.lock().expect("seen").is_empty());
        let text = returned(provider);
        assert!(text.contains("网页调试没有开启") && text.contains("只有用户能"), "{text}");
    }
    // Switched on, the page's network log is a read like its console.
    let browsing = run_on(
        NETWORK,
        PermissionMode::ReadOnly,
        debugging,
        Some("http://127.0.0.1:9/game"),
        Host::Answers,
    )
    .await;
    let Browsing { runtime, provider, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    let commands = seen.lock().expect("seen").clone();
    assert_eq!(
        commands[0].action,
        BrowserHostActionV1::Network { failed_only: true, max_entries: 50 }
    );
    assert!(returned(provider).contains("board.json"));
}

#[tokio::test]
async fn a_script_runs_on_this_computers_page_and_asks_on_any_other_site() {
    let browsing = run_on(
        EVALUATE,
        PermissionMode::WorkspaceReadWrite,
        debugging,
        Some("http://127.0.0.1:9/game"),
        Host::Answers,
    )
    .await;
    let Browsing { runtime, provider, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    let commands = seen.lock().expect("seen").clone();
    assert_eq!(commands.len(), 1, "{}", returned(provider));
    assert!(matches!(&commands[0].action, BrowserHostActionV1::Evaluate { expression }
        if expression == "window.game && window.game.score"));
    assert_eq!(commands[0].expected_host.as_deref(), Some("127.0.0.1"), "held to the page");
    assert!(returned(provider).contains("42"));
    // Another site is asked first, even with full access; a machine on the local network counts
    // as another site.
    for (mode, url) in [
        (PermissionMode::FullAccess, "https://example.com/account"),
        (PermissionMode::WorkspaceReadWrite, "http://192.168.1.1/"),
    ] {
        let browsing = run_on(EVALUATE, mode, debugging, Some(url), Host::Answers).await;
        let Browsing { runtime, seen, request, .. } = &browsing;
        waiting::wait(runtime, request, asking).await;
        assert!(seen.lock().expect("seen").is_empty(), "{url}: nothing runs before the answer");
    }
}

#[tokio::test]
async fn the_model_waits_for_the_user_and_reads_the_page_they_left() {
    let browsing = run_on(
        ASK_USER,
        PermissionMode::ReadOnly,
        |_| (),
        Some("http://127.0.0.1:9/login"),
        Host::Answers,
    )
    .await;
    let Browsing { runtime, provider, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    let commands = seen.lock().expect("seen").clone();
    assert_eq!(commands.len(), 1, "{}", returned(provider));
    assert_eq!(
        commands[0].action,
        BrowserHostActionV1::RequestUser {
            reason: "请在页面上登录测试账号".into(),
            timeout_ms: 180_000
        }
    );
    assert_eq!(commands[0].timeout_ms, 195_000, "Core's backstop outlasts the host's own timer");
    let text = returned(provider);
    assert!(text.contains("done") && text.contains("Minesweeper"), "{text}");
}

#[tokio::test]
async fn stopping_the_turn_withdraws_a_wait_for_the_user() {
    let browsing = run_on(
        ASK_USER,
        PermissionMode::ReadOnly,
        |_| (),
        Some("http://127.0.0.1:9/login"),
        Host::KeepsTheUserWaiting,
    )
    .await;
    let Browsing { runtime, seen, withdrawn, request, .. } = &browsing;
    let waited = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if let Some(command) = seen.lock().expect("seen").first() {
                return command.command_id.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the wait reached the browser");
    let stopped: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(runtime, "agent.cancel", json!(request)).await,
    )
    .expect("cancelled");
    assert_eq!(stopped.turn.state, TurnTaskState::Cancelled);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !withdrawn.lock().expect("withdrawn").contains(&waited) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the browser was told to stop waiting for the user");
}

/// An element's HTML,
/// box and style, and the page's counters, are page debugging: the user's switch first, then a read
/// through the host, in any mode.
#[tokio::test]
async fn element_inspection_and_performance_counters_need_the_switch_and_only_read() {
    for scenario in [INSPECT, PERFORMANCE] {
        let off = run_on(
            scenario,
            PermissionMode::FullAccess,
            |_| (),
            Some("http://127.0.0.1:9/game"),
            Host::Answers,
        )
        .await;
        let Browsing { runtime, provider, seen, request, .. } = &off;
        waiting::wait(runtime, request, completed).await;
        assert!(seen.lock().expect("seen").is_empty());
        assert!(returned(provider).contains("网页调试没有开启"));
    }
    for (scenario, action, shows) in [
        (INSPECT, BrowserHostActionV1::Inspect { element: "e3".into() }, "display: flex"),
        (PERFORMANCE, BrowserHostActionV1::Performance, "JSHeapUsedSize: 12.4 MB"),
    ] {
        let on = run_on(
            scenario,
            PermissionMode::ReadOnly,
            debugging,
            Some("http://127.0.0.1:9/game"),
            Host::Answers,
        )
        .await;
        let Browsing { runtime, provider, seen, request, .. } = &on;
        waiting::wait(runtime, request, completed).await;
        let commands = seen.lock().expect("seen").clone();
        assert_eq!(commands[0].action, action);
        assert!(returned(provider).contains(shows));
    }
}

/// A click at a whole-number point
/// reaches the page.
#[tokio::test]
async fn a_click_at_a_whole_number_point_reaches_the_page() {
    let browsing = run_on(
        CLICK_AT_POINT,
        PermissionMode::WorkspaceReadWrite,
        |_| (),
        Some("http://127.0.0.1:9/game"),
        Host::Answers,
    )
    .await;
    let Browsing { runtime, provider, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    let commands = seen.lock().expect("seen").clone();
    assert_eq!(commands.len(), 1, "the click ran: {}", returned(provider));
    assert!(
        matches!(commands[0].action, BrowserHostActionV1::Click { x: Some(x), y: Some(y), .. } if x == 280.0 && y == 330.0),
        "{:?}",
        commands[0].action
    );
}
