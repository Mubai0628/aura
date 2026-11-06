//! The user gives the desktop back from 近期活动 after taking it over: a turn that ended taken over
//! stays listed until then, the route gives it back, and the conversation's next turn running
//! ends the listing as well.
use super::*;

const TWICE: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.input", r#"{"app":"记事本","steps":[{"action":"type","text":"first"}]}"#),
        ("computer.input", r#"{"app":"记事本","steps":[{"action":"type","text":"second"}]}"#),
    ],
};

/// A turn whose first action the user took over, played to its end.
async fn taken_over_turn() -> Desktop {
    let mut desktop = Desktop::new(TWICE, PermissionMode::FullAccess, as_is).await;
    desktop.played.say_before(|command| match command {
        HostCommand::Guarded { hold: Some(hold), .. } => vec![HostMessage::HoldEnded {
            hold: hold.clone(),
            reason: HoldEnd::TakenOver,
            cause: Some("点了鼠标".into()),
        }],
        _ => vec![],
    });
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    let second = desktop.returned(1);
    assert!(second.contains("用户交还操作权之前不要再操作电脑"), "{second}");
    desktop
}

async fn listed(desktop: &Desktop) -> Value {
    crate::r03_tests::success(
        &desktop.runtime,
        "computer.controlling",
        json!({"session_id": desktop.session}),
    )
    .await["apps"]
        .clone()
}

/// Waits up to five seconds for the conversation to list `apps`.
async fn until_listed(desktop: &Desktop, apps: &Value) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let now = listed(desktop).await;
        if &now == apps {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "{now}");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

fn taken_over_notepad() -> Value {
    json!([{
        "mode": "foreground",
        "app_id": NOTEPAD,
        "name": "记事本",
        "foreground": true,
        "taken_over": true,
    }])
}

#[tokio::test]
async fn a_turn_that_ended_taken_over_is_listed_until_the_user_gives_the_desktop_back() {
    let desktop = taken_over_turn().await;
    until_listed(&desktop, &taken_over_notepad()).await;
    let given = crate::r03_tests::success(
        &desktop.runtime,
        "computer.give_back",
        json!({"session_id": desktop.session}),
    )
    .await;
    assert_eq!(given["apps"], json!([]), "{given}");
    assert_eq!(listed(&desktop).await, json!([]));
}

#[tokio::test]
async fn the_conversations_next_turn_ends_the_listing_of_a_turn_that_ended_taken_over() {
    let mut desktop = taken_over_turn().await;
    until_listed(&desktop, &taken_over_notepad()).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert_eq!(listed(&desktop).await, json!([]));
}
