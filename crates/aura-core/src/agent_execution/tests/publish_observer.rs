//! The observer distinguishes two legitimate calls publishing identical content.
//!
//! WHY THIS CASE EXISTS
//! `isolation-scenario-d7.mjs` used to map "the target file exists and its hash matches the bytes
//! recorded at arrival" directly to a write count of one. That is a derived **file state**: a
//! second write of the same content can leave the same bytes. This case drives two independently
//! admitted calls on two targets through the real broker and observes one publication per identity.
//! It verifies the observer, not replay of the original identity. The acceptance predicate's separate
//! regressions reject repeated original-identity records even when their target and bytes agree.
//!
//! WHAT IS REAL HERE
//! The production execution service, two independent admitted tool calls, two real `fs.write`
//! invocations, two real on-disk publications and the real ledger. The observer is the default-off
//! acceptance observer; this case binds it to a temporary directory directly instead of through a
//! control document, so nothing about the product's environment or behaviour changes.
#![cfg(feature = "acceptance-barrier")]
use super::*;
use aura_core_broker::agent_store;
use serde_json::Value;

/// The observer's own journal file, beside where a control document would live.
const OBSERVER_FILE: &str = "agent-execution-journal.jsonl";

fn entries(directory: &std::path::Path) -> Vec<Value> {
    let text =
        std::fs::read_to_string(directory.join(OBSERVER_FILE)).expect("the observer journal");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("one observer entry per line"))
        .collect()
}

#[tokio::test]
async fn s04_r7_observer_counts_two_independent_identical_writes() {
    let directory = tempfile::tempdir().expect("observer directory");
    agent_store::acceptance_hold::bind_control_directory_for_test(directory.path());
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let text = "the same bytes, twice";
    // Two admitted calls in one real batch, both publishing the identical content.
    let batch = fixture.admit(&[
        (
            "fs.write",
            json!({"path":"same-content-a.txt","text":text,"expected":{"expected_kind":"absent"}}),
        ),
        (
            "fs.write",
            json!({"path":"same-content-b.txt","text":text,"expected":{"expected_kind":"absent"}}),
        ),
    ]);
    let identities =
        batch.batch.calls.iter().map(|call| call.tool_call_id.to_string()).collect::<Vec<_>>();
    assert_eq!(identities.len(), 2, "two real calls were admitted");
    fixture.execute(batch).await;

    // The two files really hold the same bytes: no file-state derivation can tell them apart.
    let first = std::fs::read(fixture.root.join("same-content-a.txt")).expect("first bytes");
    let second = std::fs::read(fixture.root.join("same-content-b.txt")).expect("second bytes");
    assert_eq!(first, second, "both publications really landed the same bytes");
    assert_eq!(first, text.as_bytes());

    let recorded = entries(directory.path());
    // The observer is one process-wide journal, so a write by another test running at the same
    // time lands in it too. Only this case's two calls count.
    let writes: Vec<&Value> = recorded
        .iter()
        .filter(|entry| entry["action_kind"] == "fs.write" && entry["backend_entered"] == true)
        .filter(|entry| {
            identities.iter().any(|id| entry["tool_call_id"] == Value::String(id.clone()))
        })
        .collect();
    let published = writes.iter().filter(|entry| entry["published"] == true).count();
    let per_identity = identities
        .iter()
        .map(|id| {
            writes.iter().filter(|entry| entry["tool_call_id"] == Value::String(id.clone())).count()
        })
        .collect::<Vec<_>>();
    println!(
        "R7 backend_entered_fs_write={} published={published} per_identity={per_identity:?} \
         identical_bytes=true observer={OBSERVER_FILE}",
        writes.len()
    );
    assert_eq!(writes.len(), 2, "two real backend invocations must be two records");
    assert_eq!(published, 2, "two real publications must be two records");
    assert_eq!(per_identity, vec![1, 1], "each call's own identity carries its own record");
    let states = writes.iter().map(|entry| &entry["post_state"]).collect::<Vec<_>>();
    assert_eq!(states.len(), 2);
    for state in &states {
        assert_ne!(**state, Value::Null, "the post-state is a real observation, not a silence");
    }
    assert_ne!(
        states[0], states[1],
        "each record keeps its own target's real post-state, so the two publications stay \
         distinguishable"
    );
}
