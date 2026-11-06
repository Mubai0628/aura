//! What reaches the desktop and what the model gets back: an unknown result never run again,
//! a screenshot of the program only, and refusals of what was never handed out.
use super::*;

fn act_times_out(command: &HostCommand) -> Option<HostOutcome> {
    matches!(command, HostCommand::Act { .. }).then(|| {
        let mut error =
            HostError::new(HostErrorCode::Timeout, "记事本没有及时回应", Delivery::Uncertain);
        error.steps_done = Some(1);
        HostOutcome::Failed(error)
    })
}

const READ_THEN_ACT: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.snapshot", r#"{"app":"记事本"}"#),
        (
            "computer.act",
            r#"{"app":"记事本","steps":[{"action":"set_value","ref":"e2","value":"a"},{"action":"click","ref":"e3"}]}"#,
        ),
    ],
};

#[tokio::test]
async fn a_step_that_may_have_reached_the_program_is_unknown_and_never_run_again() {
    let mut desktop = Desktop::new(READ_THEN_ACT, PermissionMode::FullAccess, act_times_out).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    let text = desktop.returned(1);
    assert!(
        text.contains("computer.timeout")
            && text.contains("前 1 步已做")
            && text.contains("不确定这一步做到了哪里"),
        "{text}"
    );
    let acts =
        desktop.acted().iter().filter(|command| matches!(command, HostCommand::Act { .. })).count();
    assert_eq!(acts, 1, "never run again");
    assert_eq!(desktop.side_effects("computer.act"), [r#""unknown""#]);
}

const SHOOT: Scenario = Scenario::Sequence {
    calls: &[("computer.screenshot", r#"{"app":"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"}"#)],
};

#[tokio::test]
async fn a_screenshot_shows_only_the_program_and_reaches_a_model_that_takes_images() {
    let mut desktop = Desktop::new(SHOOT, PermissionMode::WorkspaceReadWrite, as_is).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert_eq!(
        desktop.acted(),
        [HostCommand::Screenshot {
            show: vec![notepad()],
            paint: true,
            max_long_edge: 1_568,
            region: None,
            marks: vec![],
        }]
    );
    let result = desktop.returned(0);
    assert!(
        result.contains(r#""screenshot":"p1""#) && result.contains(r#""shown_to_model":true"#),
        "{result}"
    );
    let value: serde_json::Value = serde_json::from_str(&result).expect("the result");
    assert_eq!(
        value["outcome"]["payload"]["value"]["windows"],
        json!([{
            "window": "w1312", "title": "无标题 - 记事本", "foreground": false,
            "x": 50, "y": 50, "width": 400, "height": 300
        }]),
        "the window at (100, 100) 800×600 in the screenshot's half-size pixels"
    );
    let requests = desktop.provider.requests.lock().expect("requests").clone();
    let messages = requests.last().expect("after the screenshot")["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let at = messages.iter().position(|m| m["role"] == "tool").expect("the screenshot's result");
    assert_eq!(messages[at + 1]["role"], "user", "the image follows its result");
}

const MARKED: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.screenshot", r#"{"app":"记事本","marks":true}"#),
        ("computer.snapshot", r#"{"app":"记事本"}"#),
        ("computer.screenshot", r#"{"app":"记事本","marks":true}"#),
    ],
};

#[tokio::test]
async fn marks_number_the_latest_reads_elements_that_can_be_acted_on() {
    let mut desktop = Desktop::new(MARKED, PermissionMode::WorkspaceReadWrite, as_is).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert!(desktop.returned(0).contains("computer.marks"), "{}", desktop.returned(0));
    let sent = desktop.acted();
    assert!(matches!(sent[0], HostCommand::Elements { .. }), "nothing before the read: {sent:?}");
    let HostCommand::Screenshot { marks, .. } = &sent[1] else { panic!("{sent:?}") };
    // e1 is the window itself, which nothing can be done to.
    assert_eq!(
        marks,
        &[
            Mark { number: 2, rect: Rect { x: 102, y: 120, width: 40, height: 20 } },
            Mark { number: 3, rect: Rect { x: 103, y: 120, width: 40, height: 20 } },
        ]
    );
    let shot = desktop.returned(2);
    assert!(shot.contains(r#""marks":2"#) && shot.contains("编号框的号 n 就是元素 en"), "{shot}");
}

const WRONG: Scenario = Scenario::Sequence {
    calls: &[
        (
            "computer.snapshot",
            r#"{"app":"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App","root":"e99"}"#,
        ),
        ("computer.snapshot", r#"{"app":"画图"}"#),
        ("computer.input", r#"{"app":"记事本","steps":[{"action":"click","x":10,"y":10}]}"#),
    ],
};

#[tokio::test]
async fn a_call_naming_what_was_never_handed_out_is_refused_in_cores_words() {
    let mut desktop = Desktop::new(WRONG, PermissionMode::FullAccess, as_is).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert!(desktop.returned(0).contains("computer.ref"), "{}", desktop.returned(0));
    assert!(desktop.returned(1).contains("computer.app"), "{}", desktop.returned(1));
    assert!(desktop.returned(2).contains("image"), "{}", desktop.returned(2));
    assert!(desktop.acted().is_empty(), "nothing reached the desktop");
}

const OPEN: Scenario = Scenario::Sequence { calls: &[("computer.open", r#"{"app":"记事本"}"#)] };

#[tokio::test]
async fn opening_a_program_that_already_runs_lists_its_windows_and_changes_nothing() {
    let mut desktop = Desktop::new(OPEN, PermissionMode::WorkspaceReadWrite, as_is).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert_eq!(desktop.acted(), [HostCommand::Launch { app: notepad(), timeout_ms: 15_000 }]);
    let opened = desktop.returned(0);
    assert!(opened.contains(r#""already_running":true"#) && opened.contains("w1312"), "{opened}");
    assert_eq!(desktop.side_effects("computer.open"), [r#""no_effect""#]);
}

/// The host's report after a click: the button is named 已保存 now.
fn saved_after_click(command: &HostCommand) -> Option<HostOutcome> {
    let HostCommand::Act { steps, .. } = command else { return None };
    let after = steps
        .iter()
        .map(|step| {
            let mut element = elements()
                .into_iter()
                .find(|element| element.key.runtime_id == step.element().runtime_id)?;
            element.key.name = "已保存".into();
            Some(element)
        })
        .collect();
    Some(HostOutcome::Done(HostResult::Acted { after }))
}

const CLICK_TWICE: Scenario = Scenario::Sequence {
    calls: &[
        ("computer.snapshot", r#"{"app":"记事本"}"#),
        ("computer.act", r#"{"app":"记事本","steps":[{"action":"click","ref":"e3"}]}"#),
        ("computer.act", r#"{"app":"记事本","steps":[{"action":"click","ref":"e3"}]}"#),
    ],
};

#[tokio::test]
async fn after_an_act_a_ref_stands_for_the_element_as_the_act_left_it() {
    let mut desktop =
        Desktop::new(CLICK_TWICE, PermissionMode::FullAccess, saved_after_click).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    desktop.wait(completed).await;
    assert!(
        desktop.returned(1).contains(r#"e3 button \"已保存\" [click]"#),
        "{}",
        desktop.returned(1)
    );
    let clicked: Vec<String> = desktop
        .acted()
        .into_iter()
        .filter_map(|command| match command {
            HostCommand::Act { steps, .. } => Some(steps[0].element().name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(clicked, ["保存", "已保存"], "the second click names the button as it is now");
}
