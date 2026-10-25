//! A session transaction's receipt outlives an individual caller's wait.
//! Admission stays bounded; waiting for an admitted job consumes no writer or Core worker.
use aura_domain_common::{CommandInvocationId, UtcTimestamp};
use aura_permission_domain::SessionPermissionProjection;
use aura_workspace_domain::{SessionSummary, WorkspaceSessionProjection};

use crate::{CommittedProjection, PersistenceCoordinator, PersistenceError};

pub struct SessionWriteReceipt {
    pub(crate) receiver:
        tokio::sync::oneshot::Receiver<Result<SessionWriteCompletion, PersistenceError>>,
}

pub struct SessionWriteCompletion {
    pub(crate) session: SessionSummary,
    pub(crate) permission: Option<crate::security::SecurityMutation<SessionPermissionProjection>>,
    pub(crate) projection: Option<CommittedProjection<WorkspaceSessionProjection>>,
}

pub struct SessionPublicationReceipt {
    receiver: tokio::sync::oneshot::Receiver<Result<SessionSummary, PersistenceError>>,
}

impl SessionPublicationReceipt {
    pub async fn receive(self) -> Result<SessionSummary, PersistenceError> {
        self.receiver.await.map_err(|_| PersistenceError::WriterResponseClosed)?
    }
}

impl SessionWriteReceipt {
    /// Receive the original transaction's result. The caller must transfer this receipt to its
    /// bounded completion owner before it starts a timed wait; it must never resubmit the write.
    pub async fn receive(self) -> Result<SessionWriteCompletion, PersistenceError> {
        self.receiver.await.map_err(|_| PersistenceError::WriterResponseClosed)?
    }
}

impl PersistenceCoordinator {
    /// Publish the transaction's existing projections, then retain the actual outbox receipt.
    /// Waiting for an outbox acknowledgement must not occupy a synchronous Core worker.
    pub fn publish_session_write(
        &self,
        command: CommandInvocationId,
        completion: SessionWriteCompletion,
    ) -> Result<SessionPublicationReceipt, PersistenceError> {
        let mut outbox_ids = Vec::new();
        if let Some(permission) = completion.permission {
            for projection in permission.into_projections() {
                outbox_ids.push(projection.outbox_id);
                self.publish_security(projection)?;
            }
        }
        if let Some(projection) = completion.projection {
            outbox_ids.push(projection.outbox_id);
            self.publish_workspace_session(projection)?;
        }
        let receiver =
            self.dispatch_receipt(format!("session-publication:{command}"), move |connection| {
                let transaction = connection.transaction()?;
                for outbox_id in outbox_ids {
                    transaction.execute(
                        "UPDATE projection_outbox SET published_at=?1 WHERE outbox_id=?2",
                        rusqlite::params![UtcTimestamp::now().to_string(), outbox_id],
                    )?;
                }
                transaction.commit()?;
                Ok(completion.session)
            })?;
        Ok(SessionPublicationReceipt { receiver })
    }

    pub fn finish_session_write(
        &self,
        completion: SessionWriteCompletion,
    ) -> Result<SessionSummary, PersistenceError> {
        if let Some(permission) = completion.permission {
            self.finish_security_mutation(permission)?;
        }
        if let Some(projection) = completion.projection {
            self.publish_and_mark(projection)?;
        }
        Ok(completion.session)
    }
}
