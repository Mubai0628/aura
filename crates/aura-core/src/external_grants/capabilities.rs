use super::*;

impl ExternalGrantService {
    /// Trusted, live coverage before policy or an Attempt. Bare grant IDs carry no authority.
    pub(crate) fn covers_capabilities(
        &self,
        grant: ExternalGrantId,
        session: SessionId,
        capabilities: &[ResolvedCapability],
    ) -> bool {
        !capabilities.is_empty()
            && capabilities.iter().all(|capability| {
                let needed = match capability {
                    ResolvedCapability::ExternalFileRead => ExternalGrantCapability::Read,
                    ResolvedCapability::ExternalFileWrite => ExternalGrantCapability::Write,
                    ResolvedCapability::ExternalFileCreate => ExternalGrantCapability::Create,
                    ResolvedCapability::ExternalFileDelete => ExternalGrantCapability::Delete,
                    _ => return false,
                };
                self.verify_active(grant, session, needed, CorrelationId::new()).is_ok()
                    || matches!(
                        needed,
                        ExternalGrantCapability::Create | ExternalGrantCapability::Delete
                    ) && self
                        .verify_active(
                            grant,
                            session,
                            ExternalGrantCapability::Write,
                            CorrelationId::new(),
                        )
                        .is_ok()
            })
    }
}
