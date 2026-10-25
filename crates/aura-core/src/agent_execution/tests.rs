use super::*;
use serde_json::json;
mod fixture;
use fixture::Fixture;
mod activity_isolation;
mod approval_recovery;
mod backend_selection;
mod command_mode;
mod common_admission;
mod control_isolation;
mod l01_user_proxy;
#[cfg(feature = "p08-contract-driver")]
mod l03_git;
#[cfg(feature = "p08-contract-driver")]
mod l03_git_remotes;
#[cfg(feature = "p08-contract-driver")]
mod l03_git_writes;
mod l04_packages;
mod l05_memory;
mod l06_commands;
mod l06_init;
mod l06_memory_consents;
mod l08_mcp;
mod l09_skills;
mod l13_1_git_feedback;
mod publish_observer;
mod read_rejection;
mod registry_context;
mod s05_tool_switch;
/// The tool switch through the real enablement authority.
mod s05r_tool_enablement;
mod s06;
mod s06_review;
mod s07;
mod s08;
mod s09_defaults;
mod s10_argument_followups;
mod s10_completion_defaults;
mod s10_failure_causes;
mod stage_log;
// A call its backend refused before it started still gets a result.
mod l10_14_refused_before_dispatch;

#[tokio::test]
async fn real_file_write_has_atomic_attempt_result_and_history() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"result.txt","text":"real effect","expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let operation = batch.calls[&id].operation.clone();
    let replay = f
        .service()
        .agent_execution_required()
        .admit_output_sync(f.step, f.settings.clone())
        .expect("Agent test setup and persisted assertion");
    assert_eq!(replay.calls[&id].operation, operation);
    let group = f.execute(batch).await;
    let restored = f
        .service()
        .agent_execution_required()
        .resume_batch(batch_id)
        .expect("Agent test setup and persisted assertion");
    assert_eq!(restored.calls[&id].operation, operation);
    assert_eq!(f.execute(restored).await, group);
    assert_eq!(
        std::fs::read(f.root.join("result.txt")).expect("Agent test setup and persisted assertion"),
        b"real effect"
    );
    let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    let db = store.database.connection();
    let call = aura_persistence_v2::tool::execution::call(db, id)
        .expect("Agent test setup and persisted assertion");
    assert!(call.attempt_id.is_some());
    assert!(matches!(
        call.result.expect("Agent test setup and persisted assertion").outcome,
        ToolResultOutcomeV1::Succeeded
    ));
    assert_eq!(
        aura_persistence_v2::context::history(db, f.turn.session_id)
            .expect("Agent test setup and persisted assertion"),
        vec![group]
    );
}

#[tokio::test]
async fn cancellation_before_dispatch_is_paired_without_an_attempt() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"cancelled.txt","text":"no","expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let (_sender, cancel) = tokio::sync::watch::channel(true);
    assert!(matches!(
        f.service()
            .agent_execution_required()
            .execute_batch(batch, f.service().commands_required().clone(), cancel)
            .await
            .expect("Agent test setup and persisted assertion"),
        BatchOutcome::Completed(_)
    ));
    let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("Agent test setup and persisted assertion");
    assert!(call.attempt_id.is_none());
    assert!(matches!(
        call.result.expect("Agent test setup and persisted assertion").outcome,
        ToolResultOutcomeV1::NotExecuted {
            reason: ToolNotExecutedReasonV1::CancelledBeforeDispatch,
            ..
        }
    ));
    assert!(!f.root.join("cancelled.txt").exists());
}

/// A write the session may not make without an answer never reaches a backend.
///
/// Two different protections are exercised, and they must stay distinguishable:
///
/// * **ReadOnly writing a workspace file** is a default denial the user *may* answer. The
///   confirmed rule is that the mode is a default rather than a ceiling, so what this must
///   produce is an **approval request**, and — the part that matters for safety — still no
///   attempt and no file until that request is answered. The migrated expectation used to be
///   `PermissionDenied`, which encoded the mode as unbreakable and made the confirmed rule
///   unreachable.
/// * **A path escape** (`../escape.txt`) is not approvable at all: it is outside any scope the
///   session could grant, so it stays a refusal with no attempt.
#[tokio::test]
async fn readonly_and_path_escape_never_obtain_write_attempts() {
    for (mode, path) in [
        (PermissionMode::ReadOnly, "denied.txt"),
        (PermissionMode::WorkspaceReadWrite, "../escape.txt"),
    ] {
        let f = Fixture::new(mode).await;
        let batch = f.admit(&[(
            "fs.write",
            json!({"path":path,"text":"no","expected":{"expected_kind":"absent"}}),
        )]);
        let id = batch.batch.calls[0].tool_call_id;
        let outcome = f.execute_rejecting(batch).await;
        let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
        let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
            .expect("Agent test setup and persisted assertion");
        // Neither protection may reach a backend before it is answered: no attempt either way.
        assert!(call.attempt_id.is_none());
        match mode {
            PermissionMode::ReadOnly => {
                assert!(
                    matches!(outcome, Some(BatchOutcome::AwaitingApproval { .. })),
                    "ReadOnly asks for a precise approval instead of refusing outright"
                );
                // A call waiting for its answer has not produced a result yet: the absence of
                // one is the honest state, and it is also why there is nothing to have written.
                assert!(call.result.is_none(), "a call awaiting approval has no result to report");
            }
            _ => {
                let reason = ToolNotExecutedReasonV1::ResourceClaimResolutionFailed;
                assert!(
                    matches!(call.result.expect("Agent test setup and persisted assertion").outcome, ToolResultOutcomeV1::NotExecuted { reason: actual, .. } if actual==reason)
                );
            }
        }
        // The invariant that motivated this test is unchanged and is the reason it still runs:
        // nothing was written while the call had no authority to write.
        assert!(!f.root.join(path).exists());
    }
}

/// `fs.write` must not transcode a file it overwrites. The model-facing input has
/// no encoding field, so a UTF-16LE file has to stay UTF-16LE; only a new file is
/// created as UTF-8 with LF.
#[tokio::test]
async fn fs_write_preserves_an_existing_files_encoding_and_creates_utf8() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let existing = "alpha\r\nbeta\r\n";
    let mut utf16 = vec![0xff_u8, 0xfe];
    for unit in existing.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(f.root.join("utf16.txt"), &utf16)
        .expect("Agent test setup and persisted assertion");
    let before =
        std::fs::read(f.root.join("utf16.txt")).expect("Agent test setup and persisted assertion");
    let batch = f.admit(&[
        (
            "fs.write",
            json!({
                "path":"utf16.txt",
                "text":"gamma\r\ndelta\r\n",
                "expected":{"expected_kind":"file_hash","hash":aura_domain_common::ContentHash::blake3(&before)}
            }),
        ),
        (
            "fs.write",
            json!({"path":"created.txt","text":"new\n","expected":{"expected_kind":"absent"}}),
        ),
    ]);
    f.execute(batch).await;
    let mut expected = vec![0xff_u8, 0xfe];
    for unit in "gamma\r\ndelta\r\n".encode_utf16() {
        expected.extend_from_slice(&unit.to_le_bytes());
    }
    assert_eq!(
        std::fs::read(f.root.join("utf16.txt")).expect("Agent test setup and persisted assertion"),
        expected,
        "overwriting an existing file must keep its encoding and line endings"
    );
    assert_eq!(
        std::fs::read(f.root.join("created.txt"))
            .expect("Agent test setup and persisted assertion"),
        b"new\n",
        "a new file must be created as UTF-8 with LF"
    );
}

#[tokio::test]
async fn model_complete_does_not_require_review() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let batch = f.admit(&[(
        "normal.complete",
        json!({"lineage_id":f.turn.lineage_id,"expected_lineage_revision":Revision::INITIAL,"final_report":"Finished by model decision","disclosed_limitations":[]}),
    )]);
    let group = f.execute(batch).await;
    let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    let db = store.database.connection();
    assert!(
        aura_persistence_v2::agent::turns::load(db, f.turn.turn_task_id)
            .expect("Agent test setup and persisted assertion")
            .state
            .terminal()
    );
    let call = aura_persistence_v2::tool::execution::call(db, group.calls[0].tool_call_id)
        .expect("Agent test setup and persisted assertion");
    assert!(matches!(
        call.result.expect("Agent test setup and persisted assertion").outcome,
        ToolResultOutcomeV1::Succeeded
    ));
}

#[tokio::test]
async fn completion_group_publishes_real_lineage_and_idempotent_history() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let batch = f.admit(&[
        ("task_lineage.review_completion", json!({"lineage_id":f.turn.lineage_id,"expected_lineage_revision":Revision::INITIAL,"source_read_receipt_id":null,"assessment":aura_agent_domain::CompletionAssessment::FullySatisfied,"reviewed_user_intents":[{"description":"Verified","source_event_ids":[f.turn.root_user_event_id],"result":"satisfied","evidence_refs":[]}],"evidence_refs":[]})),
        ("normal.complete", json!({"lineage_id":f.turn.lineage_id,"expected_lineage_revision":Revision::INITIAL,"final_report":"Verified","disclosed_limitations":[]})),
    ]);
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let mut store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    assert!(
        aura_persistence_v2::agent::turns::load(store.database.connection(), f.turn.turn_task_id)
            .expect("Agent test setup and persisted assertion")
            .state
            .terminal()
    );
    let replay = store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::agent::history::complete_batch(tx, batch_id, UtcTimestamp::now())
        })
        .expect("Agent test setup and persisted assertion");
    assert_eq!(replay, group);
    assert_eq!(
        aura_persistence_v2::context::history(store.database.connection(), f.turn.session_id)
            .expect("Agent test setup and persisted assertion")
            .len(),
        1
    );
}

/// The object a policy-authorized external operation acts on is the one it was
/// classified against.
///
/// The call is admitted against a real external file — that is where classification
/// records the object's identity — and the object is then replaced by a different file
/// with the same name before dispatch. The substituted object has a different identity, so
/// it is a different object: the operation must be refused rather than read, because the
/// policy decided about the first one. Opening by path and re-checking afterwards would
/// already have the wrong bytes in hand.
#[tokio::test]
async fn an_external_object_replaced_after_classification_is_never_read() {
    let f = Fixture::with_exposure(
        PermissionMode::FullAccess,
        &["fs.read", "task_lineage.review_completion", "normal.complete"],
    )
    .await;
    let outside = f.root.parent().expect("host root").join("outside-identity-swap");
    std::fs::create_dir_all(&outside).expect("external root");
    let target = outside.join("classified.txt");
    std::fs::write(&target, "classified body").expect("classified file");
    let batch = f.admit(&[(
        "fs.read",
        json!({
            "path": target.display().to_string(),
            "range": {"range_kind":"full"},
            "explicit_encoding": null
        }),
    )]);
    let id = batch.batch.calls[0].tool_call_id;

    // Same name, different object: the identity the classification recorded is gone.
    std::fs::remove_file(&target).expect("remove the classified object");
    std::fs::write(&target, "substituted body").expect("substituted file");

    let group = f.execute(batch).await;
    // The batch's own record carries what each call did. A call refused before dispatch
    // has no attempt, so the refusal is read from the group rather than from an
    // execution row that was never written.
    let entry = group
        .calls
        .iter()
        .find(|entry| entry.tool_call_id == id)
        .expect("the refused call is part of the batch record");
    let record = serde_json::to_string(entry).expect("call record");
    assert!(
        !record.contains("\"succeeded\""),
        "a substituted object must not be read as the authorized one: {record}"
    );
    assert!(
        !record.contains("substituted body"),
        "the substituted bytes must never reach the caller: {record}"
    );
}

/// Two calls that publish to the same external object cannot interleave.
///
/// Both are admitted against the same target and both expect it to be absent, so exactly one
/// of them can publish. A batch runs its calls concurrently, which is what makes this a real
/// interleaving rather than a sequence: one call wins, the other is refused with the
/// expected-state conflict, and the file holds exactly one of the two bodies — never a mix of
/// them, and never both publications landing.
#[tokio::test]
async fn parallel_external_writes_to_one_object_cannot_interleave() {
    let f = Fixture::with_exposure(
        PermissionMode::FullAccess,
        &["fs.write", "task_lineage.review_completion", "normal.complete"],
    )
    .await;
    let outside = f.root.parent().expect("host root").join("outside-parallel");
    std::fs::create_dir_all(&outside).expect("external root");
    let target = outside.join("contended.txt");
    let batch = f.admit(&[
        (
            "fs.write",
            json!({
                "path": target.display().to_string(),
                "text": "first body",
                "expected": {"expected_kind": "absent"}
            }),
        ),
        (
            "fs.write",
            json!({
                "path": target.display().to_string(),
                "text": "second body",
                "expected": {"expected_kind": "absent"}
            }),
        ),
    ]);
    let group = f.execute(batch).await;
    let body = std::fs::read_to_string(&target).expect("the winning body must exist");
    assert!(
        body == "first body" || body == "second body",
        "the target holds one complete body, never a mix: {body:?}"
    );
    let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    let mut outcomes = Vec::new();
    for call in &group.calls {
        let record = aura_persistence_v2::tool::execution::call(
            store.database.connection(),
            call.tool_call_id,
        )
        .expect("Agent test setup and persisted assertion");
        outcomes.push(record.result.expect("Agent test setup and persisted assertion").outcome);
    }
    let succeeded =
        outcomes.iter().filter(|outcome| matches!(outcome, ToolResultOutcomeV1::Succeeded)).count();
    assert_eq!(succeeded, 1, "exactly one publication wins: {outcomes:?}");
    let refused = outcomes
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                ToolResultOutcomeV1::Failed { .. } | ToolResultOutcomeV1::NotExecuted { .. }
            )
        })
        .count();
    assert_eq!(refused, 1, "the other call is refused rather than published: {outcomes:?}");
}
