//! How long a service that ended waits before it is started again: 1, 2, 4 … seconds, at most a
//! minute; a run of five minutes or more starts the count over.
use std::time::Duration;

pub const FIRST_DELAY: Duration = Duration::from_secs(1);
pub const MAX_DELAY: Duration = Duration::from_secs(60);
pub const STABLE_AFTER: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Default)]
pub struct Backoff {
    failures: u32,
}

impl Backoff {
    /// The service ended after running for `ran`: how long to wait before starting it again.
    pub fn ended(&mut self, ran: Duration) -> Duration {
        if ran >= STABLE_AFTER {
            self.failures = 0;
        }
        let delay = FIRST_DELAY.saturating_mul(1_u32 << self.failures.min(6)).min(MAX_DELAY);
        self.failures = self.failures.saturating_add(1);
        delay
    }

    /// Started again on purpose (its content or settings changed): the count starts over.
    pub fn reset(&mut self) {
        self.failures = 0;
    }
}
