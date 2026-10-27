//! Transport adapter for the V2 Model Store. Raw requests/responses remain
//! private to their real ModelStep; model-visible history is published separately.
//!
//! Boundary
//! Every method of [`TransportStore`] really writes to the V2 journal. These used to be
//! synchronous: the transport loop called them from its own async task, so the reactor worker
//! that also has to deliver the Core heartbeat was parked on `journal.lock()` for as long as the
//! SQLite work took. The trait is now awaitable and this adapter owns an [`AgentStore`], so each
//! call is one admitted section on the one shared bounded boundary:
//!
//! * **admission** - the same [`crate::sync_boundary::SYNC_WORKERS`] permit pool every other Agent
//!   step uses. No second pool and no unbounded `spawn_blocking`.
//! * **deadline** - the boundary's own section budget ([`crate::sync_boundary::SYNC_SECTION_BUDGET`]).
//! * **receipt** - a write returns one and it stays queryable on the store after the caller's own
//!   wait ended.
//! * **timeout classification** - [`StoreError::Refused`] means the section provably did not run;
//!   [`StoreError::Unknown`] means it started and may still commit. They are carried to the
//!   transport as different typed codes and are never mapped onto each other.
//!
//! `RawStreamWriter::append` is deliberately left synchronous: it only reserves and extends a
//! bounded in-memory `Vec` and touches no journal, so turning it into a storage task would be a
//! second, pointless database task for a pure memory operation.
use aura_domain_common::*;
use aura_model_domain::*;
use aura_model_transport::*;
use aura_persistence_v2::model::agent_transport as store;
use std::sync::Arc;

use crate::agent_store::{AgentStore, ReceiptState, StoreError, StoreStep};

/// How long a record of the step that outlived its caller's wait is followed to its real end.
const SETTLE_WITHIN: std::time::Duration = std::time::Duration::from_secs(300);

#[derive(Clone)]
pub(super) struct AgentModelStore {
    pub store: Arc<AgentStore>,
    /// The run this attempt belongs to, so a hold point or a receipt can be matched by identity.
    pub run: Option<AgentRunId>,
    pub snapshot: ModelExecutionSnapshot,
    /// An answer that would end the turn waits for `turn.before_complete` Hooks.
    pub hold_completion: bool,
}

/// One raw provider stream, held in memory until it is really persisted.
///
/// Nothing here takes a lock or runs SQL, so `append` stays synchronous; only `finalize` reaches
/// the journal, and it does so through the bounded owner.
pub(super) struct ModelStreamWriter {
    owner: AgentModelStore,
    bytes: Vec<u8>,
}

/// Carries the typed store classification instead of flattening it into one message.
fn error(value: crate::agent_store::StoreError) -> ModelTransportError {
    ModelTransportError::Store(format!("{}: {value}", value.code()))
}

fn detail(value: impl std::fmt::Display) -> ModelTransportError {
    ModelTransportError::Store(value.to_string())
}

impl AgentModelStore {
    /// The real answer of a record of the step. One that outlived its caller's bounded wait (the
    /// store was busy) still commits under its own permit, so it is followed to its end instead
    /// of being taken for a failure: a step failed that way would leave the record it committed
    /// afterwards, with the run already past its end.
    async fn settled(&self, answer: Result<(), StoreError>) -> Result<(), ModelTransportError> {
        let Err(answer) = answer else {
            return Ok(());
        };
        let Some(receipt) = answer.receipt() else {
            return Err(error(answer));
        };
        let deadline = tokio::time::Instant::now() + SETTLE_WITHIN;
        loop {
            match self.store.receipt(receipt).map(|receipt| receipt.state) {
                Some(ReceiptState::Settled { committed: true, .. }) => return Ok(()),
                Some(ReceiptState::InFlight) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                _ => return Err(error(answer)),
            }
        }
    }

    fn step(&self, operation: &'static str) -> StoreStep {
        match self.run {
            Some(run) => StoreStep::run(run, operation),
            None => StoreStep::operation(operation),
        }
    }

    async fn put(&self, media: &str, bytes: &[u8]) -> Result<ArtifactId, ModelTransportError> {
        let media = media.to_owned();
        let bytes = bytes.to_vec();
        let model_step_id = self.snapshot.model_step_id;
        self.store
            .write(self.step("agent.model.artifact"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    store::put_artifact(tx, model_step_id, &media, &bytes, UtcTimestamp::now())
                })
            })
            .await
            .map_err(error)
    }

    async fn verify_attempt(&self, id: TransportAttemptId) -> Result<(), ModelTransportError> {
        let snapshot_id = self.snapshot.id;
        let attempt = self
            .store
            .read(self.step("agent.model.attempt_owner"), move |db| store::attempt(db, id))
            .await
            .map_err(error)?;
        if !matches!(
            attempt.request_evidence.operation_context,
            TransportOperationContext::AgentModelAttempt { model_execution_snapshot_id, .. }
                if model_execution_snapshot_id == snapshot_id
        ) {
            return Err(detail("model.attempt_owner"));
        }
        Ok(())
    }
}

impl RawStreamWriter for ModelStreamWriter {
    fn append(&mut self, bytes: &[u8]) -> Result<(), ModelTransportError> {
        if self.bytes.len().checked_add(bytes.len()).is_none_or(|n| n > 32 * 1024 * 1024) {
            return Err(detail("model.raw_stream_limit"));
        }
        self.bytes.try_reserve(bytes.len()).map_err(detail)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    async fn finalize(self) -> Result<ArtifactId, ModelTransportError> {
        self.owner.put("text/event-stream", &self.bytes).await
    }
}

impl TransportStore for AgentModelStore {
    type Writer = ModelStreamWriter;

    async fn record_execution_snapshot(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Result<(), ModelTransportError> {
        if *snapshot != self.snapshot {
            return Err(detail("model.snapshot_changed"));
        }
        let expected = snapshot.clone();
        let id = snapshot.id;
        let stored = self
            .store
            .read(self.step("agent.model.snapshot_binding"), move |db| {
                aura_persistence_v2::model::snapshot(db, id)
            })
            .await
            .map_err(error)?;
        if stored.as_ref() != Some(&expected) {
            return Err(detail("model.snapshot_missing"));
        }
        Ok(())
    }

    async fn canonical_request_body_artifact(
        &self,
        _: TransportAttemptId,
        kind: TransportAttemptKind,
        plan: &ProviderRequestPlan,
    ) -> Result<Option<ArtifactId>, ModelTransportError> {
        if kind != TransportAttemptKind::CompatibilityAdjustedRetry {
            return Ok(None);
        }
        self.put("application/json", &plan.body).await.map(Some)
    }

    async fn begin_raw_stream(
        &self,
        id: TransportAttemptId,
    ) -> Result<Self::Writer, ModelTransportError> {
        self.verify_attempt(id).await?;
        Ok(ModelStreamWriter { owner: self.clone(), bytes: vec![] })
    }

    async fn provider_continuation_artifact(
        &self,
        id: TransportAttemptId,
        value: &ProviderContinuationArtifact,
    ) -> Result<ArtifactId, ModelTransportError> {
        self.verify_attempt(id).await?;
        self.put(&value.media_type, &value.bytes).await
    }

    async fn record_attempt(
        &self,
        value: &TransportAttemptRecord,
    ) -> Result<(), ModelTransportError> {
        let value = value.clone();
        let snapshot = self.snapshot.clone();
        let answer = self
            .store
            .write(self.step("agent.model.attempt_record"), move |journal| {
                journal.database.unit_of_work(|tx| store::record_attempt(tx, &snapshot, &value))
            })
            .await;
        self.settled(answer).await
    }

    async fn record_failure(
        &self,
        value: &PersistedModelFailure,
    ) -> Result<(), ModelTransportError> {
        let value = value.clone();
        let model_step_id = self.snapshot.model_step_id;
        let answer = self
            .store
            .write(self.step("agent.model.failure_record"), move |journal| {
                journal.database.unit_of_work(|tx| store::record_failure(tx, model_step_id, &value))
            })
            .await;
        self.settled(answer).await
    }

    async fn record_output(
        &self,
        snapshot: &ModelExecutionSnapshot,
        id: TransportAttemptId,
        output: &NormalizedModelOutput,
    ) -> Result<(), ModelTransportError> {
        if *snapshot != self.snapshot {
            return Err(detail("model.snapshot_changed"));
        }
        let snapshot = snapshot.clone();
        let output = output.clone();
        let hold = self.hold_completion;
        let answer = self
            .store
            .write(self.step("agent.model.output_record"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    store::record_output(tx, &snapshot, id, &output, hold, UtcTimestamp::now())
                })
            })
            .await;
        self.settled(answer).await
    }
}

/// A store answer that never reached business work must not be read as a completed one.
#[allow(dead_code)] // read by the R1 regressions and the diagnostics
impl AgentModelStore {
    pub(super) fn classify(answer: &StoreError) -> &'static str {
        match answer {
            StoreError::Refused { .. } => "not_started",
            StoreError::Unknown { .. } => "unknown_result_retained",
            StoreError::Failed { .. } => "operation_failed",
            StoreError::Unavailable { .. } => "dependency_unavailable",
        }
    }
}
