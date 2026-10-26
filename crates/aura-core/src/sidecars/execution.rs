use super::*;

#[allow(
    dead_code,
    reason = "the W7 authorization-bound adapter is committed before its gated fixture-runtime wiring"
)]
// UNWIRED: no production caller dispatches work to the sandbox host; only tests reach this.
impl ToolSandboxExecutionService {
    /// Final pre-dispatch liveness check. A P04 Allow never substitutes for an
    /// available Sandbox execution path, and failure here occurs before `tea_`
    /// is allocated.
    pub(crate) async fn ensure_available(
        &self,
    ) -> std::result::Result<(), ToolSandboxExecutionError> {
        self.sandbox.ping().await.map_err(ToolSandboxExecutionError::Unavailable)
    }

    /// Acquired while a ToolCall is still waiting for Runtime capacity. The
    /// caller must hold this permit before creating `tea_`/DispatchPrepared.
    pub(crate) async fn acquire_host_capacity(
        &self,
    ) -> std::result::Result<ToolSandboxHostPermit, ToolSandboxExecutionError> {
        let permit = self.host_capacity.clone().acquire_owned().await.map_err(|_| {
            ToolSandboxExecutionError::Unavailable(anyhow::anyhow!(
                "Sandbox Host capacity was closed"
            ))
        })?;
        Ok(ToolSandboxHostPermit { _permit: permit })
    }

    #[cfg(test)]
    pub(crate) fn observe_backend_entries(
        &mut self,
        entries: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) {
        self.backend_entries = Some(entries);
    }

    pub(crate) async fn execute_authorized_tool(
        &self,
        _host_permit: ToolSandboxHostPermit,
        execution: AuthorizedToolSandboxExecutionV1,
        secret: Option<ToolRuntimeSecretFrameV1>,
    ) -> std::result::Result<ExternalProcessExecutionOutcome, ToolSandboxExecutionError> {
        #[cfg(test)]
        if let Some(entries) = &self.backend_entries {
            entries.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        self.sandbox
            .request_with_tool_runtime_secret(
                method::SANDBOX_EXECUTE_AUTHORIZED_TOOL,
                &execution,
                secret,
            )
            .await
            .map_err(|error| match error.downcast::<SidecarRemoteError>() {
                Ok(remote) => ToolSandboxExecutionError::Rejected(remote.envelope),
                Err(error) => ToolSandboxExecutionError::Unavailable(error),
            })
    }

    pub(crate) async fn execute_authorized_tool_conformance(
        &self,
        _host_permit: ToolSandboxHostPermit,
        execution: AuthorizedToolConformanceSandboxExecutionV1,
        secret: Option<ToolConformanceRuntimeSecretFrameV1>,
    ) -> std::result::Result<ExternalProcessExecutionOutcome, ToolSandboxExecutionError> {
        self.sandbox
            .request_with_tool_conformance_runtime_secret(
                method::SANDBOX_EXECUTE_AUTHORIZED_TOOL_CONFORMANCE,
                &execution,
                secret,
            )
            .await
            .map_err(|error| match error.downcast::<SidecarRemoteError>() {
                Ok(remote) => ToolSandboxExecutionError::Rejected(remote.envelope),
                Err(error) => ToolSandboxExecutionError::Unavailable(error),
            })
    }

    pub(crate) async fn send_stdio_input(
        &self,
        input: &ToolStdioHostInputV1,
    ) -> std::result::Result<(), ToolSandboxExecutionError> {
        let response: Value =
            self.sandbox.request(method::SANDBOX_TOOL_STDIO_INPUT, input).await.map_err(
                |error| match error.downcast::<SidecarRemoteError>() {
                    Ok(remote) => ToolSandboxExecutionError::Rejected(remote.envelope),
                    Err(error) => ToolSandboxExecutionError::Unavailable(error),
                },
            )?;
        if response.get("accepted").and_then(Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err(ToolSandboxExecutionError::Unavailable(anyhow::anyhow!(
                "Sandbox Host returned an invalid Aura Stdio acknowledgement"
            )))
        }
    }

    pub(crate) async fn cancel(
        &self,
        cancellation_id: aura_domain_common::CancellationId,
    ) -> std::result::Result<bool, ToolSandboxExecutionError> {
        let response: Value = self
            .sandbox
            .request(method::SANDBOX_CANCEL, &CancelSandboxExecutionInput { cancellation_id })
            .await
            .map_err(|error| match error.downcast::<SidecarRemoteError>() {
                Ok(remote) => ToolSandboxExecutionError::Rejected(remote.envelope),
                Err(error) => ToolSandboxExecutionError::Unavailable(error),
            })?;
        response.get("cancelled").and_then(Value::as_bool).ok_or_else(|| {
            ToolSandboxExecutionError::Unavailable(anyhow::anyhow!(
                "Sandbox Host returned an invalid cancellation acknowledgement"
            ))
        })
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<Value> {
        self.sandbox._events.subscribe()
    }
}
