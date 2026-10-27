//! One conversation uses Chrome at a time, a tab handed to the user pauses every call, the side
//! panel's 全部停止 ends every share, fails what is still out and releases the lock, and Aura's
//! Stop on a conversation's line does that for that conversation's shares and calls only.
use super::*;
use aura_chrome_domain::commands::{
    ChromeCommand, ChromeSessionChoice, ChromeShareScope, ChromeShareState,
};
use aura_domain_common::SessionId;

struct Titles(Vec<(SessionId, &'static str)>);

impl ChromeSessions for Titles {
    fn list(&self) -> Vec<ChromeSessionChoice> {
        self.0
            .iter()
            .map(|(session, title)| ChromeSessionChoice {
                session_id: session.to_string(),
                title: (*title).to_owned(),
                workspace: "Aura".to_owned(),
            })
            .collect()
    }
}

#[test]
fn one_conversation_holds_chrome_until_its_last_call_ends() {
    let fixture = fixture_with(|_| {});
    let (docs, game) = (SessionId::new(), SessionId::new());
    fixture.control.set_sessions(Arc::new(Titles(vec![(docs, "Docs"), (game, "Game")])));
    let first = fixture.control.hold(docs).expect("free");
    let second = fixture.control.hold(docs).expect("the same conversation's next call");
    let busy = fixture.control.hold(game).err().expect("another conversation waits");
    assert!(busy.starts_with("chrome.busy") && busy.contains("「Docs」"), "{busy}");
    drop(first);
    assert!(fixture.control.hold(game).is_err(), "Docs still has a call running");
    drop(second);
    let _game = fixture.control.hold(game).expect("released with the last call");
    assert!(fixture.control.hold(docs).is_err());
}

#[test]
fn a_tab_with_the_user_pauses_every_conversations_calls() {
    let fixture = fixture_with(|_| {});
    let (docs, game) = (SessionId::new(), SessionId::new());
    fixture.control.inner.lock().shares.add(
        docs,
        7,
        "https://docs.example/",
        "Docs",
        false,
        ChromeShareScope::Session,
    );
    fixture.control.set_with_user(7, Some("请登录"));
    for session in [docs, game] {
        let paused = fixture.control.hold(session).err().expect("paused");
        assert!(paused.starts_with("chrome.with_user"), "{paused}");
    }
    fixture.control.set_with_user(7, None);
    assert!(fixture.control.hold(game).is_ok(), "the user handed it back");
}

#[tokio::test]
async fn stop_fails_what_is_out_ends_every_share_and_releases_the_lock() {
    let fixture = fixture_with(|_| {});
    fixture.enable().await;
    let (mut extension, _) = fixture.paired().await;
    let (docs, game) = (SessionId::new(), SessionId::new());
    fixture.control.inner.lock().shares.add(
        docs,
        7,
        "https://docs.example/",
        "Docs",
        false,
        ChromeShareScope::Session,
    );
    fixture.control.inner.broadcast_shares();
    let Some(CoreMessage::Shares { shares }) = extension.receive().await else { panic!("shares") };
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0].state, ChromeShareState::Active);
    let held = fixture.control.hold(docs).expect("free");
    let control = fixture.control.clone();
    let out = tokio::spawn(async move {
        control.request(json!({"op":"snapshot"}), None, Duration::from_secs(30)).await
    });
    assert!(matches!(extension.receive().await, Some(CoreMessage::Command { .. })));
    extension.send(ExtensionMessage::Stop).await;
    assert_eq!(
        out.await.expect("join"),
        Err(ChromeRequestError::Failed {
            code: "chrome.stopped".into(),
            message: "用户停止了 Aura 在 Chrome 里的操作。".into(),
        })
    );
    assert_eq!(extension.receive().await, Some(CoreMessage::Shares { shares: vec![] }));
    assert!(!fixture.control.has_shares(docs));
    // A call from before Stop ending releases nothing of a newer hold, the same conversation's
    // included.
    let after = fixture.control.hold(docs).expect("Stop released the lock");
    drop(held);
    assert!(fixture.control.hold(game).is_err(), "Docs' newer call still holds Chrome");
    drop(after);
    let game_call = fixture.control.hold(game).expect("free once that call ends");
    assert!(fixture.control.hold(docs).is_err());
    drop(game_call);
}

/// Aura's Stop on a conversation's line ends that conversation's shares and calls; another
/// conversation's shares, its calls and its hold on Chrome go on.
#[tokio::test]
async fn a_conversations_stop_ends_only_its_own_shares_and_calls() {
    let fixture = fixture_with(|_| {});
    fixture.enable().await;
    let (mut extension, _) = fixture.paired().await;
    let (docs, game) = (SessionId::new(), SessionId::new());
    for (session, tab_id, url) in
        [(docs, 7, "https://docs.example/"), (game, 9, "https://game.example/")]
    {
        fixture.control.inner.lock().shares.add(
            session,
            tab_id,
            url,
            "Page",
            false,
            ChromeShareScope::Session,
        );
    }
    let held = fixture.control.hold(docs).expect("free");
    let control = fixture.control.clone();
    let out = tokio::spawn(async move {
        control.request(json!({"op":"snapshot"}), None, Duration::from_secs(30)).await
    });
    assert!(matches!(extension.receive().await, Some(CoreMessage::Command { .. })));
    let released = |message: Option<CoreMessage>| {
        let Some(CoreMessage::Command { id: 0, command, origin: None }) = message else {
            panic!("release: {message:?}")
        };
        serde_json::from_value::<ChromeCommand>(command).expect("a command")
    };

    fixture.control.stop_session(game);
    let Some(CoreMessage::Shares { shares }) = extension.receive().await else { panic!("shares") };
    assert_eq!(shares.iter().map(|share| share.tab_id).collect::<Vec<_>>(), vec![7]);
    assert_eq!(released(extension.receive().await), ChromeCommand::Release { tab_ids: vec![9] });
    assert_eq!(
        fixture.control.inner.lock().live.as_ref().map(|live| live.pending.len()),
        Some(1),
        "Docs' call is still out"
    );
    assert!(fixture.control.hold(game).is_err(), "Docs still holds Chrome");

    fixture.control.stop_session(docs);
    assert_eq!(
        out.await.expect("join"),
        Err(ChromeRequestError::Failed {
            code: "chrome.stopped".into(),
            message: "用户停止了 Aura 在 Chrome 里的操作。".into(),
        })
    );
    assert_eq!(extension.receive().await, Some(CoreMessage::Shares { shares: vec![] }));
    assert_eq!(released(extension.receive().await), ChromeCommand::Release { tab_ids: vec![7] });
    let game_call = fixture.control.hold(game).expect("Docs' Stop released the lock");
    drop(held);
    assert!(fixture.control.hold(docs).is_err(), "Docs' old call ending keeps Game's hold");
    drop(game_call);
}
