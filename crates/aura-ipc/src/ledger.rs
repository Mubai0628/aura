use std::collections::BTreeMap;

use aura_domain_common::CorrelationId;
use aura_runtime_contract::{CancelDisposition, IpcRequestTerminal};
use thiserror::Error;

/// Where a cancellation request found its target.
///
/// The wire answer is the three-valued [`CancelDisposition`], which conflates
/// "still waiting for a lane" and "already running" in `Accepted` on purpose:
/// both mean the intent was received, and neither promises the work will stop.
/// These four states are what this ledger records per request and what Core
/// traces, so an accepted cancellation is never mistaken for a stopped one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationOutcome {
    /// Admitted, but its handler never started: the request is still waiting for a
    /// lane.
    QueuedNotStarted,
    /// The handler is running right now.
    Executing,
    /// The request already reached its terminal; there is nothing left to cancel.
    AlreadyCompleted,
    /// This ledger holds no record of the request, so nothing is known about it.
    /// Absence of a record is not evidence that nothing ran.
    Unknown,
}

impl CancellationOutcome {
    /// The wire answer for this outcome. `Accepted` acknowledges the intent only.
    #[must_use]
    pub fn disposition(self) -> CancelDisposition {
        match self {
            Self::QueuedNotStarted | Self::Executing => CancelDisposition::Accepted,
            Self::AlreadyCompleted => CancelDisposition::TooLateToCancel,
            Self::Unknown => CancelDisposition::UnknownRequest,
        }
    }
}

/// What a received cancellation intent achieved, decided when its request reached
/// a terminal.
///
/// This is deliberately not [`CancellationOutcome`]: that one says where the
/// request was when the intent arrived, while this one says what its handler did
/// about it. `CancelDisposition::Accepted` is neither — a handler that was already
/// running can answer normally and never look at the intent at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationSettlement {
    /// No cancellation intent was ever received for this request.
    NotRequested,
    /// An intent was received and the handler answered `Cancelled`.
    Honoured,
    /// An intent was received and the handler reached its own terminal anyway: the
    /// cancellation was acknowledged, not enforced.
    NotHonoured,
    /// An intent was received while the request was still waiting for a lane and it
    /// never started, so there was nothing to stop.
    NeverStarted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LedgerState {
    /// Accepted and waiting for a lane: the handler has not started.
    Queued { cancellation_requested: bool },
    /// The handler really began running ([`RequestLedger::mark_started`]).
    Executing { cancellation_requested: bool },
    /// The one terminal, with what the cancellation intent (if any) achieved.
    Terminal { terminal: IpcRequestTerminal, cancellation: CancellationSettlement },
}

impl LedgerState {
    fn cancellation_requested(self) -> bool {
        match self {
            Self::Queued { cancellation_requested }
            | Self::Executing { cancellation_requested } => cancellation_requested,
            Self::Terminal { .. } => false,
        }
    }

    /// What an intent that is already recorded achieved at this terminal.
    fn settlement(self, terminal: IpcRequestTerminal) -> CancellationSettlement {
        if !self.cancellation_requested() {
            return CancellationSettlement::NotRequested;
        }
        match (self, terminal) {
            // The handler never ran, so the intent stopped work that had not
            // started instead of being ignored by it.
            (Self::Queued { .. }, _) => CancellationSettlement::NeverStarted,
            (Self::Executing { .. }, IpcRequestTerminal::Cancelled) => {
                CancellationSettlement::Honoured
            }
            // Running work that reached its own terminal anyway.
            _ => CancellationSettlement::NotHonoured,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LedgerError {
    #[error("the request already exists in the ledger")]
    DuplicateRequest,
    #[error("the request does not exist in the ledger")]
    UnknownRequest,
    #[error("the request already entered a terminal state")]
    AlreadyTerminal,
}

#[derive(Default)]
pub struct RequestLedger {
    entries: BTreeMap<CorrelationId, LedgerState>,
}

impl RequestLedger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin(&mut self, request_id: CorrelationId) -> Result<(), LedgerError> {
        if self.entries.contains_key(&request_id) {
            return Err(LedgerError::DuplicateRequest);
        }
        self.entries.insert(request_id, LedgerState::Queued { cancellation_requested: false });
        Ok(())
    }

    /// Record that the handler for `request_id` really began running.
    ///
    /// This is the transition that separates "waiting for a lane" from
    /// "executing": without it a cancellation cannot tell the two apart, and an
    /// accepted intent would be recorded against the wrong state.
    pub fn mark_started(&mut self, request_id: CorrelationId) -> Result<(), LedgerError> {
        match self.entries.get(&request_id).copied() {
            Some(LedgerState::Queued { cancellation_requested }) => {
                // An intent recorded while the request waited is still an intent.
                self.entries.insert(request_id, LedgerState::Executing { cancellation_requested });
                Ok(())
            }
            // A request starts once; a repeat names the same start.
            Some(LedgerState::Executing { .. }) => Ok(()),
            Some(LedgerState::Terminal { .. }) => Err(LedgerError::AlreadyTerminal),
            None => Err(LedgerError::UnknownRequest),
        }
    }

    /// Record a cancellation intent and report where the request was when it
    /// arrived.
    pub fn cancel(&mut self, request_id: CorrelationId) -> CancellationOutcome {
        match self.entries.get(&request_id).copied() {
            Some(LedgerState::Queued { .. }) => {
                self.entries
                    .insert(request_id, LedgerState::Queued { cancellation_requested: true });
                CancellationOutcome::QueuedNotStarted
            }
            Some(LedgerState::Executing { .. }) => {
                self.entries
                    .insert(request_id, LedgerState::Executing { cancellation_requested: true });
                CancellationOutcome::Executing
            }
            Some(LedgerState::Terminal { .. }) => CancellationOutcome::AlreadyCompleted,
            None => CancellationOutcome::Unknown,
        }
    }

    /// Whether a cancellation intent has been received for a request that has not
    /// reached its terminal yet. A finished request holds no live intent.
    #[must_use]
    pub fn cancellation_requested(&self, request_id: CorrelationId) -> bool {
        self.entries.get(&request_id).is_some_and(|state| state.cancellation_requested())
    }

    /// Whether the handler for this request really began running. False while the
    /// request waits for a lane, and false once it finished: started work is not
    /// running.
    #[must_use]
    pub fn is_executing(&self, request_id: CorrelationId) -> bool {
        matches!(self.entries.get(&request_id), Some(LedgerState::Executing { .. }))
    }

    /// Close the request's one terminal, and report what a cancellation intent
    /// received for it achieved.
    pub fn finish(
        &mut self,
        request_id: CorrelationId,
        terminal: IpcRequestTerminal,
    ) -> Result<CancellationSettlement, LedgerError> {
        let current = match self.entries.get(&request_id).copied() {
            Some(state) => state,
            None => return Err(LedgerError::UnknownRequest),
        };
        if matches!(current, LedgerState::Terminal { .. }) {
            return Err(LedgerError::AlreadyTerminal);
        }
        let cancellation = current.settlement(terminal);
        self.entries.insert(request_id, LedgerState::Terminal { terminal, cancellation });
        Ok(cancellation)
    }

    pub fn finish_all_running(&mut self, terminal: IpcRequestTerminal) -> usize {
        let mut count = 0;
        for state in self.entries.values_mut() {
            if matches!(state, LedgerState::Terminal { .. }) {
                continue;
            }
            let cancellation = (*state).settlement(terminal);
            *state = LedgerState::Terminal { terminal, cancellation };
            count += 1;
        }
        count
    }

    #[must_use]
    pub fn terminal(&self, request_id: CorrelationId) -> Option<IpcRequestTerminal> {
        match self.entries.get(&request_id) {
            Some(LedgerState::Terminal { terminal, .. }) => Some(*terminal),
            Some(LedgerState::Queued { .. } | LedgerState::Executing { .. }) | None => None,
        }
    }

    /// What the cancellation intent received for this request achieved, once the
    /// request reached its terminal. `None` while it is still queued or running.
    #[must_use]
    pub fn cancellation_settlement(
        &self,
        request_id: CorrelationId,
    ) -> Option<CancellationSettlement> {
        match self.entries.get(&request_id) {
            Some(LedgerState::Terminal { cancellation, .. }) => Some(*cancellation),
            Some(LedgerState::Queued { .. } | LedgerState::Executing { .. }) | None => None,
        }
    }
}
