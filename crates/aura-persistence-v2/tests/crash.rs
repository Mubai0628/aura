use aura_domain_common::{EventId, SecurityOperationId, UtcTimestamp};
use aura_persistence_core::{
    PersistenceCoreError, event,
    operation::{self, OperationDraft, OperationState},
};
use aura_persistence_v2::V2Store;
use serde_json::json;

#[test]
fn process_crash_rolls_back_uncommitted_work_and_preserves_dispatch_for_reconciliation() {
    if let Some(root) = std::env::var_os("AURA_R02_CRASH_TEST_ROOT") {
        let mut store = V2Store::open(std::path::Path::new(&root)).expect(
            "process crash rolls back uncommitted work and preserves dispatch for reconciliation",
        );
        let before = std::env::var("AURA_R02_CRASH_PHASE").expect(
            "process crash rolls back uncommitted work and preserves dispatch for reconciliation",
        ) == "before_commit";
        let at: UtcTimestamp = "2026-09-07T01:00:00Z".parse().expect(
            "process crash rolls back uncommitted work and preserves dispatch for reconciliation",
        );
        let id = SecurityOperationId::new();
        store
            .database
            .unit_of_work(|tx| {
                operation::begin(
                    tx,
                    &OperationDraft {
                        id,
                        domain: "crash",
                        kind: "fixture",
                        request_key: "once",
                        version: 1,
                        payload: &json!({"version":1}),
                        at,
                    },
                )?;
                operation::transition(tx, id, 1, OperationState::Running, at)?;
                event::append(
                    tx,
                    &event::EventDraft {
                        id: EventId::new(),
                        owner: "crash",
                        kind: "prepared",
                        version: 1,
                        payload: &json!({"version":1}),
                        at,
                    },
                    "crash",
                )?;
                if before {
                    std::process::exit(23);
                }
                Ok::<_, PersistenceCoreError>(())
            })
            .expect("process crash rolls back uncommitted work and preserves dispatch for reconciliation");
        std::process::exit(23);
    }
    let temp = tempfile::tempdir().expect(
        "process crash rolls back uncommitted work and preserves dispatch for reconciliation",
    );
    for phase in ["before_commit", "after_commit"] {
        let root = temp.path().join(phase);
        drop(V2Store::create(&root).expect(
            "process crash rolls back uncommitted work and preserves dispatch for reconciliation",
        ));
        let status = std::process::Command::new(std::env::current_exe().expect(
            "process crash rolls back uncommitted work and preserves dispatch for reconciliation",
        ))
        .args([
            "--exact",
            "process_crash_rolls_back_uncommitted_work_and_preserves_dispatch_for_reconciliation",
            "--nocapture",
        ])
        .env("AURA_R02_CRASH_TEST_ROOT", &root)
        .env("AURA_R02_CRASH_PHASE", phase)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect(
            "process crash rolls back uncommitted work and preserves dispatch for reconciliation",
        );
        assert_eq!(status.code(), Some(23));
        let store = V2Store::open(&root).expect(
            "process crash rolls back uncommitted work and preserves dispatch for reconciliation",
        );
        let expected = usize::from(phase == "after_commit");
        assert_eq!(operation::recoverable(store.database.connection()).expect("process crash rolls back uncommitted work and preserves dispatch for reconciliation").len(), expected);
        assert_eq!(event::pending(store.database.connection(), 10).expect("process crash rolls back uncommitted work and preserves dispatch for reconciliation").len(), expected);
    }
}
