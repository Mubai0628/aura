use crate::*;

pub(crate) struct CoreConfigTransactionRecovery;

impl ConfigTransactionRecovery for CoreConfigTransactionRecovery {
    type Error = std::io::Error;

    fn cleanup_prepared(
        &mut self,
        _transaction_id: &str,
        target: &ConfigTransactionTarget,
    ) -> Result<(), Self::Error> {
        if target.owned_paths.is_empty() && target.credential_refs.is_empty() {
            return Ok(());
        }
        Err(std::io::Error::other(
            "prepared config transaction requires a component-owned, identity-checked cleanup",
        ))
    }

    fn complete_committed(
        &mut self,
        _transaction_id: &str,
        _target: &ConfigTransactionTarget,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}
