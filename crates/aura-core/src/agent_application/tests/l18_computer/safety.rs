use super::*;

const WRITE: Scenario = Scenario::Sequence {
    calls: &[(
        "computer.input",
        r#"{"app":"记事本","steps":[{"action":"type","text":"must-not-arrive"}]}"#,
    )],
};

fn password(command: &HostCommand) -> Option<HostOutcome> {
    matches!(command, HostCommand::Inspect { .. })
        .then(|| HostOutcome::Done(HostResult::Inspected { safety: "password".into() }))
}

fn payment(command: &HostCommand) -> Option<HostOutcome> {
    matches!(command, HostCommand::Inspect { .. })
        .then(|| HostOutcome::Done(HostResult::Inspected { safety: "payment".into() }))
}

#[tokio::test]
async fn financial_action_is_refused_before_approval_even_for_an_always_allowed_program() {
    let mut desktop = Desktop::new(WRITE, PermissionMode::FullAccess, payment).await;
    switch(&desktop.runtime, true).await;
    always_allow(&desktop.runtime).await;
    desktop.start().await;
    let view = desktop.wait(|view| completed(view) || asking(view)).await;
    assert!(completed(&view));
    let returned = desktop.returned(0);
    assert!(returned.contains("付款") && returned.contains("没有执行"), "{returned}");
    assert!(desktop.acted().is_empty());
}

#[tokio::test]
async fn password_refusal_reaches_the_model_without_approval_or_execution() {
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        let mut desktop = Desktop::new(WRITE, mode, password).await;
        switch(&desktop.runtime, true).await;
        always_allow(&desktop.runtime).await;
        desktop.start().await;
        let view = desktop.wait(|view| completed(view) || asking(view)).await;
        assert!(completed(&view), "a refusal must never ask");
        let returned = desktop.returned(0);
        assert!(
            returned.contains("密码要用户自己输入") && returned.contains("没有执行"),
            "{returned}"
        );
        assert!(desktop.acted().is_empty());
        assert!(desktop.side_effects("computer.input").is_empty());
    }
}
