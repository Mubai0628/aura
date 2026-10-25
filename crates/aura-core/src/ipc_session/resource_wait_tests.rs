//! Connection task ownership, distinct from filesystem or authenticated-process coverage.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;
use std::{sync::Arc, time::Duration};

fn request(path: &str) -> JsonRpcRequest {
    use aura_workspace_domain::{FsExpectedState, FsWriteFileRequest, WorkspaceFileTarget};
    JsonRpcRequest {
        jsonrpc: aura_ipc::JSON_RPC_VERSION.into(),
        id: CorrelationId::new(),
        command_invocation_id: None,
        method: aura_ipc::method::FS_WRITE_FILE.into(),
        params: serde_json::to_value(FsWriteFileRequest {
            workspace_id: aura_domain_common::WorkspaceId::new(),
            session_id: aura_domain_common::SessionId::new(),
            target: WorkspaceFileTarget {
                path: aura_domain_common::WorkspacePath::parse(path).unwrap(),
            },
            content: "written".into(),
            expected: FsExpectedState::Absent,
            encoding: None,
            line_ending: None,
        })
        .unwrap(),
    }
}

#[tokio::test]
async fn resource_wait_terminal_is_reaped_by_the_connection() {
    let mut session = IpcSession::new();
    let request = request("held.txt");
    assert_eq!(session.admit(request.clone()), Admission::Accepted);
    let (ticket, _) = session.take_ready().pop().unwrap().into_parts();
    let (reporter, mut signals) = ResourceWaitSignals::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let handler = reporter.for_ticket(ticket);
    session.attach(
        ticket,
        tokio::spawn(async move {
            handler.entered(std::time::Instant::now() + Duration::from_secs(2));
            released.await.unwrap();
            HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Cancelled)
        }),
    );
    tokio::task::yield_now().await;
    session.apply_wait_signals(signals.drain());
    assert_eq!(session.waiting_count(), 1);
    assert_eq!(session.waiting_tickets(), vec![ticket]);
    assert_eq!(session.running_count(), 0);
    release.send(()).unwrap();
    let completed = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            let completed = session.poll_completions();
            if !completed.is_empty() {
                break completed;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a waiting task must remain owned until its terminal is reaped");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].0, request.id);
    assert_eq!(
        completed[0].1,
        HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Cancelled)
    );
    assert!(!session.is_busy());
    assert!(session.poll_completions().is_empty(), "one terminal, once");
}

#[tokio::test]
async fn resource_wait_shutdown_joins_the_parked_handler() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Ended(Arc<AtomicBool>);
    impl Drop for Ended {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let mut session = IpcSession::new();
    assert_eq!(session.admit(request("held.txt")), Admission::Accepted);
    let (ticket, _) = session.take_ready().pop().unwrap().into_parts();
    let (reporter, mut signals) = ResourceWaitSignals::channel();
    let ended = Arc::new(AtomicBool::new(false));
    let dropped = Arc::clone(&ended);
    let handler = reporter.for_ticket(ticket);
    session.attach(
        ticket,
        tokio::spawn(async move {
            let _ended = Ended(dropped);
            handler.entered(std::time::Instant::now() + Duration::from_secs(2));
            std::future::pending::<HandlerTerminal>().await
        }),
    );
    tokio::task::yield_now().await;
    session.apply_wait_signals(signals.drain());
    assert_eq!(session.waiting_count(), 1);
    assert!(session.shutdown_within(Duration::from_millis(200)).await.is_empty());
    assert!(ended.load(Ordering::Acquire), "teardown did not abandon the waiting task");
    assert!(!session.is_busy());
}

async fn reap_count(
    session: &mut IpcSession,
    count: usize,
) -> Vec<(CorrelationId, HandlerTerminal)> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut completed = Vec::new();
        while completed.len() < count {
            completed.extend(session.poll_completions());
            tokio::task::yield_now().await;
        }
        completed
    })
    .await
    .expect("tracked tasks reached their terminals")
}

#[tokio::test]
async fn resource_wait_retries_reacquire_bounded_capacity_and_leave_room_for_unrelated_work() {
    let mut session = IpcSession::new();
    let (reporter, mut signals) = ResourceWaitSignals::channel();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let gate = Arc::new(tokio::sync::Notify::new());
    let mut finishers = Vec::new();
    for _ in 0..MAX_CONCURRENT_SESSION_WRITES {
        assert_eq!(session.admit(request("held.txt")), Admission::Accepted);
    }
    for started in session.take_ready() {
        let (ticket, _) = started.into_parts();
        let handler = reporter.for_ticket(ticket);
        let gate = Arc::clone(&gate);
        let (finish, finished) = tokio::sync::oneshot::channel();
        finishers.push(finish);
        session.attach(
            ticket,
            tokio::spawn(async move {
                handler.entered(deadline);
                gate.notified().await;
                assert!(handler.resume(deadline).await, "the retry reacquired a slot");
                finished.await.unwrap();
                HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Succeeded)
            }),
        );
    }
    tokio::task::yield_now().await;
    session.apply_wait_signals(signals.drain());
    assert_eq!(session.waiting_count(), MAX_CONCURRENT_SESSION_WRITES);
    assert_eq!(session.running_count(), 0);
    let unrelated = request("unrelated.txt");
    assert_eq!(session.admit(unrelated.clone()), Admission::Accepted);
    gate.notify_waiters();
    tokio::task::yield_now().await;
    session.apply_wait_signals(signals.drain());
    let ready = session.take_ready();
    assert_eq!(ready.len(), 1, "resume grants never dispatch the request a second time");
    assert_eq!(ready[0].request_id(), unrelated.id);
    let (ticket, _) = ready.into_iter().next().unwrap().into_parts();
    let (finish_unrelated, finished_unrelated) = tokio::sync::oneshot::channel();
    session.attach(
        ticket,
        tokio::spawn(async move {
            finished_unrelated.await.unwrap();
            HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Succeeded)
        }),
    );
    assert_eq!(session.running_count(), MAX_CONCURRENT_SESSION_WRITES);
    assert_eq!(session.waiting_count(), 1, "eight retries plus new work cannot exceed eight slots");
    finish_unrelated.send(()).unwrap();
    let first = reap_count(&mut session, 1).await;
    assert_eq!(first[0].0, unrelated.id);
    assert!(session.take_ready().is_empty(), "resumption wakes an existing handler");
    assert_eq!(session.waiting_count(), 0);
    assert_eq!(session.running_count(), MAX_CONCURRENT_SESSION_WRITES);
    for finish in finishers {
        finish.send(()).unwrap();
    }
    assert_eq!(reap_count(&mut session, MAX_CONCURRENT_SESSION_WRITES).await.len(), 8);
    assert!(!session.is_busy());
}

#[tokio::test]
async fn resource_wait_keeps_same_target_order_and_does_not_extend_its_deadline() {
    let mut session = IpcSession::new();
    let first = request("same.txt");
    let mut later = first.clone();
    later.id = CorrelationId::new();
    let mut sibling = first.clone();
    sibling.id = CorrelationId::new();
    sibling.params["target"]["path"] = serde_json::json!("sibling.txt");
    assert_eq!(session.admit(first.clone()), Admission::Accepted);
    let (ticket, _) = session.take_ready().pop().unwrap().into_parts();
    let (reporter, mut signals) = ResourceWaitSignals::channel();
    let handler = reporter.for_ticket(ticket);
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let observe = handler.clone();
    let (resume, resumed) = tokio::sync::oneshot::channel();
    let (finish, finished) = tokio::sync::oneshot::channel();
    session.attach(
        ticket,
        tokio::spawn(async move {
            handler.entered(deadline);
            handler.entered(deadline + Duration::from_secs(10));
            resumed.await.unwrap();
            assert!(handler.resume(deadline + Duration::from_secs(10)).await);
            finished.await.unwrap();
            HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Succeeded)
        }),
    );
    tokio::task::yield_now().await;
    session.apply_wait_signals(signals.drain());
    assert_eq!(observe.deadline(), Some(deadline));
    assert_eq!(session.admit(later.clone()), Admission::Accepted);
    assert_eq!(session.admit(sibling.clone()), Admission::Accepted);
    let ready = session.take_ready();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].request_id(), sibling.id, "same-target later request did not overtake");
    let (sibling_ticket, _) = ready.into_iter().next().unwrap().into_parts();
    session.attach(
        sibling_ticket,
        tokio::spawn(async {
            HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Succeeded)
        }),
    );
    reap_count(&mut session, 1).await;
    resume.send(()).unwrap();
    tokio::task::yield_now().await;
    session.apply_wait_signals(signals.drain());
    assert!(session.take_ready().is_empty(), "the waiting original resumes first");
    finish.send(()).unwrap();
    let completed = reap_count(&mut session, 1).await;
    assert_eq!(completed[0].0, first.id);
    let ready = session.take_ready();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].request_id(), later.id);
    session.shutdown().await;
}
