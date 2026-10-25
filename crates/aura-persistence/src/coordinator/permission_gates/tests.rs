use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::time::Duration;

fn poison_registry(gates: &Arc<PermissionGates>) {
    let gates = Arc::clone(gates);
    assert!(
        std::thread::spawn(move || {
            let _registry = gates.sessions.lock().expect("registry held");
            panic!("injected registry fault");
        })
        .join()
        .is_err()
    );
}

#[test]
fn registry_fault_does_not_let_a_writer_bypass_its_sessions_reader() {
    let gates = Arc::new(PermissionGates::default());
    let session = SessionId::new();
    let gate = gates.of(session);
    let held = gate.authority.read().expect("reader held");
    poison_registry(&gates);
    let writing = Arc::clone(&gates);
    let (entered, received) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let gate = writing.of(session);
        let _write = gate.authority.write().expect("same authority remains usable");
        entered.send(()).expect("observe acquisition");
    });
    let bypassed = received.recv_timeout(Duration::from_millis(200)).is_ok();
    drop(held);
    writer.join().expect("writer released");
    assert!(!bypassed, "a registry fault must not replace live session authority");
    if !bypassed {
        received.recv_timeout(Duration::from_secs(1)).expect("writer acquired after release");
    }
}

#[tokio::test]
async fn registry_fault_preserves_a_live_start_lease() {
    let gates = Arc::new(PermissionGates::default());
    let session = SessionId::new();
    let held = gates.of(session).start.read_owned().await;
    poison_registry(&gates);
    let changing = gates.of(session).start.write_owned();
    tokio::pin!(changing);
    let bypassed = tokio::time::timeout(Duration::from_millis(50), &mut changing).await.is_ok();
    drop(held);
    assert!(!bypassed, "the same session's change must still wait for its start");
    let _changed = changing.await;
}

#[test]
fn writer_unwind_poison_is_local_and_cannot_be_reset_by_lookup() {
    let gates = PermissionGates::default();
    let session = SessionId::new();
    let gate = gates.of(session);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _held = gate.authority.write().expect("writer held");
            panic!("injected authority writer fault");
        }))
        .is_err()
    );
    drop(gate);
    for _ in 0..PERMISSION_GATE_SESSIONS {
        let _idle = gates.of(SessionId::new());
    }
    assert!(matches!(gates.of(session).authority.read(), Err(PersistenceError::ReadLockPoisoned)));
    assert!(matches!(gates.of(session).authority.write(), Err(PersistenceError::ReadLockPoisoned)));
    let other = gates.of(SessionId::new());
    let _unrelated = other.authority.write().expect("unrelated session remains usable");
}

#[test]
fn reader_unwind_releases_without_poisoning_authority() {
    let gates = PermissionGates::default();
    let session = SessionId::new();
    let gate = gates.of(session);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _held = gate.authority.read().expect("reader held");
            panic!("injected authority reader fault");
        }))
        .is_err()
    );
    let _next = gates.of(session).authority.write().expect("reader fault did not poison authority");
}
