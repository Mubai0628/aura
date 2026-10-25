//! Existing approval-to-directory bindings; no new storage or migration.
use crate::{PersistenceCoordinator, PersistenceError};
use aura_domain_common::{ApprovalRequestId, ExternalGrantId};
use std::str::FromStr;

impl PersistenceCoordinator {
    pub fn directory_approval_ids(
        &self,
        grant: ExternalGrantId,
    ) -> Result<Vec<ApprovalRequestId>, PersistenceError> {
        self.with_reader(|db| {
            let mut query = db.prepare(
                "SELECT approval_id FROM external_grant_decisions WHERE external_grant_id=?1 ORDER BY approval_id",
            )?;
            let values = query.query_map([grant.to_string()], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            values.into_iter().map(|id| ApprovalRequestId::from_str(&id).map_err(PersistenceError::from)).collect()
        })
    }
}
