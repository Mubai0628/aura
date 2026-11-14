//! A page the user handed over with @: a browser call that names it reaches Main with that page
//! and is held to the site that page was last seen at, not the model's own page's; a name that is
//! no page id is refused before anything is asked or sent.
use super::*;

const CLICK_HANDED: Scenario =
    Scenario::Browser { tool: "browser.click", arguments: r#"{"page":"p1","x":10,"y":20}"# };
const NOT_A_PAGE: Scenario =
    Scenario::Browser { tool: "browser.snapshot", arguments: r#"{"page":"page-1"}"# };

/// A turn whose model makes the scenario's call, with the user's page p1 last seen at `url`.
async fn run_handed(scenario: Scenario, url: &str) -> Browsing {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(scenario).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    set_browser(&runtime, |_| ());
    services(&runtime).agent_required().browser().note_page(session, "p1", url);
    let (seen, withdrawn) = play_host(&runtime, Host::Answers);
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    Browsing { _host: host, runtime, provider, seen, withdrawn, request }
}

#[tokio::test]
async fn a_call_naming_a_handed_page_reaches_main_with_that_page_and_its_site() {
    let browsing = run_handed(CLICK_HANDED, "http://127.0.0.1:9/docs").await;
    let Browsing { runtime, provider, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    let commands = seen.lock().expect("seen").clone();
    assert_eq!(commands.len(), 1, "the click ran: {}", returned(provider));
    assert_eq!(commands[0].page.as_deref(), Some("p1"));
    assert_eq!(commands[0].expected_host.as_deref(), Some("127.0.0.1"), "held to p1's site");
    // Main's answer moved p1, never the model's own page.
    let bridge = services(runtime).agent_required().browser();
    assert_eq!(bridge.page_url(request.session_id, "model"), None);
    assert_eq!(
        bridge.page_url(request.session_id, "p1").as_deref(),
        Some("http://127.0.0.1:9/docs")
    );
}

#[tokio::test]
async fn a_page_name_that_is_no_page_id_is_refused_before_anything_is_sent() {
    let browsing = run_handed(NOT_A_PAGE, "http://127.0.0.1:9/docs").await;
    let Browsing { runtime, provider, seen, request, .. } = &browsing;
    waiting::wait(runtime, request, completed).await;
    assert!(seen.lock().expect("seen").is_empty());
    let text = returned(provider);
    assert!(text.contains("browser.page"), "{text}");
}
