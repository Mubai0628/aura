//! A turn keeps the desktop from its first action to its end, except while it waits for the
//! user's answer, and Aura's Stop gives it back at once. The user takes it over by using it: the
//! model is told what they did, the turn's later actions never reach the desktop nor ask for
//! another program, and the conversation's answers stay.
use super::*;

const TWICE: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.input", r#"{"app":"记事本","steps":[{"action":"type","text":"first"}]}"#),
        ("computer.input", r#"{"app":"记事本","steps":[{"action":"type","text":"second"}]}"#),
    ],
};

fn taken_over(command: &HostCommand) -> Option<HostOutcome> {
    matches!(command, HostCommand::Input { .. }).then(|| {
        HostOutcome::Failed(HostError {
            code: HostErrorCode::TakenOver,
            message: "用户已接管电脑，用户交还操作权之前不要再操作电脑".into(),
            delivery: Delivery::Uncertain,
            steps_done: Some(0),
        })
    })
}

/// The holds of the actions the host was sent, in order.
fn holds(played: &ScriptedHost) -> Vec<Option<String>> {
    played
        .requests()
        .into_iter()
        .filter_map(|command| match command {
            HostCommand::Guarded { hold, .. } => Some(hold),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_turns_actions_share_one_hold_that_is_given_back_when_the_turn_ends() {
    let mut desktop = Desktop::new(TWICE, PermissionMode::FullAccess, as_is).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    let view = desktop.wait(completed).await;
    let turn = view.turn.turn_task_id.to_string();
    assert_eq!(holds(&desktop.played), [Some(turn.clone()), Some(turn.clone())]);
    let given_back = HostCommand::Release { hold: turn };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !desktop.played.requests().contains(&given_back) {
        assert!(std::time::Instant::now() < deadline, "{:?}", desktop.played.requests());
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

const TWICE_THEN_READ: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.input", r#"{"app":"记事本","steps":[{"action":"type","text":"first"}]}"#),
        ("computer.input", r#"{"app":"记事本","steps":[{"action":"type","text":"second"}]}"#),
        ("computer.snapshot", r#"{"app":"记事本"}"#),
    ],
};

#[tokio::test]
async fn taking_over_tells_the_model_ends_the_turns_actions_and_keeps_the_answers() {
    let mut desktop = Desktop::new(TWICE_THEN_READ, PermissionMode::FullAccess, taken_over).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    let view = desktop.wait(|view| completed(view) || asking(view)).await;
    assert!(completed(&view), "the turn goes on to its end");
    let first = desktop.returned(0);
    assert!(first.contains("computer.taken_over") && first.contains("用户已接管电脑"), "{first}");
    let second = desktop.returned(1);
    assert!(second.contains("用户已接管电脑") && second.contains("没有执行"), "{second}");
    let inputs = desktop
        .acted()
        .iter()
        .filter(|command| matches!(command, HostCommand::Input { .. }))
        .count();
    assert_eq!(inputs, 1, "the second action never reached the desktop");
    let read = desktop.returned(2);
    assert!(read.contains(r#"e2 edit \"文字\""#), "the turn still reads the screen: {read}");
    assert!(control(&desktop.runtime).always_allowed(desktop.session, NOTEPAD));
}

const THEN_PAINT: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.input", r#"{"app":"记事本","steps":[{"action":"type","text":"first"}]}"#),
        ("computer.input", r#"{"app":"画图","steps":[{"action":"type","text":"second"}]}"#),
    ],
};

/// The host lists Paint beside Notepad.
fn beside_paint(command: &HostCommand) -> Option<HostOutcome> {
    let paint = AppEntry {
        id: "Microsoft.Paint_8wekyb3d8bbwe!App".into(),
        name: "画图".into(),
        kind: AppKind::Packaged,
        path: None,
    };
    matches!(command, HostCommand::Catalog { .. })
        .then(|| HostOutcome::Done(HostResult::Catalog { apps: vec![notepad(), paint] }))
}

/// The host lists Paint beside Notepad, and the user takes over Notepad's action.
fn taken_over_beside_paint(command: &HostCommand) -> Option<HostOutcome> {
    beside_paint(command).or_else(|| taken_over(command))
}

/// Whether the played host is sent `command` within a second.
async fn sent_soon(played: &ScriptedHost, command: &HostCommand) -> bool {
    for _ in 0..100 {
        if played.requests().contains(command) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    false
}

#[tokio::test]
async fn a_turn_waiting_for_an_answer_gives_the_desktop_back_until_its_next_action() {
    let mut desktop = Desktop::new(THEN_PAINT, PermissionMode::FullAccess, beside_paint).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    let view = desktop.wait(asking).await;
    let hold = view.turn.turn_task_id.to_string();
    let given_back = HostCommand::Release { hold: hold.clone() };
    assert!(
        sent_soon(&desktop.played, &given_back).await,
        "the click on 允许 would take the desktop over: {:?}",
        desktop.played.requests()
    );
    let controlling = crate::r03_tests::success(
        &desktop.runtime,
        "computer.controlling",
        json!({"session_id": desktop.session}),
    )
    .await;
    assert_eq!(controlling["apps"], json!([]), "{controlling}");
    let (id, approval) = desktop.question().await;
    desktop.answer(id, &approval, "allow_session").await;
    desktop.wait(completed).await;
    assert_eq!(holds(&desktop.played), [Some(hold.clone()), Some(hold)]);
    let second = desktop.returned(1);
    assert!(!second.contains("接管"), "{second}");
}

#[tokio::test]
async fn after_a_takeover_the_turn_asks_for_no_other_program() {
    let mut desktop =
        Desktop::new(THEN_PAINT, PermissionMode::FullAccess, taken_over_beside_paint).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    let view = desktop.wait(|view| completed(view) || asking(view)).await;
    assert!(completed(&view), "asked for Paint after the user took over");
    let second = desktop.returned(1);
    assert!(second.contains("computer.taken_over") && second.contains("没有执行"), "{second}");
    assert_eq!(desktop.acted().len(), 1);
}

/// The host refuses the turn's second action: the user took the desktop over after the first,
/// before Core heard of it.
fn refused_second(command: &HostCommand) -> Option<HostOutcome> {
    let HostCommand::Input { steps, .. } = command else { return None };
    matches!(steps.first(), Some(InputStep::Type { text }) if text == "second").then(|| {
        HostOutcome::Failed(HostError {
            code: HostErrorCode::TakenOver,
            message: "用户已接管电脑，用户交还操作权之前不要再操作电脑".into(),
            delivery: Delivery::NotDelivered,
            steps_done: None,
        })
    })
}

#[tokio::test]
async fn an_action_the_host_refuses_after_a_takeover_is_returned_as_cancelled() {
    let mut desktop = Desktop::new(TWICE, PermissionMode::FullAccess, refused_second).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    let second = desktop.returned(1);
    assert!(
        second.contains("computer.taken_over") && second.contains(r#""category":"cancelled""#),
        "{second}"
    );
}

#[tokio::test]
async fn a_takeover_between_two_actions_refuses_the_second_before_it_reaches_the_desktop() {
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
    assert!(!desktop.returned(0).contains("接管"), "{}", desktop.returned(0));
    let second = desktop.returned(1);
    assert!(second.contains("computer.taken_over") && second.contains("没有执行"), "{second}");
    assert!(
        second.contains("用户已接管电脑（点了鼠标）"),
        "the model hears what the user did: {second}"
    );
    assert_eq!(desktop.acted().len(), 1);
}
