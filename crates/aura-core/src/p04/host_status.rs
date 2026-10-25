use super::*;

impl P04Services {
    /// The ordinary status read.
    ///
    /// Three questions are kept apart here, because they cost very different amounts and
    /// only one of them is a read:
    ///
    /// * *what does the stored state say?* One light read. This is the answer in the
    ///   ordinary case, and it never probes the host and never queues a write.
    /// * *is a prepared plan past its own deadline?* One indexed pre-check, and only
    ///   then is a settlement asked for. The settlement re-checks both facts inside its
    ///   transaction, so this pre-check can never settle a plan that was completed or
    ///   replaced in between; it only decides whether asking is worth it.
    /// * *where is the expensive work done?* On the Host Preparation worker, which owns
    ///   the host probe and the settlement write. A caller whose budget expires is told
    ///   the component is busy instead of waiting behind the persistence writer.
    pub(super) fn host_preparation_status(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<HostPreparationStatus, IpcErrorEnvelope> {
        let started = std::time::Instant::now();
        let stored = self
            .persistence
            .host_preparation_state()
            .map_err(|error| persistence_error(error, correlation_id))?;
        record_host_preparation_stage(crate::host_preparation::HostPreparationStageReport {
            stage: crate::host_preparation::HostPreparationStage::StatusRead,
            correlation_id,
            operation_id: stored
                .as_ref()
                .and_then(|state| state.status.active_operation.as_ref())
                .map(|operation| operation.operation_id),
            ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        let checked = std::time::Instant::now();
        let expired = self
            .persistence
            .prepared_host_preparation_expiry()
            .map_err(|error| persistence_error(error, correlation_id))?;
        record_host_preparation_stage(crate::host_preparation::HostPreparationStageReport {
            stage: crate::host_preparation::HostPreparationStage::ExpiryChecked,
            correlation_id,
            operation_id: expired.map(|expiry| expiry.operation_id),
            ms: u64::try_from(checked.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        if let Some(expiry) = expired
            && expiry.expires_at < UtcTimestamp::now()
        {
            // The settlement writes on the worker, which reports what really happened: an
            // error or an expired budget is surfaced rather than folded into success, so a
            // caller is never told a plan was settled when it was not.
            let settled = self.host_preparation.settle(expiry.operation_id, correlation_id)?;
            if settled.is_some()
                && let Ok(mut nonces) = self.host_preparation_nonces.lock()
            {
                // The nonce is forgotten only when the settlement really happened: a plan
                // that was not settled still owns the secret a later attempt may need.
                nonces.remove(&expiry.operation_id);
            }
            let stored = self
                .persistence
                .host_preparation_state()
                .map_err(|error| persistence_error(error, correlation_id))?;
            return stored.map(|state| state.status).ok_or_else(|| {
                envelope(
                    "sandbox.host_preparation_unavailable",
                    ErrorCategory::Unavailable,
                    "Host Preparation has not been probed.",
                    true,
                    correlation_id,
                )
            });
        }
        stored.map(|state| state.status).ok_or_else(|| {
            envelope(
                "sandbox.host_preparation_unavailable",
                ErrorCategory::Unavailable,
                "Host Preparation has not been probed.",
                true,
                correlation_id,
            )
        })
    }

    /// The explicit probe route: expensive by definition, so it runs on the worker.
    pub(super) fn probe_and_persist(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<HostPreparationStatus> {
        self.reconcile_expired_host_preparation(correlation_id)?;
        self.host_preparation.probe(correlation_id).map_err(|error| anyhow!("{error:?}"))?;
        self.persistence
            .host_preparation_state()?
            .map(|state| state.status)
            .ok_or_else(|| anyhow!("Host Preparation has not been probed"))
    }

    /// Settles an expired prepared plan from a path that already knows there is one.
    ///
    /// Used by the explicit probe and prepare routes, which are allowed to pay for the
    /// settlement synchronously; the ordinary status route uses the light pre-check
    /// instead so it never queues a write it does not need.
    pub(super) fn reconcile_expired_host_preparation(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<()> {
        let Some(expiry) = self.persistence.prepared_host_preparation_expiry()? else {
            return Ok(());
        };
        if expiry.expires_at >= UtcTimestamp::now() {
            return Ok(());
        }
        let settled =
            self.host_preparation.settle(expiry.operation_id, correlation_id).map_err(|error| {
                anyhow!("expired Host Preparation plan could not be settled: {error:?}")
            })?;
        if settled.is_some() {
            self.host_preparation_nonces
                .lock()
                .map_err(|_| anyhow!("Host Preparation nonce state is unavailable"))?
                .remove(&expiry.operation_id);
        }
        Ok(())
    }
}
