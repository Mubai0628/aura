use super::{CompatExecution, monitor::Record};
use anyhow::Result;
use aura_domain_common::UtcTimestamp;
use aura_windows_user_contract::{BackgroundState, RunnerBackgroundUpdate};
use std::time::Duration;
use tokio::sync::watch;

pub(super) fn follow(
    record: &Record,
    execution: &mut CompatExecution,
    mut stdout: usize,
    mut stderr: usize,
    cancelled: &watch::Receiver<bool>,
) -> Result<()> {
    let mut sequence = 0;
    loop {
        let (mut out, mut err, truncated) = execution.output_since(stdout, stderr)?;
        let interrupted = *cancelled.borrow() || truncated;
        let active = execution.active_processes();
        let state = if interrupted || active.is_err() {
            if execution.stop(Duration::from_secs(3)).confirmed {
                BackgroundState::Interrupted
            } else {
                BackgroundState::Unknown
            }
        } else if matches!(active, Ok(0)) {
            BackgroundState::Exited
        } else {
            BackgroundState::Running
        };
        if state != BackgroundState::Running {
            // Nothing can write any more: what was held as the possible start of a hidden value
            // is released with the last update.
            execution.finish_output();
            let (rest_out, rest_err, _) =
                execution.output_since(stdout + out.len(), stderr + err.len())?;
            out.extend(rest_out);
            err.extend(rest_err);
        }
        stdout += out.len();
        stderr += err.len();
        if sequence == 0 || state != BackgroundState::Running || !out.is_empty() || !err.is_empty()
        {
            sequence += 1;
            let update = RunnerBackgroundUpdate {
                attempt: record.request.binding.attempt.clone(),
                sequence,
                observed_at: UtcTimestamp::now(),
                state,
                stdout: out,
                stderr: err,
                output_truncated: truncated,
            };
            record.write("compat.background", move |port, request| {
                port.background(&request, &update)
            })?;
        }
        if state != BackgroundState::Running {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
