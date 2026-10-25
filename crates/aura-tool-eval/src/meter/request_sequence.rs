use super::MeterPurpose;
use crate::budget::Ledger;
use anyhow::Result;
use std::sync::Mutex;
use zeroize::Zeroizing;

/// One meter belongs to one task. Keep only the preceding reserved body in memory;
/// exact comparison avoids treating similar but changed turns as transparent retries.
#[derive(Default)]
pub(super) struct RequestSequence {
    previous: Mutex<Option<Zeroizing<Vec<u8>>>>,
}

impl RequestSequence {
    pub(super) fn reserve(
        &self,
        ledger: &Ledger,
        task: &str,
        purpose: MeterPurpose,
        body: &[u8],
        input: u64,
        output: u64,
    ) -> Result<i64> {
        if matches!(purpose, MeterPurpose::Probe) {
            return ledger.reserve(task, input, output, "probe");
        }
        let mut previous = self
            .previous
            .lock()
            .map_err(|_| anyhow::anyhow!("eval.request_sequence_lock_failed"))?;
        let kind = if previous.as_ref().is_some_and(|value| value.as_slice() == body) {
            "retry"
        } else {
            "generation"
        };
        let id = ledger.reserve(task, input, output, kind)?;
        *previous = Some(Zeroizing::new(body.to_vec()));
        Ok(id)
    }
}
