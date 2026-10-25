use super::*;

impl CommittedToolBatchExecutionV1 {
    #[must_use]
    pub fn admitted_event_id(&self) -> Option<EventId> {
        self.persistence.event_ids.first().copied()
    }
}

impl ToolBatchAdmissionServiceV1 {
    pub fn prepare(
        self,
        input: PrepareToolBatchExecutionInputV1<'_>,
    ) -> Result<ToolBatchPreparationOutcomeV1, ToolBatchPreparationErrorV1> {
        use aura_execution_orchestrator::{BatchPreparation, PreparedBatch};
        let prepared = match aura_execution_orchestrator::prepare_batch(input)? {
            BatchPreparation::IdentityIntegrityFault { code } => {
                return Ok(ToolBatchPreparationOutcomeV1::IdentityIntegrityFault { code });
            }
            BatchPreparation::BatchRejected { code, results } => {
                return Ok(ToolBatchPreparationOutcomeV1::BatchRejected { code, results });
            }
            BatchPreparation::Prepared(prepared) => *prepared,
        };
        let PreparedBatch {
            admitted,
            planned,
            projection_state,
            projections,
            not_executed_results,
            admitted_at,
            correlation_id,
        } = prepared;
        let commit_request = ToolBatchAdmissionCommitRequest {
            batch: admitted.batch.clone(),
            execution_plan: planned.execution_plan.clone(),
            not_executed_results,
            batch_projection: projections.batch,
            call_projections: projections.calls,
            session_activity_projection: projections.session,
            admitted_at,
            correlation_id,
        };
        Ok(ToolBatchPreparationOutcomeV1::Prepared(Box::new(PreparedToolBatchExecutionV1 {
            admitted,
            planned,
            projection_state,
            commit_request,
        })))
    }
}

impl PreparedToolBatchExecutionV1 {
    pub fn commit(
        self,
        persistence: &PersistenceCoordinator,
    ) -> Result<CommittedToolBatchExecutionV1, PersistenceError> {
        let committed = persistence.commit_tool_batch_admission(self.commit_request)?;
        let projection_publish_error =
            persistence.publish_tool_projection_changes(&committed.projection_changes).err();
        Ok(CommittedToolBatchExecutionV1 {
            admitted: self.admitted,
            planned: self.planned,
            projection_state: self.projection_state,
            persistence: committed,
            projection_publish_error,
        })
    }
}
