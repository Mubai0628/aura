//! Ending a prepared bundle by its transaction identity, for a caller that no longer holds it.
use super::*;

impl SettingsFacade {
    /// Ends a prepared transaction that was never published, releasing its domains.
    ///
    /// A preparation can fail after the bundle was reserved and before any durable record owns
    /// it, and by then the caller may hold only the transaction identity. Publication writes the
    /// transaction journal before it replaces any file, so a live transaction without a journal
    /// has changed nothing and ending it is safe; one with a journal is left to its owner and
    /// reported as pending. Returns whether a live transaction was ended.
    pub fn abort_unpublished_module_bundle(
        &self,
        transaction_id: &str,
    ) -> Result<bool, SettingsFacadeError> {
        if !self.is_transaction_live(transaction_id)? {
            return Ok(false);
        }
        let journal_path =
            config_bundle_transaction_dir(&self.paths).join(format!("{transaction_id}.json"));
        if journal_path.exists() {
            return Err(SettingsFacadeError::BundlePending {
                transaction_id: transaction_id.to_owned(),
            });
        }
        self.end_transaction(transaction_id)?;
        Ok(true)
    }
}
