//! A directory record derived from file consent cannot outlive that consent.
use super::*;

impl ExternalGrantService {
    pub(super) fn file_approval_in_force(
        &self,
        record: &ExternalGrantRecord,
    ) -> Result<bool, ExternalGrantError> {
        let approvals = self.persistence.directory_approval_ids(record.id)?;
        if approvals.is_empty() {
            return Ok(true);
        }
        let journal = self.bound_journal().ok_or(ExternalGrantError::PermissionGrantUnavailable)?;
        let store = journal.lock().map_err(|_| ExternalGrantError::PermissionGrantUnavailable)?;
        let db = store.database.connection();
        let mut file_origin = false;
        for approval in approvals {
            let grants = aura_persistence_v2::file_consents::for_approval(db, approval)
                .map_err(|_| ExternalGrantError::PermissionGrantUnavailable)?;
            // A reused directory record can also have an independent approval.
            // Withdrawing file consent must preserve that separate authority.
            if grants.is_empty() {
                return Ok(true);
            }
            for grant in grants {
                file_origin = true;
                if grant.is_live_at(UtcTimestamp::now())
                    && aura_persistence_v2::workspace::session(db, record.session_id)
                        .map_err(|_| ExternalGrantError::PermissionGrantUnavailable)?
                        .is_some()
                    && grant.scope.file_consent.as_ref().is_some_and(|consent| {
                        consent.session_id == record.session_id
                            && consent.principal_id
                                == aura_domain_common::PrincipalId::from_bytes(
                                    record.session_id.to_bytes(),
                                )
                            && consent.grant_revision == grant.revision
                            && record.capabilities.iter().all(|cap| {
                                consent.capabilities.contains(&match cap {
                                    ExternalGrantCapability::Read => {
                                        ResolvedCapability::ExternalFileRead
                                    }
                                    ExternalGrantCapability::Write => {
                                        ResolvedCapability::ExternalFileWrite
                                    }
                                    ExternalGrantCapability::Create => {
                                        ResolvedCapability::ExternalFileCreate
                                    }
                                    ExternalGrantCapability::Delete => {
                                        ResolvedCapability::ExternalFileDelete
                                    }
                                    ExternalGrantCapability::ModelEgress => return false,
                                })
                            })
                            && consent.targets.iter().any(|target| {
                                target.identity_key
                                    == aura_security_contract::file_object_key(
                                        &record.root_identity,
                                    )
                            })
                    })
                {
                    return Ok(true);
                }
            }
        }
        // Picker grants and independently issued legacy grants have their own authority.
        Ok(!file_origin)
    }

    pub(crate) fn withdraw_file_companions(
        &self,
        consent: &aura_security_contract::FileSessionConsentRef,
        correlation: CorrelationId,
    ) -> Result<(), ExternalGrantError> {
        for id in self
            .persistence
            .directory_grants_for_operation(consent.approved_operation_fingerprint.0.hex())?
        {
            let record = self.record(id)?;
            if record.session_id == consent.session_id
                && record.term == ExternalGrantTerm::Session
                && record.lifecycle != ExternalGrantLifecycle::Revoked
                && !self.file_approval_in_force(&record)?
            {
                self.persistence.revoke_external_grant(id, consent.session_id, correlation)?;
            }
        }
        Ok(())
    }
}
