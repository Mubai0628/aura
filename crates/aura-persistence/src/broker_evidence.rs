//! Read-only receipt projections of the domain's existing commit journals.
//! Broker does not issue workspace SQL or duplicate domain business records.
use crate::{PersistenceCoordinator, PersistenceError};
use aura_domain_common::CorrelationId;
use serde_json::{Value, json};

/// The prefix every broker evidence read's identity starts with.
///
/// It is a constant because it is what makes the POST-EXECUTION step of a command nameable: the
/// evidence read happens after that command's handler has already run, and a hold that means "park
/// one command's evidence step" has to be able to say so before the command is sent, when the
/// correlation it will carry is not yet known.
pub const BROKER_EVIDENCE_IDENTITY_PREFIX: &str = "broker-evidence:";

impl PersistenceCoordinator {
    /// The identity one command's evidence read is recorded under.
    ///
    /// The evidence read is a real storage step of exactly one invocation's own path, so it carries
    /// that invocation's correlation. Naming it is what lets the step be told from any other
    /// sync-class arrival - by this runtime's own terminal records and by a driver that may only
    /// select what it can name in advance. It is derived, never stored, so the same correlation
    /// always produces the same identity.
    #[must_use]
    pub fn broker_evidence_identity(correlation: CorrelationId) -> String {
        format!("{BROKER_EVIDENCE_IDENTITY_PREFIX}{correlation}")
    }

    pub fn workspace_broker_evidence(
        &self,
        correlation: CorrelationId,
    ) -> Result<Value, PersistenceError> {
        // The interactive lane and the ordinary Sync budget, unchanged: this is the same dispatch
        // the caller had, with the caller's own identity attached to the job.
        self.dispatch_class_identified(
            crate::writer_queue::WorkClass::Sync,
            Some(std::time::Instant::now() + self.sync_deadline()),
            Self::broker_evidence_identity(correlation),
            move |connection| {
                let mut query = connection.prepare(
                    "SELECT journal_id,operation_kind,state,trace_json FROM workspace_operation_journals
                 WHERE json_extract(trace_json,'$.correlation_id')=?1 ORDER BY journal_id"
                )?;
                let rows = query.query_map([correlation.to_string()], |row| {
                    Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?,row.get::<_, String>(2)?,row.get::<_, String>(3)?))
                })?;
                let mut result = Vec::new();
                for row in rows {
                    let (id, kind, state, trace) = row?;
                    result.push(json!({"journal_id":id,"kind":kind,"state":state,"trace":serde_json::from_str::<Value>(&trace)?}));
                }
                Ok(Value::Array(result))
            },
        )
    }
}
