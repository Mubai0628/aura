use super::*;

impl CommandExecution {
    pub fn candidates_for(&self, _mode: PermissionMode) -> Vec<Arc<dyn ExecutionBackend>> {
        vec![self.compat.clone()]
    }
    /// Verification already started at composition, off the reactor.
    pub async fn wait_until_ready(&self) -> anyhow::Result<()> {
        self.compat.wait_until_ready().await
    }
    pub async fn managed_runtime(
        &self,
    ) -> anyhow::Result<Arc<aura_backend_windows_user::compat::CompatBackend>> {
        self.compat.managed_runtime().await
    }
}
