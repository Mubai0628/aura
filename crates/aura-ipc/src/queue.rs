use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use thiserror::Error;
use tokio::sync::Notify;

use crate::{
    AUTHENTICATED_FRAME_HEADER_SIZE, FRAME_LENGTH_PREFIX_SIZE, MAX_CONTROL_PAYLOAD_BYTES, WireFrame,
};

const MAX_TOTAL_FRAMES: usize = 64;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const P0_RESERVED_FRAMES: usize = 8;
const P0_RESERVED_BYTES: usize = 32 * 1024;
const SHARED_MAX_FRAMES: usize = MAX_TOTAL_FRAMES - P0_RESERVED_FRAMES;
const SHARED_MAX_BYTES: usize = MAX_TOTAL_BYTES - P0_RESERVED_BYTES;
const P1_RESERVED_FRAMES: usize = 8;
const P1_RESERVED_BYTES: usize =
    MAX_CONTROL_PAYLOAD_BYTES + FRAME_LENGTH_PREFIX_SIZE + 1 + AUTHENTICATED_FRAME_HEADER_SIZE;
const BULK_MAX_FRAMES: usize = SHARED_MAX_FRAMES - P1_RESERVED_FRAMES;
const BULK_MAX_BYTES: usize = SHARED_MAX_BYTES - P1_RESERVED_BYTES;
const P0_MAX_FRAME_BYTES: usize = 32 * 1024;
/// Answers the connection reader writes itself ([`AsyncPriorityQueue::try_send`] at P0) that find
/// the reservation full wait here, right behind it, so a burst of refused requests is answered
/// instead of ending the connection. Main keeps at most 848 requests per channel waiting for an
/// answer (768 ordinary, 80 control), so such a burst always fits; the bound still stops a peer that
/// never reads from growing it.
const P0_OVERFLOW_FRAMES: usize = 1024;
const P0_OVERFLOW_BYTES: usize = 1024 * 1024;
const SHARED_SCHEDULE: [QueuePriority; 7] = [
    QueuePriority::P1,
    QueuePriority::P1,
    QueuePriority::P1,
    QueuePriority::P1,
    QueuePriority::P2,
    QueuePriority::P2,
    QueuePriority::P3,
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QueuePriority {
    P0,
    P1,
    P2,
    P3,
}

#[derive(Debug, Eq, PartialEq)]
pub struct OutboundFrame {
    pub priority: QueuePriority,
    pub frame: WireFrame,
}

impl OutboundFrame {
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        self.frame.encoded_len() + AUTHENTICATED_FRAME_HEADER_SIZE
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum QueuePushError {
    #[error("the IPC queue is closed")]
    Closed,
    #[error("the selected IPC priority partition is full")]
    Full,
    #[error("a P0 frame exceeds its 32 KiB emergency partition limit")]
    CriticalFrameTooLarge,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("the IPC queue is closed")]
pub struct QueueClosed;

#[derive(Default)]
struct QueueState {
    p0: VecDeque<OutboundFrame>,
    p0_overflow: VecDeque<OutboundFrame>,
    p1: VecDeque<OutboundFrame>,
    p2: VecDeque<OutboundFrame>,
    p3: VecDeque<OutboundFrame>,
    p0_bytes: usize,
    p0_overflow_bytes: usize,
    shared_bytes: usize,
    shared_cursor: usize,
}

impl QueueState {
    fn shared_frames(&self) -> usize {
        self.p1.len() + self.p2.len() + self.p3.len()
    }

    fn bulk_frames(&self) -> usize {
        self.p2.len() + self.p3.len()
    }

    fn bulk_bytes(&self) -> usize {
        self.p2.iter().chain(self.p3.iter()).map(OutboundFrame::encoded_len).sum()
    }

    /// Frames still queued, for tracing only.
    fn depth(&self) -> usize {
        self.p0.len() + self.p0_overflow.len() + self.shared_frames()
    }

    fn validate_push(&self, outbound: &OutboundFrame) -> Result<(), QueuePushError> {
        let bytes = outbound.encoded_len();
        match outbound.priority {
            QueuePriority::P0 => {
                if bytes > P0_MAX_FRAME_BYTES {
                    return Err(QueuePushError::CriticalFrameTooLarge);
                }
                if self.p0.len() >= P0_RESERVED_FRAMES
                    || self.p0_bytes.saturating_add(bytes) > P0_RESERVED_BYTES
                {
                    return Err(QueuePushError::Full);
                }
            }
            QueuePriority::P1 => {
                if self.shared_frames() >= SHARED_MAX_FRAMES
                    || self.shared_bytes.saturating_add(bytes) > SHARED_MAX_BYTES
                {
                    return Err(QueuePushError::Full);
                }
            }
            QueuePriority::P2 | QueuePriority::P3 => {
                if self.bulk_frames() >= BULK_MAX_FRAMES
                    || self.bulk_bytes().saturating_add(bytes) > BULK_MAX_BYTES
                {
                    return Err(QueuePushError::Full);
                }
            }
        }
        Ok(())
    }

    fn push(&mut self, outbound: OutboundFrame) {
        let bytes = outbound.encoded_len();
        match outbound.priority {
            QueuePriority::P0 => {
                self.p0_bytes += bytes;
                self.p0.push_back(outbound);
            }
            QueuePriority::P1 => {
                self.shared_bytes += bytes;
                self.p1.push_back(outbound);
            }
            QueuePriority::P2 => {
                self.shared_bytes += bytes;
                self.p2.push_back(outbound);
            }
            QueuePriority::P3 => {
                self.shared_bytes += bytes;
                self.p3.push_back(outbound);
            }
        }
    }

    /// [`AsyncPriorityQueue::try_send`]'s push. A P0 answer that finds the reservation full, or
    /// earlier answers already waiting behind it, joins the overflow, so the reader's answers keep
    /// their order and control frames sent with [`AsyncPriorityQueue::send`] keep the reservation.
    fn push_answer(&mut self, outbound: OutboundFrame) -> Result<(), QueuePushError> {
        let fits = self.validate_push(&outbound);
        if outbound.priority != QueuePriority::P0 {
            fits?;
            self.push(outbound);
            return Ok(());
        }
        match fits {
            Ok(()) if self.p0_overflow.is_empty() => self.push(outbound),
            Ok(()) | Err(QueuePushError::Full) => {
                let bytes = outbound.encoded_len();
                if self.p0_overflow.len() >= P0_OVERFLOW_FRAMES
                    || self.p0_overflow_bytes.saturating_add(bytes) > P0_OVERFLOW_BYTES
                {
                    return Err(QueuePushError::Full);
                }
                self.p0_overflow_bytes += bytes;
                self.p0_overflow.push_back(outbound);
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }

    fn pop(&mut self) -> Option<OutboundFrame> {
        if let Some(frame) = self.p0.pop_front() {
            self.p0_bytes = self.p0_bytes.saturating_sub(frame.encoded_len());
            return Some(frame);
        }
        if let Some(frame) = self.p0_overflow.pop_front() {
            self.p0_overflow_bytes = self.p0_overflow_bytes.saturating_sub(frame.encoded_len());
            return Some(frame);
        }

        for _ in 0..SHARED_SCHEDULE.len() {
            let priority = SHARED_SCHEDULE[self.shared_cursor];
            self.shared_cursor = (self.shared_cursor + 1) % SHARED_SCHEDULE.len();
            let frame = match priority {
                QueuePriority::P1 => self.p1.pop_front(),
                QueuePriority::P2 => self.p2.pop_front(),
                QueuePriority::P3 => self.p3.pop_front(),
                QueuePriority::P0 => None,
            };
            if let Some(frame) = frame {
                self.shared_bytes = self.shared_bytes.saturating_sub(frame.encoded_len());
                return Some(frame);
            }
        }
        None
    }
}

struct SharedQueue {
    /// Held only to push or pop one frame, never across an await, so a contended lock is a wait of
    /// microseconds rather than a reason to report the queue full.
    state: Mutex<QueueState>,
    item_available: Notify,
    space_available: Notify,
    connection_closed: Notify,
    closed: AtomicBool,
}

impl SharedQueue {
    /// The critical sections cannot leave the state half changed, so a poisoned lock is still usable.
    fn lock(&self) -> MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Clone)]
pub struct AsyncPriorityQueue {
    shared: Arc<SharedQueue>,
}

impl Default for AsyncPriorityQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncPriorityQueue {
    #[must_use]
    pub fn new() -> Self {
        Self {
            shared: Arc::new(SharedQueue {
                state: Mutex::new(QueueState::default()),
                item_available: Notify::new(),
                space_available: Notify::new(),
                connection_closed: Notify::new(),
                closed: AtomicBool::new(false),
            }),
        }
    }

    pub async fn send(&self, outbound: OutboundFrame) -> Result<(), QueuePushError> {
        loop {
            let notified = self.shared.space_available.notified();
            if self.shared.closed.load(Ordering::Acquire) {
                return Err(QueuePushError::Closed);
            }
            {
                let mut state = self.shared.lock();
                match state.validate_push(&outbound) {
                    Ok(()) => {
                        state.push(outbound);
                        drop(state);
                        self.shared.item_available.notify_one();
                        return Ok(());
                    }
                    Err(QueuePushError::Full) => {}
                    Err(error) => return Err(error),
                }
            }
            notified.await;
        }
    }

    /// Answer without waiting for space.
    ///
    /// [`Self::send`] parks until the target partition has room, which is fine
    /// for producer tasks. The connection reader cannot afford that: it owes the
    /// peer an immediate answer for a refused request, and waiting would stop it
    /// from serving cancellation and connection control. A P0 answer that finds
    /// the reservation full waits in the overflow behind it
    /// ([`P0_OVERFLOW_FRAMES`]); only when that is full too, or for another
    /// priority whose partition is full, is [`QueuePushError::Full`] reported and
    /// the caller decides.
    pub fn try_send(&self, outbound: OutboundFrame) -> Result<(), QueuePushError> {
        if self.shared.closed.load(Ordering::Acquire) {
            return Err(QueuePushError::Closed);
        }
        let mut state = self.shared.lock();
        state.push_answer(outbound)?;
        drop(state);
        self.shared.item_available.notify_one();
        Ok(())
    }

    pub async fn recv(&self) -> Result<OutboundFrame, QueueClosed> {
        loop {
            let notified = self.shared.item_available.notified();
            {
                let mut state = self.shared.lock();
                if let Some(frame) = state.pop() {
                    let depth = state.depth();
                    drop(state);
                    self.shared.space_available.notify_waiters();
                    crate::trace::trace(
                        "queue.dequeue",
                        &format!("priority={:?} depth={depth}", frame.priority),
                    );
                    return Ok(frame);
                }
                if self.shared.closed.load(Ordering::Acquire) {
                    return Err(QueueClosed);
                }
            }
            notified.await;
        }
    }

    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.item_available.notify_waiters();
        self.shared.space_available.notify_waiters();
        self.shared.connection_closed.notify_waiters();
    }

    /// Waits for disconnection without consuming a frame or its receiver's notification.
    pub async fn closed(&self) {
        loop {
            let notified = self.shared.connection_closed.notified();
            if self.shared.closed.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod answer_overflow_tests;
#[cfg(test)]
mod media_connection_tests;
#[cfg(test)]
mod tests {
    use super::*;

    fn error_frame(size: usize) -> OutboundFrame {
        OutboundFrame { priority: QueuePriority::P0, frame: WireFrame::Control(vec![0_u8; size]) }
    }

    #[test]
    fn try_send_reports_a_full_partition_instead_of_waiting() {
        let queue = AsyncPriorityQueue::new();
        let mut sent = 0;
        // The reservation and the overflow behind it are both bounded: once
        // they are full the reader must be told "full" rather than park.
        loop {
            match queue.try_send(error_frame(64)) {
                Ok(()) => sent += 1,
                Err(QueuePushError::Full) => break,
                Err(error) => panic!("unexpected push error: {error}"),
            }
            assert!(sent <= P0_RESERVED_FRAMES + P0_OVERFLOW_FRAMES, "the bound must hold");
        }
        assert_eq!(sent, P0_RESERVED_FRAMES + P0_OVERFLOW_FRAMES, "every bounded place is usable");
    }

    #[tokio::test]
    async fn try_send_never_blocks_and_a_closed_queue_is_reported() {
        let queue = AsyncPriorityQueue::new();
        queue.try_send(error_frame(64)).expect("first frame fits");
        // Drain the single frame, then close: both paths stay synchronous.
        let _ = queue.recv().await.expect("frame");
        queue.close();
        assert_eq!(queue.try_send(error_frame(64)), Err(QueuePushError::Closed));
    }
}
