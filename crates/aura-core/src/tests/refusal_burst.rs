use super::*;
use aura_core_base::ipc_session::{Admission, Lane};
use aura_ipc::{AsyncPriorityQueue, WireFrame};

#[tokio::test]
async fn a_refused_burst_is_answered_while_the_writer_is_stalled() {
    // The writer is stuck behind a pipe Main has not read yet, so nothing leaves the queue while the
    // reader refuses a burst of reads - far more than the eight P0 places reserved for connection
    // control. Each refusal must still be queued: an error here would retire the whole connection.
    let queue = AsyncPriorityQueue::new();
    let refused: Vec<CorrelationId> = (0..192).map(|_| CorrelationId::new()).collect();
    for id in &refused {
        let error = Admission::overloaded_error(*id, Lane::Concurrent);
        crate::send_control_answer(&queue, &JsonRpcResponse::failure(*id, error))
            .expect("a refusal is answered, not turned into a connection failure");
    }
    // When the writer moves again, every refused request gets its own typed overload answer.
    for id in &refused {
        let frame = queue.recv().await.expect("the refusal is delivered");
        let WireFrame::Control(payload) = frame.frame else {
            panic!("a refusal is a control frame")
        };
        let answer: serde_json::Value = serde_json::from_slice(&payload).expect("a JSON answer");
        assert_eq!(answer["id"], serde_json::to_value(id).expect("id"));
        assert_eq!(answer["error"]["data"]["code"], "ipc.handler_overloaded");
    }
}
