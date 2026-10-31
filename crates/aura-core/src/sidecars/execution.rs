use super::*;

#[allow(
    dead_code,
    reason = "only the Sandbox Host lifecycle tests probe availability through this service"
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
}
