use std::sync::Arc;

use aura_artifact_contract::{
    ArtifactAccess, ArtifactOperationOwnerRef, ArtifactOwnerRef, ArtifactScope,
};
use aura_domain_common::{ArtifactId, ModelStepId, SessionId, TransportAttemptId};
use aura_model_domain::{
    ModelExecutionSnapshot, NormalizedModelOutput, PersistedModelFailure, TransportAttemptKind,
    TransportAttemptRecord, TransportOperationContext,
};
use aura_model_transport::{
    ModelTransportError, ProviderContinuationArtifact, ProviderRequestPlan, RawStreamWriter,
    TransportStore,
};
use aura_persistence::{ArtifactWriteRequest, PersistenceCoordinator};

/// P06-owned lookup used only for an Agent model attempt.
///
/// Discovery and Endpoint Connection Test owners are fully represented by
/// `TransportOperationContext` and never need a synthetic Session. Until P06
/// installs a real resolver, Agent artifact persistence fails closed.
pub trait AgentModelArtifactOwnerResolver: Send + Sync {
    fn session_for_model_step(
        &self,
        model_step_id: ModelStepId,
    ) -> Result<SessionId, ModelTransportError>;
}

#[derive(Default)]
pub struct UnavailableAgentModelArtifactOwnerResolver;

impl AgentModelArtifactOwnerResolver for UnavailableAgentModelArtifactOwnerResolver {
    fn session_for_model_step(
        &self,
        _model_step_id: ModelStepId,
    ) -> Result<SessionId, ModelTransportError> {
        Err(ModelTransportError::Store(
            "P06 has not installed the Agent ModelStep artifact-owner resolver".to_owned(),
        ))
    }
}

#[derive(Clone)]
pub struct PersistentTransportStore {
    persistence: Arc<PersistenceCoordinator>,
    agent_owner_resolver: Arc<dyn AgentModelArtifactOwnerResolver>,
    max_raw_stream_bytes: usize,
    /// The bounded owner of this store's synchronous persistence work.
    ///
    /// WHY. Every method below really writes the legacy persistence coordinator's database or
    /// artifact store. The transport drives them from an async task, so a synchronous call parked
    /// whichever Tokio worker ran it. They are now one admitted section each, on a fixed-size
    /// [`crate::blocking_pool::BlockingPool`] with the same capacity as every other bounded owner
    /// in the Core; the pool is never grown and a section that cannot get a permit inside the
    /// budget is refused as "did not run" instead of queueing on the reactor.
    ///
    /// This instance belongs to the P05 model *configuration / discovery / availability* path.
    /// The Agent run path uses `AgentModelStore` over `AgentStore`, so the two dependency paths do
    /// not share a permit budget and neither can starve the other.
    pool: crate::blocking_pool::BlockingPool,
}

impl PersistentTransportStore {
    pub fn new(
        persistence: Arc<PersistenceCoordinator>,
        agent_owner_resolver: Arc<dyn AgentModelArtifactOwnerResolver>,
        max_raw_stream_bytes: usize,
    ) -> Result<Self, ModelTransportError> {
        if max_raw_stream_bytes == 0 {
            return Err(ModelTransportError::Store(
                "raw model stream byte limit must not be zero".to_owned(),
            ));
        }
        Ok(Self {
            persistence,
            agent_owner_resolver,
            max_raw_stream_bytes,
            pool: crate::blocking_pool::BlockingPool::new(crate::blocking_pool::BLOCKING_WORKERS),
        })
    }

    /// One admitted synchronous section of this store.
    ///
    /// * **admission** - one permit from this store's fixed pool.
    /// * **deadline** - the boundary's production section budget.
    /// * **owner** - the permit belongs to the blocking task, released by the work itself.
    /// * **classification** - `Refused` means the work provably did not run; `Unknown` means it
    ///   started and may still commit. They are carried as two different typed codes.
    async fn section<T, F>(
        &self,
        operation: &'static str,
        work: F,
    ) -> Result<T, ModelTransportError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, ModelTransportError> + Send + 'static,
    {
        let deadline = std::time::Instant::now() + crate::sync_boundary::SYNC_SECTION_BUDGET;
        match self.pool.run_settled(deadline, work, |_: &Result<T, ModelTransportError>| {}).await {
            crate::blocking_pool::BlockingAdmission::Completed(result) => result,
            crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                Err(ModelTransportError::Store(format!(
                    "p05.model_store_refused: {operation} did not run; no store worker was free \
                     within {waited_ms} ms"
                )))
            }
            crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                Err(ModelTransportError::Store(format!(
                    "p05.model_store_unknown: {operation} started and did not answer within \
                     {waited_ms} ms; its own permit still owns the work"
                )))
            }
        }
    }

    pub async fn write_operation_artifact(
        &self,
        operation_context: &TransportOperationContext,
        media_type: String,
        bytes: Vec<u8>,
    ) -> Result<ArtifactId, ModelTransportError> {
        let (owner, scope) = self.owner_and_scope(operation_context)?;
        let persistence = Arc::clone(&self.persistence);
        self.section("p05.model_operation_artifact", move || {
            persistence
                .write_artifact(ArtifactWriteRequest {
                    scope,
                    media_type,
                    access: ArtifactAccess::NONE,
                    owner,
                    bytes,
                })
                .map(|record| record.artifact_id)
                .map_err(persistence_error)
        })
        .await
    }

    fn owner_and_scope(
        &self,
        operation_context: &TransportOperationContext,
    ) -> Result<(ArtifactOwnerRef, ArtifactScope), ModelTransportError> {
        match operation_context {
            TransportOperationContext::AgentModelAttempt { model_step_id, .. } => {
                let session_id =
                    self.agent_owner_resolver.session_for_model_step(*model_step_id)?;
                Ok((ArtifactOwnerRef::Session { session_id }, ArtifactScope::Session))
            }
            TransportOperationContext::ModelDiscovery { discovery_run_id } => Ok((
                ArtifactOwnerRef::Operation {
                    operation: ArtifactOperationOwnerRef::ModelDiscovery {
                        model_discovery_run_id: *discovery_run_id,
                    },
                },
                ArtifactScope::SystemCache,
            )),
            TransportOperationContext::ModelAvailabilityTest {
                model_availability_test_id, ..
            } => Ok((
                ArtifactOwnerRef::Operation {
                    operation: ArtifactOperationOwnerRef::ModelAvailabilityTest {
                        model_availability_test_id: *model_availability_test_id,
                    },
                },
                ArtifactScope::SystemCache,
            )),
        }
    }

    async fn operation_context_for_attempt(
        &self,
        attempt_id: TransportAttemptId,
    ) -> Result<TransportOperationContext, ModelTransportError> {
        let persistence = Arc::clone(&self.persistence);
        self.section("p05.model_attempt_owner", move || {
            persistence
                .transport_attempt(attempt_id)
                .map_err(persistence_error)?
                .map(|attempt| attempt.request_evidence.operation_context)
                .ok_or_else(|| {
                    ModelTransportError::Store(format!(
                        "transport attempt {attempt_id} has not been persisted"
                    ))
                })
        })
        .await
    }
}

pub struct PersistentRawStreamWriter {
    store: PersistentTransportStore,
    operation_context: TransportOperationContext,
    max_bytes: usize,
    bytes: Vec<u8>,
}

impl RawStreamWriter for PersistentRawStreamWriter {
    fn append(&mut self, bytes: &[u8]) -> Result<(), ModelTransportError> {
        // Pure bounded memory: no database, no file, no lock. It stays synchronous on purpose;
        // making it awaitable would create a storage task for an operation that touches no store.
        let next_length = self.bytes.len().checked_add(bytes.len()).ok_or_else(|| {
            ModelTransportError::Store("raw model stream length overflowed usize".to_owned())
        })?;
        if next_length > self.max_bytes {
            return Err(ModelTransportError::Store(
                "raw model stream exceeded its configured persistence limit".to_owned(),
            ));
        }
        self.bytes.try_reserve(bytes.len()).map_err(|_| {
            ModelTransportError::Store("raw model stream allocation failed".to_owned())
        })?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    async fn finalize(self) -> Result<ArtifactId, ModelTransportError> {
        self.store
            .write_operation_artifact(
                &self.operation_context,
                "text/event-stream".to_owned(),
                self.bytes,
            )
            .await
    }
}

impl TransportStore for PersistentTransportStore {
    type Writer = PersistentRawStreamWriter;

    async fn record_execution_snapshot(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Result<(), ModelTransportError> {
        let persistence = Arc::clone(&self.persistence);
        let snapshot = snapshot.clone();
        self.section("p05.model_execution_snapshot", move || {
            persistence.store_model_execution_snapshot(&snapshot).map_err(persistence_error)
        })
        .await
    }

    async fn canonical_request_body_artifact(
        &self,
        _attempt_id: TransportAttemptId,
        attempt_kind: TransportAttemptKind,
        request_plan: &ProviderRequestPlan,
    ) -> Result<Option<ArtifactId>, ModelTransportError> {
        if attempt_kind != TransportAttemptKind::CompatibilityAdjustedRetry {
            return Ok(None);
        }
        self.write_operation_artifact(
            &request_plan.request_evidence.operation_context,
            "application/json".to_owned(),
            request_plan.body.clone(),
        )
        .await
        .map(Some)
    }

    async fn begin_raw_stream(
        &self,
        attempt_id: TransportAttemptId,
    ) -> Result<Self::Writer, ModelTransportError> {
        let operation_context = self.operation_context_for_attempt(attempt_id).await?;
        Ok(PersistentRawStreamWriter {
            store: self.clone(),
            operation_context,
            max_bytes: self.max_raw_stream_bytes,
            bytes: Vec::new(),
        })
    }

    async fn provider_continuation_artifact(
        &self,
        attempt_id: TransportAttemptId,
        continuation: &ProviderContinuationArtifact,
    ) -> Result<ArtifactId, ModelTransportError> {
        let operation_context = self.operation_context_for_attempt(attempt_id).await?;
        self.write_operation_artifact(
            &operation_context,
            continuation.media_type.clone(),
            continuation.bytes.clone(),
        )
        .await
    }

    async fn record_attempt(
        &self,
        attempt: &TransportAttemptRecord,
    ) -> Result<(), ModelTransportError> {
        let persistence = Arc::clone(&self.persistence);
        let attempt = attempt.clone();
        self.section("p05.model_attempt_record", move || {
            persistence.upsert_transport_attempt(&attempt).map_err(persistence_error)
        })
        .await
    }

    async fn record_failure(
        &self,
        failure: &PersistedModelFailure,
    ) -> Result<(), ModelTransportError> {
        let persistence = Arc::clone(&self.persistence);
        let failure = failure.clone();
        self.section("p05.model_failure_record", move || {
            persistence.store_model_failure(&failure).map_err(persistence_error)
        })
        .await
    }

    async fn record_output(
        &self,
        snapshot: &ModelExecutionSnapshot,
        attempt_id: TransportAttemptId,
        output: &NormalizedModelOutput,
    ) -> Result<(), ModelTransportError> {
        let persistence = Arc::clone(&self.persistence);
        let snapshot = snapshot.clone();
        let output = output.clone();
        self.section("p05.model_output_record", move || {
            persistence
                .store_model_output(&snapshot, attempt_id, &output)
                .map_err(persistence_error)
        })
        .await
    }
}

fn persistence_error(error: aura_persistence::PersistenceError) -> ModelTransportError {
    ModelTransportError::Store(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aura_artifact_contract::{ArtifactOperationOwnerRef, ArtifactOwnerRef, ArtifactScope};
    use aura_domain_common::{
        ModelAvailabilityTestId, ModelDiscoveryRunId, ModelProfileId, RuntimeInstanceId,
    };
    use aura_model_domain::TransportOperationContext;
    use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};

    use super::{PersistentTransportStore, UnavailableAgentModelArtifactOwnerResolver};

    fn store() -> (tempfile::TempDir, PersistentTransportStore) {
        let root = tempfile::tempdir().expect("temp root");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            root.path().join("aura.db"),
            root.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence start")
        {
            PersistenceStartup::Ready(value) => Arc::new(value),
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected recovery: {}", recovery.reason)
            }
        };
        let store = PersistentTransportStore::new(
            persistence,
            Arc::new(UnavailableAgentModelArtifactOwnerResolver),
            16 * 1024,
        )
        .expect("transport store");
        (root, store)
    }

    #[test]
    fn operation_artifacts_derive_exact_direct_owner_and_system_cache_scope() {
        let (_root, store) = store();
        let discovery_run_id = ModelDiscoveryRunId::new();
        let (owner, scope) = store
            .owner_and_scope(&TransportOperationContext::ModelDiscovery { discovery_run_id })
            .expect("discovery owner");
        assert_eq!(scope, ArtifactScope::SystemCache);
        assert_eq!(
            owner,
            ArtifactOwnerRef::Operation {
                operation: ArtifactOperationOwnerRef::ModelDiscovery {
                    model_discovery_run_id: discovery_run_id,
                },
            }
        );

        let model_availability_test_id = ModelAvailabilityTestId::new();
        let model_profile_id = ModelProfileId::new();
        let (owner, scope) = store
            .owner_and_scope(&TransportOperationContext::ModelAvailabilityTest {
                model_availability_test_id,
                model_profile_id,
            })
            .expect("availability-test owner");
        assert_eq!(scope, ArtifactScope::SystemCache);
        assert_eq!(
            owner,
            ArtifactOwnerRef::Operation {
                operation: ArtifactOperationOwnerRef::ModelAvailabilityTest {
                    model_availability_test_id,
                },
            }
        );
    }
}
