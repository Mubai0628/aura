//! Lifecycle changes against calls: a running call finishes on the content it
//! started with, the content is collected once nothing references it, and a new
//! installed version refuses calls admitted for the old one.
use super::support::*;
use super::*;
use crate::package_program::tests::support::Kind;
use crate::package_services::test_support::{payloads, uninstall};

fn waiting(service: &crate::RuntimeServices) -> Installed {
    install(service, Kind::Command, |raw| {
        let tool = &mut raw["tools"][0];
        tool["model"]["input_schema"] = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false,
            "properties": {"wait": {"type": "boolean"}},
            "required": ["wait"]
        });
        tool["conformance_cases"][0]["arguments"] = json!({"wait": false});
        tool["runtime"]["command"]["arguments"] =
            json!([{"type": "conditional_flag", "source": "/wait", "flag": "--wait-for-cancel"}]);
    })
}

/// The call holds its package content: the last thing its start writes before the program runs.
fn holds_content(f: &Fixture, operation: impl std::fmt::Display) -> bool {
    let store = f.service().broker.lock().expect("journal");
    store
        .database
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_package_references              WHERE owner_kind='execution' AND owner_id=?1)",
            [format!("agent-call:{operation}")],
            |row| row.get(0),
        )
        .expect("the call's reference")
}

#[tokio::test]
async fn l04_uninstalling_keeps_a_running_calls_content_until_the_call_ends() {
    let (f, installed) = Fixture::with_packages(PermissionMode::FullAccess, &[], waiting).await;
    let name = installed.tool_name();
    let batch = f.admit(&[(name.as_str(), json!({"wait": true}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let operation = batch.calls[&id].operation.operation_id;
    let (stop, cancel) = tokio::sync::watch::channel(false);
    let engine = f.service().agent_execution_required().clone();
    let commands = f.service().commands_required().clone();
    let running = tokio::spawn(async move { engine.execute_batch(batch, commands, cancel).await });
    // The uninstall comes once the call holds its content, not a fixed half second after
    // its attempt was recorded (the audit saw the start take longer, and the content go first);
    // the half second is now the program's own staging and launch after that.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !holds_content(&f, operation) {
        assert!(std::time::Instant::now() < until, "the program must start");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let removed = uninstall(f.service(), installed.summary.package_id);
    assert!(removed.installed.is_none(), "uninstall takes effect at once");
    assert_eq!(payloads(f.service()), 1, "the running call still holds its content");

    stop.send(true).expect("end the running call");
    tokio::time::timeout(std::time::Duration::from_secs(60), running)
        .await
        .expect("the call ends")
        .expect("the batch task")
        .expect("the batch answers");
    assert_eq!(payloads(f.service()), 0, "nothing references it any more: collected");
    let store = f.service().broker.lock().expect("journal");
    let attempt = aura_persistence_v2::execution::load(store.database.connection(), operation, 1)
        .expect("the call's attempt");
    let aura_execution_contract::AttemptState::Terminal { result } = attempt.state else {
        panic!("the call ended")
    };
    assert!(
        result.warnings.iter().any(|warning| warning == "tool.package.changed_during_call"),
        "the result says the call started before the uninstall: {:?}",
        result.warnings
    );
    drop(store);
    let (summary, details) = activity(&f, id);
    assert!(summary.ends_with("；开始后工具包已变更"), "the row says so: {summary}");
    let started_before = "这个调用在工具包停用、撤销信任、卸载或更换版本之前已开始";
    assert!(details.iter().any(|line| line.starts_with(started_before)), "{details:?}");
}

#[tokio::test]
async fn l04_a_newly_installed_version_refuses_calls_admitted_for_the_old_one() {
    let (f, installed) = Fixture::with_packages(PermissionMode::FullAccess, &[], |service| {
        install(service, Kind::Command, |_| {})
    })
    .await;
    let name = installed.tool_name();
    let batch = f.admit(&[(name.as_str(), json!({}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let upgraded = install(f.service(), Kind::Command, |raw| {
        raw["package_version"] = json!("1.0.1");
    });
    assert_eq!(upgraded.summary.package_id, installed.summary.package_id, "same package");
    f.execute(batch).await;
    let refused = result(&f, id);
    assert!(
        matches!(
            &refused.outcome,
            ToolResultOutcomeV1::NotExecuted { error: Some(error), .. }
                if error.message.contains("package.version_changed")
        ),
        "{refused:?}"
    );
    assert!(refused.attempts.is_empty(), "nothing ran");
}
