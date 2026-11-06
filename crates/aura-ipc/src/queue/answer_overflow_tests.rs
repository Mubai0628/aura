//! Answers Core's reader writes itself (`try_send`) during a burst of refused requests.
use super::*;
use std::time::Duration;

/// One refusal about the size Core's reader writes for an overloaded request.
fn refusal(index: usize) -> OutboundFrame {
    let payload = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":\"cor_{index:026}\",\"error\":{{\"code\":-32000,\
         \"message\":\"handler overloaded\",\"data\":{{\"code\":\"ipc.handler_overloaded\",\
         \"category\":\"unavailable\",\"retryable\":true,\"terminal\":\"failed\",\
         \"details\":{{\"lane\":\"concurrent\",\"padding\":\"{}\"}}}}}}}}",
        "x".repeat(320)
    );
    OutboundFrame { priority: QueuePriority::P0, frame: WireFrame::Control(payload.into_bytes()) }
}

#[tokio::test]
async fn a_refused_burst_outlasts_the_reservation_while_the_writer_is_stalled() {
    // Nothing receives: the writer is stuck behind a pipe Main has not read yet. A refusal that
    // finds the reservation full must still be queued; failing here retires the whole connection.
    let queue = AsyncPriorityQueue::new();
    let burst = 192;
    for index in 0..burst {
        assert_eq!(queue.try_send(refusal(index)), Ok(()), "refusal {index} must be queued");
    }
    // Once the writer moves again, every refusal leaves in the order the reader wrote it.
    for index in 0..burst {
        let frame = tokio::time::timeout(Duration::from_millis(100), queue.recv())
            .await
            .expect("a queued refusal is delivered")
            .expect("the queue is open");
        assert_eq!(frame, refusal(index), "refusal {index} keeps its place");
    }
}

#[tokio::test]
async fn a_control_frame_keeps_its_reservation_during_a_refused_burst() {
    let queue = AsyncPriorityQueue::new();
    let burst = P0_RESERVED_FRAMES + 16;
    for index in 0..burst {
        queue.try_send(refusal(index)).expect("the refusal is queued");
    }
    // The writer sends one frame, so the reservation has room for one control frame again.
    assert_eq!(queue.recv().await.expect("frame"), refusal(0));
    let heartbeat = || OutboundFrame {
        priority: QueuePriority::P0,
        frame: WireFrame::Control(b"pulse".to_vec()),
    };
    tokio::time::timeout(Duration::from_millis(100), queue.send(heartbeat()))
        .await
        .expect("a control frame does not wait for the refusals behind it")
        .expect("the control frame is queued");
    let mut order = Vec::new();
    for _ in 0..burst {
        order.push(queue.recv().await.expect("frame"));
    }
    let pulse =
        order.iter().position(|frame| *frame == heartbeat()).expect("the control frame left");
    assert!(pulse < P0_RESERVED_FRAMES, "the control frame leaves with the reservation: {pulse}");
    let refusals: Vec<_> = order.into_iter().filter(|frame| *frame != heartbeat()).collect();
    assert_eq!(refusals, (1..burst).map(refusal).collect::<Vec<_>>(), "refusals keep their order");
}

/// Holds the queue's state lock on another thread for `hold`, the way a handler pushing its reply
/// or the writer taking the next frame holds it for a moment.
fn hold_state(queue: &AsyncPriorityQueue, hold: Duration) -> std::thread::JoinHandle<()> {
    let shared = Arc::clone(&queue.shared);
    let (held, holding) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _state = shared.lock();
        held.send(()).expect("report the lock is held");
        std::thread::sleep(hold);
    });
    holding.recv().expect("the lock is held");
    holder
}

#[test]
fn a_reader_answer_is_not_refused_because_another_task_holds_the_lock() {
    let queue = AsyncPriorityQueue::new();
    // The partition is empty; the only obstacle is a lock held for a moment.
    let holder = hold_state(&queue, Duration::from_millis(50));
    assert_eq!(queue.try_send(refusal(0)), Ok(()), "a held lock is not a full partition");
    holder.join().expect("the holder ends");
}

#[test]
fn the_overflow_is_bounded_by_bytes_as_well_as_frames() {
    // P0 frames may be up to 32 KiB, so a frame count alone would not bound the memory.
    let queue = AsyncPriorityQueue::new();
    let largest =
        P0_MAX_FRAME_BYTES - FRAME_LENGTH_PREFIX_SIZE - 1 - AUTHENTICATED_FRAME_HEADER_SIZE;
    let frame = || OutboundFrame {
        priority: QueuePriority::P0,
        frame: WireFrame::Control(vec![b'x'; largest]),
    };
    assert_eq!(frame().encoded_len(), P0_MAX_FRAME_BYTES, "the largest P0 frame");
    let mut sent = 0;
    while queue.try_send(frame()).is_ok() {
        sent += 1;
    }
    let reserved = P0_RESERVED_BYTES / P0_MAX_FRAME_BYTES;
    assert_eq!(sent, reserved + P0_OVERFLOW_BYTES / P0_MAX_FRAME_BYTES, "bytes bound both places");
    assert_eq!(queue.try_send(refusal(0)), Err(QueuePushError::Full), "and nothing more fits");
}

#[tokio::test]
async fn a_producer_still_waits_for_the_reservation_itself() {
    // Only the reader's own answers use the overflow; a task that can wait keeps waiting.
    let queue = AsyncPriorityQueue::new();
    for index in 0..P0_RESERVED_FRAMES {
        queue.try_send(refusal(index)).expect("the reservation has room");
    }
    let parked = tokio::time::timeout(Duration::from_millis(50), queue.send(refusal(99))).await;
    assert!(parked.is_err(), "a full reservation parks a producer");
}
