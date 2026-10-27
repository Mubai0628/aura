//! Handing the tab to the user: the model waits for them and reads the page they left; every
//! Chrome call waits meanwhile, and stopping the turn withdraws the wait.
use super::*;

const ASK_USER: Scenario = Scenario::Browser {
    tool: "chrome.request_user",
    arguments: r#"{"reason":"请在页面上登录"}"#,
};

fn assisted(outcome: ChromeAssistOutcome) -> Answers {
    Box::new(move |command| match command {
        ChromeCommand::Assist { .. } => {
            done(ChromeAssistReport { outcome, page: Some(page(SHOP)) })
        }
        other => answers(ORDINARY)(other),
    })
}

#[tokio::test]
async fn the_model_waits_for_the_user_and_reads_the_page_they_left() {
    let chrome = run(
        ASK_USER,
        Setup::sharing(PermissionMode::ReadOnly),
        assisted(ChromeAssistOutcome::Done),
    )
    .await;
    let view = chrome.wait(|view| completed(view) || asking(view)).await;
    assert!(completed(&view), "handing the tab over never asks");
    assert_eq!(
        chrome.sent(),
        vec![ChromeCommand::Assist {
            tab_id: TAB,
            reason: "请在页面上登录".into(),
            timeout_seconds: 300
        }]
    );
    let text = chrome.returned(0);
    assert!(text.contains(r#""outcome":"done""#) && text.contains("Checkout"), "{text}");
}

#[tokio::test]
async fn while_the_user_has_the_tab_chrome_waits_and_stopping_the_turn_withdraws_the_wait() {
    let waiting: Answers = Box::new(|command| match command {
        ChromeCommand::Assist { .. } => None,
        other => answers(ORDINARY)(other),
    });
    let chrome = run(ASK_USER, Setup::sharing(PermissionMode::ReadOnly), waiting).await;
    let control = Arc::clone(&services(&chrome.runtime).chrome);
    until(|| !chrome.sent().is_empty()).await;
    let paused = control.hold(SessionId::new()).err().expect("paused while the user has the tab");
    assert!(paused.starts_with("chrome.with_user"), "{paused}");
    let stopped: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(&chrome.runtime, "agent.cancel", json!(chrome.request)).await,
    )
    .expect("cancelled");
    assert_eq!(stopped.turn.state, TurnTaskState::Cancelled);
    until(|| !chrome.extension.cancelled.lock().expect("cancelled").is_empty()).await;
    assert!(matches!(
        chrome.extension.cancelled.lock().expect("cancelled")[0],
        ChromeCommand::Assist { .. }
    ));
    until(|| control.hold(SessionId::new()).is_ok()).await;
}

/// Aura shows the conversation's tab with the user and why; its 完成 ends the wait in Chrome.
#[tokio::test]
async fn aura_shows_the_tab_with_the_user_and_its_done_reaches_the_extension() {
    let waiting: Answers = Box::new(|command| match command {
        ChromeCommand::Assist { .. } => None,
        other => answers(ORDINARY)(other),
    });
    let chrome = run(ASK_USER, Setup::sharing(PermissionMode::ReadOnly), waiting).await;
    until(|| !chrome.sent().is_empty()).await;
    let request = json!({"session_id": chrome.session});
    let shares: ChromeSessionShares = serde_json::from_value(
        crate::r03_tests::success(&chrome.runtime, "chrome.shares", request.clone()).await,
    )
    .expect("shares");
    assert!(shares.connected);
    assert_eq!(shares.tabs.len(), 1);
    assert_eq!(shares.tabs[0].tab, "t1");
    assert_eq!(shares.tabs[0].state, ChromeSharedTabState::WithUser);
    assert_eq!(shares.tabs[0].assist_reason.as_deref(), Some("请在页面上登录"));
    crate::r03_tests::success(&chrome.runtime, "chrome.assist.finish", request).await;
    until(|| chrome.extension.assist_done.lock().expect("assist_done").clone() == [TAB]).await;
}
