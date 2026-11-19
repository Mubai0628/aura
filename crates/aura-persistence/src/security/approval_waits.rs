//! Presentation and expiry for approvals projected through the legacy session tray.
use super::*;
use aura_domain_common::human_wait::{Activity, HumanWait, valid_viewer};

fn load(db: &Connection, id: ApprovalRequestId) -> Result<Option<HumanWait>, PersistenceError> {
    db.query_row(
        "SELECT timing_json FROM approval_wait_state WHERE approval_request_id=?1",
        [id.to_string()],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|json| Ok(serde_json::from_str(&json)?))
    .transpose()
}

pub(super) fn require_open(
    db: &Connection,
    id: ApprovalRequestId,
    at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    if load(db, id)?.is_some_and(|wait| wait.deadline <= at) {
        return Err(PersistenceError::ApprovalStale);
    }
    Ok(())
}

impl PersistenceCoordinator {
    pub fn update_legacy_approval_wait(
        &self,
        session: SessionId,
        id: ApprovalRequestId,
        viewer: String,
        activity: Activity,
    ) -> Result<(HumanWait, bool), PersistenceError> {
        if !valid_viewer(&viewer) {
            return Err(PersistenceError::InvalidRequest("invalid approval viewer".into()));
        }
        self.dispatch(move |connection| {
            let tx = connection.transaction()?;
            let approval = load_approval(&tx, id)?.ok_or(PersistenceError::ApprovalNotFound)?;
            if approval.identity.session_id != Some(session) || approval.state != ApprovalState::Pending {
                return Err(PersistenceError::ApprovalStale);
            }
            let at = UtcTimestamp::now();
            let prior = load(&tx, id)?;
            if prior.as_ref().is_some_and(|wait| wait.deadline <= at) {
                return Err(PersistenceError::ApprovalStale);
            }
            let next = HumanWait::advance(prior.as_ref(), &viewer, activity, at)
                .ok_or(PersistenceError::ApprovalStale)?;
            if prior.as_ref() != Some(&next) {
                tx.execute(
                    "INSERT INTO approval_wait_state(approval_request_id,timing_json) VALUES (?1,?2)
                     ON CONFLICT(approval_request_id) DO UPDATE SET timing_json=excluded.timing_json",
                    params![id.to_string(), serde_json::to_string(&next)?],
                )?;
            }
            tx.commit()?;
            Ok((next.clone(), prior.is_none_or(|wait| next.deadline < wait.deadline)))
        })
    }

    /// Returns the new deadline if a live window moved it ahead of the timer.
    pub fn expire_legacy_approval_wait(
        &self,
        id: ApprovalRequestId,
    ) -> Result<Option<UtcTimestamp>, PersistenceError> {
        let (deadline, projection) = self.dispatch(move |connection| {
            let tx = connection.transaction()?;
            let Some(mut approval) = load_approval(&tx, id)? else { return Ok((None, None)) };
            let Some(wait) = load(&tx, id)? else { return Ok((None, None)) };
            if approval.state != ApprovalState::Pending {
                return Ok((None, None));
            }
            let at = UtcTimestamp::now();
            if wait.deadline > at {
                return Ok((Some(wait.deadline), None));
            }
            approval.state = ApprovalState::Expired;
            approval.request_revision =
                approval.request_revision.checked_add(1).ok_or_else(|| {
                    PersistenceError::InvalidRequest("approval revision exhausted".into())
                })?;
            approval.resolved_at = Some(at);
            update_approval(&tx, &approval)?;
            let mut event = PersistentEventDraft::system(
                "ApprovalExpired",
                CorrelationId::new(),
                json!({"approval_id": id}),
            );
            event.workspace_id = approval.identity.workspace_id;
            event.session_id = approval.identity.session_id;
            event.turn_task_id = approval.identity.turn_task_id;
            event.agent_run_id = approval.identity.agent_run_id;
            insert_event(&tx, &event)?;
            let projection = commit_pending_approval_projection(&tx)?;
            tx.commit()?;
            Ok((None, Some(projection)))
        })?;
        if let Some(projection) = projection {
            self.publish_and_mark_security(projection)?;
        }
        Ok(deadline)
    }
}
