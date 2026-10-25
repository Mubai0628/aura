//! Live control of a synchronous preparation pass.
//!
//! A grant pass runs synchronously and cannot be interrupted from outside, so
//! it polls its control between objects. The control carries the pass's own
//! deadline and the launch's live cancellation signal — never a snapshot of
//! that signal: a copy taken when the pass began cannot see a cancellation
//! raised while the walk is rewriting the tree, which is the defect this
//! module replaces.

/// A live cancellation signal a synchronous pass polls between objects.
///
/// Owned (`Arc`) and `Send + Sync`, so a pass running on a blocking thread
/// keeps reading the launch's real signal. The supervisor builds this from the
/// launch's watch receiver; recovery builds one that never fires.
pub struct GrantSignal(std::sync::Arc<dyn Fn() -> bool + Send + Sync>);

impl GrantSignal {
    #[must_use]
    pub fn new(signal: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(signal))
    }

    /// A signal that never fires: recovery and callers with no live launch.
    #[must_use]
    pub fn never() -> Self {
        Self::new(|| false)
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        (self.0)()
    }
}

impl Clone for GrantSignal {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::fmt::Debug for GrantSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GrantSignal(..)")
    }
}

/// What stops a synchronous preparation pass.
///
/// Both answers come from the caller, because only the caller knows what the launch's
/// cancellation signal and deadline are. The pass asks between objects, so a stop leaves
/// every object it touched either untouched or fully captured and applied.
pub struct GrantControl {
    deadline: std::time::Instant,
    signal: GrantSignal,
}

impl GrantControl {
    #[must_use]
    pub fn new(deadline: std::time::Instant, signal: GrantSignal) -> Self {
        Self { deadline, signal }
    }

    /// A control that stops at `budget`, or earlier if the launch is cancelled.
    #[must_use]
    pub fn with_budget(budget: std::time::Duration, signal: GrantSignal) -> Self {
        Self { deadline: std::time::Instant::now() + budget, signal }
    }

    /// A control that never stops: for callers with no budget, such as recovery.
    #[must_use]
    pub fn unbounded() -> Self {
        Self {
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(86_400),
            signal: GrantSignal::never(),
        }
    }

    /// Why the pass must stop now, if it must. Checked between objects, never
    /// in the middle of one.
    pub(crate) fn stop_reason(&self) -> Option<GrantStop> {
        if self.signal.is_cancelled() {
            return Some(GrantStop::Cancelled);
        }
        (std::time::Instant::now() >= self.deadline).then_some(GrantStop::BudgetExhausted)
    }
}

/// Why a bounded piece of preparation work stopped before it was finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum GrantStop {
    /// The launch was cancelled, revoked or downgraded.
    Cancelled,
    /// The preparation budget ran out.
    BudgetExhausted,
}

impl GrantStop {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::BudgetExhausted => "budget_exhausted",
        }
    }
}

/// How one bounded preparation pass ended.
///
/// A stop is not a failure and never a success: the pass really did the work
/// its counts report, and the caller must both keep those counts and refuse to
/// treat the grant as finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantVerdict {
    /// Every captured object was rewritten and nothing stopped the pass.
    Complete,
    /// The pass stopped at a control check with its partial work recorded.
    Stopped(GrantStop),
    /// The pass failed; objects captured before the failure stay retained and
    /// journaled for restoration.
    Failed,
}

/// What one grant pass did, and how it ended.
///
/// The counts are the point: a pass that died on object eight thousand of fourteen
/// thousand had captured, journaled and rewritten eight thousand objects, and a recovery
/// has to undo exactly those. Reporting zero because the pass ended badly would make a
/// launch that touched half the workspace look like one that touched nothing. So a pass
/// returns its counts whether it finished, stopped at a deadline, or failed — and the
/// verdict still says a stopped pass was not a finished grant.
pub struct GrantOutcome {
    pub work: crate::GrantWork,
    pub verdict: GrantVerdict,
    pub failure: Option<anyhow::Error>,
}

impl GrantOutcome {
    /// The work this pass did before it ended, whatever the ending was.
    #[must_use]
    pub fn work(&self) -> &crate::GrantWork {
        &self.work
    }

    /// Fails unless the pass completed: a stop is an error, never a silent success.
    pub fn finish(self) -> anyhow::Result<crate::GrantWork> {
        match self.verdict {
            GrantVerdict::Complete => Ok(self.work),
            GrantVerdict::Stopped(reason) => {
                Err(anyhow::anyhow!("runner.grant_stopped.{}", reason.code()))
            }
            GrantVerdict::Failed => {
                Err(self.failure.unwrap_or_else(|| anyhow::anyhow!("runner.grant_failed")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn a_live_signal_is_read_at_each_check() {
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let signal = GrantSignal::new(move || flag.load(Ordering::Acquire));
        assert!(!signal.is_cancelled());
        cancelled.store(true, Ordering::Release);
        assert!(signal.is_cancelled(), "the pass must see a cancel raised after it began");
    }

    #[test]
    fn stop_reason_prefers_cancel_over_budget() {
        let cancelled = std::sync::Arc::new(AtomicBool::new(true));
        let flag = cancelled.clone();
        let control = GrantControl::with_budget(
            std::time::Duration::from_secs(60),
            GrantSignal::new(move || flag.load(Ordering::Acquire)),
        );
        assert_eq!(control.stop_reason(), Some(GrantStop::Cancelled));
    }

    #[test]
    fn finish_rejects_stopped_and_failed_but_accepts_complete() {
        let complete = GrantOutcome {
            work: crate::GrantWork::default(),
            verdict: GrantVerdict::Complete,
            failure: None,
        };
        assert!(complete.finish().is_ok());
        let stopped = GrantOutcome {
            work: crate::GrantWork::default(),
            verdict: GrantVerdict::Stopped(GrantStop::Cancelled),
            failure: None,
        };
        let error = stopped.finish().expect_err("a stopped pass must not finish as success");
        assert_eq!(format!("{error:#}"), "runner.grant_stopped.cancelled");
        let failed = GrantOutcome {
            work: crate::GrantWork::default(),
            verdict: GrantVerdict::Failed,
            failure: Some(anyhow::anyhow!("runner.scope_escape")),
        };
        assert!(failed.finish().is_err());
    }
}
