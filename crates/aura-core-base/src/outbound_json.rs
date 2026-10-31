//! A JSON response or notification onto Core's outbound priority queue, traced by request id.
use anyhow::Result;
use aura_ipc::{
    AsyncPriorityQueue, JSON_RPC_VERSION, JsonRpcNotification, OutboundFrame, QueuePriority,
    WireFrame,
};
use serde::Serialize;

pub async fn send_notification<T: Serialize>(
    queue: &AsyncPriorityQueue,
    priority: QueuePriority,
    method_name: &str,
    value: &T,
) -> Result<()> {
    let notification = JsonRpcNotification {
        jsonrpc: JSON_RPC_VERSION.to_owned(),
        method: method_name.to_owned(),
        params: serde_json::to_value(value)?,
    };
    send_json(queue, priority, &notification).await
}

pub async fn send_json<T: Serialize>(
    queue: &AsyncPriorityQueue,
    priority: QueuePriority,
    value: &T,
) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    let bytes = payload.len();
    let id = trace_request_id(&payload);
    let kind = if id == "none" { "notification" } else { "response" };
    aura_ipc::trace(
        "core.enqueue",
        &format!("kind={kind} priority={priority:?} bytes={bytes} id={id}"),
    );
    queue.send(OutboundFrame { priority, frame: WireFrame::Control(payload) }).await?;
    aura_ipc::trace("core.enqueued", &format!("kind={kind} id={id}"));
    Ok(())
}

/// Best-effort request id for tracing. Reads only the `id` field.
pub fn trace_request_id(payload: &[u8]) -> String {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(payload) else {
        return "unknown".to_owned();
    };
    value.get("id").and_then(|id| id.as_str()).unwrap_or("none").to_owned()
}
