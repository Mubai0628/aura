//! Package program tools in the real Agent path: registry, permission, Job run,
//! result, recovery, invalidation and cancellation.
use super::support::*;
use super::*;
use aura_package_service::package_program::tests::support::Kind;

fn command(service: &crate::RuntimeServices) -> Installed {
    install(service, Kind::Command, |_| {})
}

fn package_resource(call: &CallContext) -> ResourceDescriptor {
    call.operation
        .content
        .resources
        .iter()
        .find(|resource| matches!(resource, ResourceDescriptor::ToolPackage { .. }))
        .cloned()
        .expect("a package call names its installed package")
}

#[tokio::test]
async fn l04_installed_program_tool_runs_for_the_agent_and_restores_without_replay() {
    let (f, installed) = Fixture::with_packages(PermissionMode::FullAccess, &[], command).await;
    let name = installed.tool_name();
    let registry = f.registry();
    let entry = registry
        .catalog_entries
        .iter()
        .find(|entry| entry.canonical_name.as_str() == name)
        .expect("the installed tool is in the catalog");
    assert_eq!(entry.load_state, ToolLoadStateV1::Ephemeral, "loaded like any discoverable tool");
    assert_eq!(entry.source, ToolSourceKindV1::Imported);

    let batch = f.admit(&[(name.as_str(), json!({}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let call = &batch.calls[&id];
    assert!(question(&f, call).is_none(), "full access does not ask");
    assert!(matches!(
        package_resource(call),
        ResourceDescriptor::ToolPackage { runtime: PackageRuntimeClass::Program, package_id, .. }
            if package_id == installed.summary.package_id
    ));
    assert_eq!(call.operation.content.capabilities, vec![ResolvedCapability::ProcessExecute]);
    let operation = call.operation.clone();
    let group = f.execute(batch).await;
    assert_eq!(f.output(id), json!("authorized command fixture\n"));
    let (summary, details) = activity(&f, id);
    assert_eq!(summary, "执行成功；以当前 Windows 用户身份运行，Aura 不隔离它的文件写入");
    assert!(details.is_empty(), "nothing changed during the call: {details:?}");

    // A restart rebuilds the batch from the stored records and never runs it again.
    let restored = f
        .service()
        .agent_execution_required()
        .resume_batch(batch_id)
        .expect("restored from the records stored with the snapshot");
    assert_eq!(restored.calls[&id].operation, operation);
    assert_eq!(f.execute(restored).await, group);
}

#[tokio::test]
async fn l04_workspace_mode_asks_once_and_the_session_answer_names_only_this_package() {
    let (mut f, installed) =
        Fixture::with_packages(PermissionMode::WorkspaceReadWrite, &[], command).await;
    let name = installed.tool_name();
    let batch = f.admit(&[(name.as_str(), json!({}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let proposal = question(&f, &batch.calls[&id]).expect("workspace mode asks for a program");
    assert!(proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowSession));
    let grant = answer(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
    let consent = grant.scope.network_consent.as_ref().expect("a package session answer");
    assert_eq!(consent.caller, NetworkConsentCaller::PackageTools);
    let ResourceDescriptor::ToolPackage { package_id, package_fingerprint, .. } =
        package_resource(&batch.calls[&id])
    else {
        unreachable!()
    };
    assert_eq!(
        consent.destinations,
        NetworkConsentDestinations::Package { package_id, package_fingerprint }
    );
    let group = f.execute(batch).await;
    assert_eq!(f.output(id), json!("authorized command fixture\n"));
    f.continue_after_tools(batch_id, &group);

    // The stored session answer covers the next call: execution does not stop to ask.
    let again = f.admit(&[(name.as_str(), json!({}))]);
    let second = again.batch.calls[0].tool_call_id;
    f.execute(again).await;
    assert_eq!(f.output(second), json!("authorized command fixture\n"));

    let other = f.package_session(PermissionMode::WorkspaceReadWrite, &[]);
    let batch = other.admit(&[(name.as_str(), json!({}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(question(&other, &batch.calls[&id]).is_some(), "another session asks again");
    assert!(matches!(
        other.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    assert!(!attempted(&other, id));

    // Disabling the package ends the answer: re-enabled content is a new question.
    set_enabled(f.service(), installed.summary.package_id, false);
    let store = f.service().broker.lock().expect("store");
    let answers =
        aura_persistence_v2::network_consents::list(store.database.connection(), f.turn.session_id)
            .expect("session answers");
    let answer = answers.iter().find(|answer| answer.grant_id == grant.grant_id).expect("listed");
    assert!(answer.revoked_at.is_some(), "the disable revoked it: {answer:?}");
}

#[tokio::test]
async fn l04_read_only_asks_every_time_and_offers_no_session_answer() {
    let (mut f, installed) = Fixture::with_packages(PermissionMode::ReadOnly, &[], command).await;
    let name = installed.tool_name();
    for _ in 0..2 {
        let batch = f.admit(&[(name.as_str(), json!({}))]);
        let id = batch.batch.calls[0].tool_call_id;
        let batch_id = batch.batch.tool_batch_id;
        let proposal = question(&f, &batch.calls[&id]).expect("read-only asks every time");
        assert_eq!(
            proposal.allowed_decisions,
            vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
        );
        answer(&f, &batch.calls[&id], ApprovalDecisionKind::AllowOnce);
        let group = f.execute(batch).await;
        assert_eq!(f.output(id), json!("authorized command fixture\n"));
        f.continue_after_tools(batch_id, &group);
    }
}

#[tokio::test]
async fn l04_disabling_refuses_queued_and_new_calls_before_anything_runs() {
    let (mut f, installed) = Fixture::with_packages(PermissionMode::FullAccess, &[], command).await;
    let name = installed.tool_name();
    let queued = f.admit(&[(name.as_str(), json!({}))]);
    let id = queued.batch.calls[0].tool_call_id;
    let batch_id = queued.batch.tool_batch_id;
    set_enabled(f.service(), installed.summary.package_id, false);
    let group = f.execute(queued).await;
    let refused = result(&f, id);
    assert!(
        matches!(
            &refused.outcome,
            ToolResultOutcomeV1::NotExecuted { error: Some(error), .. }
                if error.message.contains("package.disabled")
        ),
        "a call admitted before the disable never runs after it: {refused:?}"
    );
    assert!(refused.attempts.is_empty());
    let (summary, details) = activity(&f, id);
    assert!(!summary.contains("身份运行"), "a call that never ran does not say it ran: {summary}");
    assert!(details.iter().any(|line| line.contains("package.disabled")), "{details:?}");
    f.continue_after_tools(batch_id, &group);

    let batch = f.admit(&[(name.as_str(), json!({}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(!batch.calls.contains_key(&id), "a disabled package's tool is not planned");
    assert!(!attempted(&f, id));
    assert!(matches!(result(&f, id).outcome, ToolResultOutcomeV1::NotExecuted { .. }));
}

#[tokio::test]
async fn l04_cancelling_a_running_program_stops_its_process_tree() {
    let (f, installed) = Fixture::with_packages(PermissionMode::FullAccess, &[], |service| {
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
    })
    .await;
    let name = installed.tool_name();
    let batch = f.admit(&[(name.as_str(), json!({"wait": true}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let (stop, cancel) = tokio::sync::watch::channel(false);
    let engine = f.service().agent_execution_required().clone();
    let commands = f.service().commands_required().clone();
    let running = tokio::spawn(async move { engine.execute_batch(batch, commands, cancel).await });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !attempted(&f, id) {
        assert!(std::time::Instant::now() < until, "the program must start");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    stop.send(true).expect("cancel the running batch");
    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), running)
        .await
        .expect("cancellation ends the call long before the program's own wait")
        .expect("the batch task")
        .expect("the batch answers");
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
    assert!(!matches!(outcome, BatchOutcome::Unknown { .. }), "the stop is confirmed");
    let stopped = result(&f, id);
    assert!(
        matches!(
            &stopped.outcome,
            ToolResultOutcomeV1::Failed { error } if error.message.starts_with("package.cancelled")
        ),
        "{stopped:?}"
    );
    assert_eq!(
        stopped.side_effect_state,
        aura_execution_contract::SideEffectStateV1::NoEffect,
        "the tree was stopped"
    );
}
