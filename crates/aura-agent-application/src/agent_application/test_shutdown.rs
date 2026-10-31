//! Awaitable teardown only for same-process integration fixtures.
use super::*;

impl AgentApplication {
    pub async fn quiesce_for_test(&self, deadline: Instant) {
        self.derived_backfill.stop_and_join_for_test(deadline).await;
        self.expiry_timers.stop_and_join_for_test(deadline).await;
        let outcome = self.stop_all_within(deadline).await;
        self.expiry_timers.stop_and_join_for_test(deadline).await;
        assert!(outcome.residue.is_empty(), "old Agent workers did not end: {:?}", outcome.residue);
    }
}
