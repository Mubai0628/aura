//! Each UI Automation request runs on a thread of its own, so that a program that stops
//! answering holds up only that thread and the host still answers in time.
use crate::native;
use aura_desktop_domain::{Delivery, HostError, HostErrorCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// Requests may be left waiting on programs that stopped answering; once this many are, new
/// ones are refused at once instead of leaving more threads behind.
pub const MAX_LEFT_WAITING: usize = 4;
/// Requests whose caller stopped waiting while their thread still runs. Requests that are being
/// waited for do not count: several of them may run at once in one process.
static LEFT_WAITING: AtomicUsize = AtomicUsize::new(0);

/// A request's thread runs while its caller waits for it.
const WAITED: u8 = 0;
/// The caller stopped waiting; the thread is counted in `LEFT_WAITING` until it ends.
const LEFT: u8 = 1;
const ENDED: u8 = 2;

/// Marks the request's end when its thread ends, also by a panic, and stops counting it as left
/// waiting.
struct Ends(Arc<AtomicU8>);

impl Drop for Ends {
    fn drop(&mut self) {
        if self.0.swap(ENDED, Ordering::SeqCst) == LEFT {
            LEFT_WAITING.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

pub enum Ran<T> {
    Done(Result<T, HostError>),
    /// `work` did not finish in time; it goes on by itself and its answer is dropped.
    TimedOut,
}

/// Runs `work` on a new thread in COM and waits for it up to `timeout`.
pub fn on_own_thread<T: Send + 'static>(
    timeout: Duration,
    work: impl FnOnce() -> Result<T, HostError> + Send + 'static,
) -> Ran<T> {
    if LEFT_WAITING.load(Ordering::SeqCst) >= MAX_LEFT_WAITING {
        return Ran::Done(Err(HostError::new(
            HostErrorCode::Timeout,
            "前几次请求还在等没有响应的程序，等它恢复了再试",
            Delivery::NotDelivered,
        )));
    }
    let (sender, receiver) = mpsc::channel();
    let state = Arc::new(AtomicU8::new(WAITED));
    let ends = Ends(Arc::clone(&state));
    let spawned = std::thread::Builder::new().name("aura-desktop-uia".into()).spawn(move || {
        let _ends = ends;
        let result = match native::Com::init() {
            Ok(_com) => work(),
            Err(error) => Err(HostError::new(
                HostErrorCode::Internal,
                format!("COM 准备不了：{error}"),
                Delivery::NotDelivered,
            )),
        };
        let _ = sender.send(result);
    });
    if let Err(error) = spawned {
        return Ran::Done(Err(HostError::new(
            HostErrorCode::Internal,
            format!("开不了读取的线程：{error}"),
            Delivery::NotDelivered,
        )));
    }
    match receiver.recv_timeout(timeout) {
        Ok(result) => Ran::Done(result),
        Err(_) => {
            // Counted before it is marked left, so that the thread's end never takes away a
            // count that is not there yet.
            LEFT_WAITING.fetch_add(1, Ordering::SeqCst);
            if state.compare_exchange(WAITED, LEFT, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return Ran::TimedOut;
            }
            // It ended as the wait ran out; its answer, if it sent one, is here.
            LEFT_WAITING.fetch_sub(1, Ordering::SeqCst);
            match receiver.try_recv() {
                Ok(result) => Ran::Done(result),
                Err(_) => Ran::TimedOut,
            }
        }
    }
}
