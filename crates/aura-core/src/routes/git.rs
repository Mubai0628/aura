use crate::*;
use aura_core_base::sync_boundary::{SyncSection, SyncSubject};
pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    ledger: &Arc<Mutex<RequestLedger>>,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    let outcome: Result<serde_json::Value, IpcErrorEnvelope> = async {
        let StartupMode::Ready(services) = &state.startup_mode else { return Err(broker::error_envelope(request.id, "core.recovery_required")); };
        if request.method == "git.runtime.get" {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(request.params.clone()).map_err(|error| broker::error_envelope(request.id, error))?;
            return match state.sync.run(SyncSection::Inspect, SyncSubject::new().request(request.id).method(&request.method),
                || aura_git_service::git::runtime::status().and_then(|value| Ok(serde_json::to_value(value)?))).await {
                aura_core_base::blocking_pool::BlockingAdmission::Completed(value) => value.map_err(|error| aura_git_service::git::envelope(request.id, error)),
                _ => Err(broker::error_envelope(request.id, "git.detection_pending: 检测尚未完成，请稍后重新检测")),
            };
        }
        if request.method != "git.workspace.read" { return Err(broker::error_envelope(request.id, "git.unknown_method")); }
        let token = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let guard = aura_git_service::git::CancelOnDrop(token.clone());
        let work = broker::dispatch_git(state, services, request, token);
        tokio::pin!(work);
        loop {
            tokio::select! {
                result = &mut work => return result,
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    let cancelled = ledger.lock().map_err(|_| broker::error_envelope(request.id, "git.request_ledger"))?.cancellation_requested(request.id);
                    if cancelled { guard.0.store(true, std::sync::atomic::Ordering::Release); }
                }
            }
        }
    }.await;
    let (terminal, response) = match outcome {
        Ok(value) => (IpcRequestTerminal::Succeeded, JsonRpcResponse::success(request.id, value)),
        Err(error) => (error.terminal, JsonRpcResponse::failure(request.id, error)),
    };
    send_recorded_response(state, queue, request, response).await?;
    Ok(terminal)
}
