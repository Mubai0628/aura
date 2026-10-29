//! What the writer of Main's connection is doing, for the heartbeat's diagnosis. A heartbeat
//! leaves after the frame being written: a large one Main has not read yet holds it back, and the
//! pulse can then say so.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
/// When the frame being written started, in milliseconds after `START` plus one; 0 when none is.
static WRITING_SINCE: AtomicU64 = AtomicU64::new(0);
static WRITING_BYTES: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    u64::try_from(START.get_or_init(Instant::now).elapsed().as_millis())
        .unwrap_or(u64::MAX - 1)
        .saturating_add(1)
}

pub(crate) fn writing(epoch: u64, sequence: u64, bytes: usize) {
    aura_ipc::trace("core.write.start", &format!("epoch={epoch} seq={sequence}"));
    WRITING_BYTES.store(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
    WRITING_SINCE.store(now_ms(), Ordering::Release);
}

pub(crate) fn written(epoch: u64, sequence: u64) {
    WRITING_SINCE.store(0, Ordering::Release);
    aura_ipc::trace("core.write.done", &format!("epoch={epoch} seq={sequence}"));
}

/// The frame being written to Main now, if any: for how many milliseconds, and how many bytes.
pub(crate) fn current() -> Option<(u64, u64)> {
    let since = WRITING_SINCE.load(Ordering::Acquire);
    (since != 0).then(|| (now_ms().saturating_sub(since), WRITING_BYTES.load(Ordering::Relaxed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_is_current_from_its_start_to_its_end() {
        written(1, 1);
        assert_eq!(current(), None);
        writing(1, 2, 4_096);
        let (_, bytes) = current().expect("a write in progress");
        assert_eq!(bytes, 4_096);
        written(1, 2);
        assert_eq!(current(), None);
    }
}
