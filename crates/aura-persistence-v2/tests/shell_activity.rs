#![allow(clippy::unwrap_used)]
#[allow(dead_code)] // Shared transaction fixture also serves completion tests.
#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::AgentTaskMode;
use aura_domain_common::*;
use aura_persistence_v2::{V2Store, execution::shell, tool};
use aura_windows_user_contract::*;

#[test]
fn exact_visible_call_lookup_crosses_the_recent_twenty_boundary_and_checks_scope() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let calls = (0..25)
        .map(|_| ("fs.read", serde_json::json!({"path":"note.txt","start_line":1,"line_count":10})))
        .collect();
    let batch = f.admit(&mut store, calls);
    let ids = batch.batch.calls.iter().map(|c| c.tool_call_id).collect::<Vec<_>>();
    let mut request = aura_tool_projection::AgentToolActivityRequest {
        session_id: f.turn.session_id,
        before_call_id: None,
        limit: 100,
        call_ids: Some(ids.clone()),
    };
    let page = tool::activity::page(store.database.connection(), &request).unwrap();
    assert_eq!(page.calls.len(), 25);
    assert!(page.calls.iter().all(|c| c.display.target.as_deref() == Some("note.txt")));
    assert_eq!(
        page.calls
            .iter()
            .map(|c| c.summary.tool_call_id)
            .collect::<std::collections::BTreeSet<_>>(),
        ids.into_iter().collect()
    );
    request.session_id = SessionId::new();
    assert!(tool::activity::page(store.database.connection(), &request).is_err());
}

#[test]
fn queued_shell_has_immutable_input_and_session_bound_pagination_then_interruption() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("v2");
    let mut store = V2Store::create(&root).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let input = ShellExecuteInput {
        requires_network: false,
        script: "Write-Output '原始脚本'".into(),
        working_directory: ".".into(),
        stdin: "原始输入".into(),
        timeout_ms: 1000,
        max_output_bytes: 4096,
    };
    let batch = f.admit(
        &mut store,
        vec![
            ("shell.powershell", serde_json::to_value(&input).unwrap()),
            ("shell.cmd", serde_json::to_value(&input).unwrap()),
        ],
    );
    let call = batch.batch.calls[0].tool_call_id;
    let query =
        ShellQueryInput { tool_call_id: call, stdout_offset: 0, stderr_offset: 0, max_bytes: 4096 };
    let before = shell::query(store.database.connection(), f.turn.session_id, &query).unwrap();
    assert_eq!(before.state, ShellCallState::Waiting);
    assert_eq!(before.command.script, input.script);
    assert!(before.started_at.is_none() && before.exit_code.is_none());
    assert!(shell::query(store.database.connection(), SessionId::new(), &query).is_err());
    let page = tool::activity::page(
        store.database.connection(),
        &aura_tool_projection::AgentToolActivityRequest {
            session_id: f.turn.session_id,
            before_call_id: None,
            limit: 1,
            call_ids: None,
        },
    )
    .unwrap();
    assert_eq!(page.calls.len(), 1);
    assert!(page.next_before_call_id.is_some());
    assert!(
        tool::activity::page(
            store.database.connection(),
            &aura_tool_projection::AgentToolActivityRequest {
                session_id: SessionId::new(),
                before_call_id: page.next_before_call_id,
                limit: 1,
                call_ids: None,
            }
        )
        .is_err()
    );
    store
        .database
        .unit_of_work(|tx| shell::request_stop(tx, f.turn.session_id, call, f.at))
        .unwrap();
    let stopped = shell::query(store.database.connection(), f.turn.session_id, &query).unwrap();
    assert_eq!(stopped.state, ShellCallState::Interrupted);
    assert!(stopped.started_at.is_none());
    assert_eq!(stopped.command, before.command);
    let result = tool::execution::call(store.database.connection(), call).unwrap().result;
    store
        .database
        .unit_of_work(|tx| shell::request_stop(tx, f.turn.session_id, call, f.at))
        .unwrap();
    assert_eq!(tool::execution::call(store.database.connection(), call).unwrap().result, result);
    drop(store);
    let reopened = V2Store::open(&root).unwrap();
    assert_eq!(
        shell::query(reopened.database.connection(), f.turn.session_id, &query).unwrap(),
        stopped
    );
}

#[test]
fn received_output_survives_missing_exit_and_late_snapshot_does_not_duplicate_it() {
    use aura_execution_contract::*;
    use aura_persistence_v2::execution;
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let original = ShellExecuteInput {
        requires_network: false,
        script: "Write-Output partial; exit 7".into(),
        working_directory: ".".into(),
        stdin: "original stdin".into(),
        timeout_ms: 1000,
        max_output_bytes: 4096,
    };
    let batch =
        f.admit(&mut store, vec![("shell.powershell", serde_json::to_value(&original).unwrap())]);
    let call = batch.batch.calls[0].tool_call_id;
    let mut command = original.clone().command(ManagedShell::PowerShell).unwrap();
    command.working_directory = "C:/synthetic".into();
    let request = store
        .database
        .unit_of_work(|tx| {
            f.begin_execution(
                tx,
                &batch,
                0,
                Some(("shell.powershell", serde_json::to_value(&command).unwrap())),
            )
        })
        .unwrap();
    let query =
        ShellQueryInput { tool_call_id: call, stdout_offset: 0, stderr_offset: 0, max_bytes: 4096 };
    let progress = RunnerProgress {
        attempt: request.binding.attempt.clone(),
        sequence: 1,
        observed_at: f.at,
        submitted_script: Some("bootstrap\noriginal script".into()),
        stdout: b"partial".to_vec(),
        stderr: b"err".to_vec(),
        output_truncated: false,
    };
    store
        .database
        .unit_of_work(|tx| {
            shell::append(
                tx,
                &request,
                "shell.started",
                &json!({"process_id":123,"at":f.at}),
                f.at,
            )?;
            shell::append(tx, &request, "shell.progress", &serde_json::to_value(&progress)?, f.at)
        })
        .unwrap();
    let running = shell::query(store.database.connection(), f.turn.session_id, &query).unwrap();
    assert_eq!(running.state, ShellCallState::Running);
    assert_eq!(running.original_input, original);
    assert_eq!(running.command, command);
    assert_eq!(running.stdout, b"partial");
    assert!(running.exit_code.is_none());
    // The model reads the state and the output as text, and none of the script's copies.
    let model = aura_persistence_v2::execution::shell_result::model_result(
        store.database.connection(),
        f.turn.session_id,
        call,
    )
    .unwrap();
    assert_eq!(model.state, ShellCallState::Running);
    assert_eq!((model.stdout.as_str(), model.stderr.as_str()), ("partial", "err"));
    let text = serde_json::to_string(&model).unwrap();
    assert!(!text.contains("original script") && !text.contains("submitted_script"), "{text}");
    store
        .database
        .unit_of_work(|tx| shell::lost(tx, request.operation.operation_id, f.at))
        .unwrap();
    let lost = shell::query(store.database.connection(), f.turn.session_id, &query).unwrap();
    assert_eq!(lost.state, ShellCallState::Indeterminate);
    assert_eq!(lost.stdout, running.stdout);
    assert_eq!(lost.submitted_script, running.submitted_script);
    assert!(lost.exit_code.is_none());
    let original_result =
        execution::load(store.database.connection(), request.operation.operation_id, 1).unwrap();
    let observation = RunnerObservation {
        attempt: request.binding.attempt.clone(),
        binding_fingerprint: jcs::canonical_sha256(&request.binding).unwrap(),
        account_sid: "synthetic".into(),
        process_id: 123,
        shell_image: ExecutionFileBinding {
            path: "C:/synthetic/pwsh.exe".into(),
            volume_serial: "1".into(),
            file_id: [0; 16],
            sha256: None,
        },
        started_at: f.at,
        finished_at: f.at,
        exit_code: Some(7),
        stop_reason: RunnerStopReason::Exited,
        process_tree_stopped: false,
        stdout: b"partial tail".to_vec(),
        stderr: b"err".to_vec(),
        submitted_script: progress.submitted_script.unwrap(),
        output_truncated: false,
    };
    store
        .database
        .unit_of_work(|tx| {
            shell::append(
                tx,
                &request,
                "shell.late_observation",
                &json!({"observation":observation}),
                f.at,
            )
        })
        .unwrap();
    let late = shell::query(store.database.connection(), f.turn.session_id, &query).unwrap();
    assert_eq!(late.state, ShellCallState::Indeterminate);
    assert_eq!(late.exit_code, Some(7));
    assert_eq!(late.stdout, b"partial tail");
    assert!(matches!(original_result.state, AttemptState::Terminal { .. }));
    assert_eq!(
        execution::load(store.database.connection(), request.operation.operation_id, 1).unwrap(),
        original_result
    );
    let page = shell::query(
        store.database.connection(),
        f.turn.session_id,
        &ShellQueryInput { stdout_offset: 7, max_bytes: 3, ..query },
    )
    .unwrap();
    assert_eq!(page.stdout, b" ta");
    assert_eq!(page.stdout_next, Some(10));
}

#[test]
fn shell_diagnostic_reason_is_persisted_and_queried() {
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let original = ShellExecuteInput {
        requires_network: false,
        script: "Write-Output handshake".into(),
        working_directory: ".".into(),
        stdin: String::new(),
        timeout_ms: 1000,
        max_output_bytes: 4096,
    };
    let batch =
        f.admit(&mut store, vec![("shell.powershell", serde_json::to_value(&original).unwrap())]);
    let call = batch.batch.calls[0].tool_call_id;
    let mut command = original.clone().command(ManagedShell::PowerShell).unwrap();
    command.working_directory = "C:/synthetic".into();
    let request = store
        .database
        .unit_of_work(|tx| {
            f.begin_execution(
                tx,
                &batch,
                0,
                Some(("shell.powershell", serde_json::to_value(&command).unwrap())),
            )
        })
        .unwrap();
    let reason = "runner.core_file_mismatch.path.file_id";
    store
        .database
        .unit_of_work(|tx| {
            shell::append(tx, &request, "shell.diagnostic", &json!({"reason": reason}), f.at)
        })
        .unwrap();
    let query =
        ShellQueryInput { tool_call_id: call, stdout_offset: 0, stderr_offset: 0, max_bytes: 4096 };
    let view = shell::query(store.database.connection(), f.turn.session_id, &query).unwrap();
    assert_eq!(view.diagnostic.as_deref(), Some(reason));
    assert!(view.started_at.is_none());
    assert!(view.exit_code.is_none());
}
