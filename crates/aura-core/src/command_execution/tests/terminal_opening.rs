//! A real user-open request must stop while its launch is still in the blocking queue.
use super::*;
use crate::{
    AsyncPriorityQueue, JsonRpcRequest, ProjectionSubscriptions, RequestLedger, StartupMode,
};
use aura_runtime_contract::IpcRequestTerminal;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Release {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[test]
fn l03_b0_cancel_pool_queued_user_open_finishes_cancelled_and_never_launches() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let host = crate::r03_tests::Fixture::new();
            let state = Arc::new(host.start().await);
            let StartupMode::Ready(services) = &state.startup_mode else { panic!("ready") };
            let created = crate::r03_tests::success(
                &state,
                "workspace.create",
                serde_json::json!({"display_name":"cancel opening","selected_root":host.root.path()}),
            ).await;
            let workspace: aura_workspace_domain::WorkspaceSummary =
                serde_json::from_value(created["workspace"].clone()).unwrap();
            let session = SessionId::new();
            services.persistence.upsert_workspace_session(
                workspace.clone(),
                aura_workspace_domain::SessionSummary {
                    session_id: session, workspace_id: workspace.workspace_id,
                    title: "cancel opening".into(), state: "idle".into(),
                }, CorrelationId::new(),
            ).unwrap();
            let commands = services.commands_required();
            commands.compat.wait_until_ready().await.unwrap();
            let request = JsonRpcRequest {
                jsonrpc: crate::JSON_RPC_VERSION.into(), id: CorrelationId::new(),
                command_invocation_id: Some(CommandInvocationId::new()),
                method: "terminal.user.open".into(),
                params: serde_json::json!({"session_id":session,"shell":"cmd","columns":80,"rows":24}),
            };
            let queue = AsyncPriorityQueue::new();
            let observer = queue.clone();
            let claim = match crate::reserve_command_invocation(&state, &queue, &request).await.unwrap() {
                crate::CommandReservation::Granted(claim) => claim,
                crate::CommandReservation::Refused => panic!("own invocation"),
            };
            let cancel = JsonRpcRequest {
                jsonrpc: crate::JSON_RPC_VERSION.into(), id: CorrelationId::new(),
                command_invocation_id: Some(CommandInvocationId::new()), method: "aura.cancel".into(),
                params: serde_json::json!({"request_id":request.id}),
            };
            let cancel_queue = AsyncPriorityQueue::new();
            let cancel_claim = match crate::reserve_command_invocation(&state, &cancel_queue, &cancel).await.unwrap() {
                crate::CommandReservation::Granted(claim) => claim,
                crate::CommandReservation::Refused => panic!("own cancel invocation"),
            };
            let ledger = Arc::new(Mutex::new(RequestLedger::new()));
            ledger.lock().unwrap().begin(request.id).unwrap();
            ledger.lock().unwrap().mark_started(request.id).unwrap();
            let blocked = Arc::new((Mutex::new(false), Condvar::new()));
            let release = Release(blocked.clone());
            let (entered, entered_rx) = tokio::sync::oneshot::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                entered.send(()).unwrap();
                let mut allowed = blocked.0.lock().unwrap();
                while !*allowed { allowed = blocked.1.wait(allowed).unwrap(); }
            });
            entered_rx.await.unwrap();
            let (invoked, invoked_rx) = tokio::sync::oneshot::channel();
            let open_state = state.clone();
            let open_ledger = ledger.clone();
            let open_request = request.clone();
            let mut opened = tokio::spawn(async move {
                invoked.send(()).unwrap();
                crate::handle_request(&open_state, &queue, &ProjectionSubscriptions::default(),
                    &open_ledger, &open_request, claim.as_ref()).await
            });
            invoked_rx.await.unwrap();
            tokio::task::yield_now().await;
            let crate::WireFrame::Control(progress) = tokio::time::timeout(Duration::from_secs(2), observer.recv()).await.unwrap().unwrap().frame
            else { panic!("opening notification") };
            let progress: serde_json::Value = serde_json::from_slice(&progress).unwrap();
            let cancelled = crate::handle_request(&state, &cancel_queue,
                &ProjectionSubscriptions::default(), &ledger, &cancel, cancel_claim.as_ref()).await.unwrap();
            let crate::WireFrame::Control(cancel_response) = cancel_queue.recv().await.unwrap().frame
            else { panic!("cancel response") };
            let cancel_response: serde_json::Value = serde_json::from_slice(&cancel_response).unwrap();
            // Observe before releasing the blocking slot. Timeout borrows the task,
            // preserving ownership so the old-code red path can clean up its child.
            let early = tokio::time::timeout(Duration::from_millis(750), &mut opened).await;
            drop(release);
            blocker.await.unwrap();
            let finished_early = early.is_ok();
            let terminal = match early {
                Ok(result) => result.unwrap().unwrap(),
                Err(_) => opened.await.unwrap().unwrap(),
            };
            let terminals = commands.compat.terminals.list(session).unwrap();
            commands.shutdown();
            assert_eq!(cancelled, IpcRequestTerminal::Succeeded, "the test must execute the authenticated cancel handler: {cancel_response}");
            assert_eq!(cancel_response["result"]["value"], "accepted");
            assert!(finished_early, "cancellation must finish without waiting for blocking-pool capacity");
            assert_eq!(terminal, IpcRequestTerminal::Cancelled, "accepted cancellation is an actual original-request terminal");
            assert!(terminals.is_empty(), "a cancelled queued opening must never create a terminal");
            assert_eq!(progress["method"], "terminal.opening.v1");
            assert_eq!(progress["params"]["phase"], "waiting_launch", "a verified runtime must not claim to be checking while only pool capacity is unavailable");
        });
}
