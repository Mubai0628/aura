//! Dedicated user-opening gate first; every other cancellation keeps its original semantics.
use crate::*;
use aura_runtime_contract::CancelDisposition;

pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    ledger: &Arc<Mutex<RequestLedger>>,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    let cancellation: CancellationRequest =
        serde_json::from_value(request.params.clone()).context("invalid aura.cancel params")?;
    let (opening, general) = {
        // Registration takes this same ledger lock before installing its gate.
        // An accepted queued intent is therefore visible even before registration.
        let mut requests =
            ledger.lock().map_err(|_| anyhow::anyhow!("request ledger was poisoned"))?;
        let opening = state.terminal_openings.cancel(
            cancellation.request_id,
            requests.terminal(cancellation.request_id).is_some(),
            ledger,
        )?;
        match opening {
            Some(CancelDisposition::Accepted) => {
                requests.cancel(cancellation.request_id);
                (opening, None)
            }
            Some(_) => (opening, None), // Launching is too late; do not write cancel intent.
            None => (None, Some(requests.cancel(cancellation.request_id))),
        }
    };
    let disposition = if let Some(opening) = opening {
        opening
    } else {
        let outcome = general.context("missing cancellation classification")?;
        // Accepted remains intent-only for all existing non-opening handlers.
        aura_ipc::trace(
            "core.cancel.recorded",
            &format!(
                "id={} target={} outcome={outcome:?} disposition={:?}",
                request.id,
                cancellation.request_id,
                outcome.disposition(),
            ),
        );
        aura_fs_tools::cancel_search(cancellation.request_id);
        // A pre-effect file resource wait observes this mark before it starts;
        // an already-started effect retains its completion/recovery responsibility.
        if matches!(outcome, aura_ipc::CancellationOutcome::Executing) {
            aura_fs_tools::cancel_resource_wait(cancellation.request_id);
        }
        outcome.disposition()
    };
    send_recorded_response(
        state,
        queue,
        request,
        JsonRpcResponse::success(request.id, serde_json::to_value(disposition)?),
    )
    .await?;
    Ok(IpcRequestTerminal::Succeeded)
}
