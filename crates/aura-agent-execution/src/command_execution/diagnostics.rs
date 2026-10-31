use super::port::CorePort;
use aura_execution_contract::DispatchRequest;

impl CorePort {
    pub(super) fn record_filtered_names(
        &self,
        request: &DispatchRequest,
        names: &[String],
    ) -> anyhow::Result<()> {
        // There is deliberately no API accepting values at this publication seam.
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_v2::execution::shell::append(
                tx,
                request,
                "shell.environment_filtered",
                &serde_json::json!({"names": names}),
                aura_domain_common::UtcTimestamp::now(),
            )
        })?;
        Ok(())
    }
}
