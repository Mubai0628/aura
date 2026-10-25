//! ExecutionBackend adapter: the port remains the authority and durable Result owner.
use super::{runtime::RuntimeCheck, soft_proxy::SoftProxy};
use crate::{WindowsUserCorePort, controller::ActiveMap};
use aura_domain_common::RuntimeInstanceId;
use std::{path::Path, sync::Arc};

pub const COMPAT_BACKEND_ID: &str = "windows-user-compat-v1";
pub struct CompatExecutionBackend {
    pub(super) runtime: RuntimeInstanceId,
    pub(super) check: RuntimeCheck,
    pub(super) proxy: Arc<SoftProxy>,
    pub(super) port: Arc<dyn WindowsUserCorePort>,
    pub(crate) active: ActiveMap,
    pub terminals: Arc<super::TerminalRegistry>,
}
impl CompatExecutionBackend {
    pub fn new(
        root: &Path,
        runtime: RuntimeInstanceId,
        port: Arc<dyn WindowsUserCorePort>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            runtime,
            check: RuntimeCheck::start(root),
            proxy: Arc::new(SoftProxy::open()?),
            port,
            active: Default::default(),
            terminals: Default::default(),
        })
    }
    pub async fn wait_until_ready(&self) -> anyhow::Result<()> {
        self.check.wait().await.map(drop)
    }
    /// The same verified managed runtime the Shell uses. A package PowerShell tool
    /// borrows it so every pinned file stays held for the whole call.
    pub async fn managed_runtime(&self) -> anyhow::Result<Arc<super::CompatBackend>> {
        self.check.wait().await
    }
    pub fn is_runtime_check_pending(&self) -> bool {
        self.check.get().is_none()
    }
    pub async fn open_terminal(
        &self,
        session: aura_domain_common::SessionId,
        options: super::TerminalOptions,
    ) -> anyhow::Result<super::TerminalSummary> {
        let backend = self.check.wait().await?;
        let registry = Arc::clone(&self.terminals);
        let proxy = Arc::clone(&self.proxy);
        tokio::task::spawn_blocking(move || {
            registry.open(session, || backend.launch_terminal(&options, Some(&proxy.endpoint)))
        })
        .await
        .map_err(|_| anyhow::anyhow!("terminal.launch_worker_lost"))?
    }
    /// User opening only: cancellation remains possible through pool and registry-lock waiting.
    pub async fn open_terminal_cancellable(
        &self,
        session: aura_domain_common::SessionId,
        options: super::TerminalOptions,
        mut cancelled: tokio::sync::watch::Receiver<bool>,
        on_runtime_ready: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
        before_launch: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> anyhow::Result<super::TerminalSummary> {
        let backend = self.check.wait_with_cancel(cancelled.clone()).await?;
        on_runtime_ready()?;
        #[cfg(feature = "terminal-test-profile")]
        super::terminal_opening_gate::wait(session, cancelled.clone()).await?;
        let registry = Arc::clone(&self.terminals);
        let proxy = Arc::clone(&self.proxy);
        let mut launched = tokio::task::spawn_blocking(move || {
            // The gate runs after the registry's lifecycle lock, immediately before
            // launch. Pool/lifecycle waiting must never move an opening to Launching.
            registry.open(session, || {
                before_launch()?;
                backend.launch_terminal(&options, Some(&proxy.endpoint))
            })
        });
        tokio::select! {
            biased;
            _ = cancelled.wait_for(|value| *value) => {
                launched.abort(); // Removes queued work; a running closure still checks its gate.
                anyhow::bail!("terminal.open_cancelled")
            }
            result = &mut launched => result.map_err(|_| anyhow::anyhow!("terminal.launch_worker_lost"))?,
        }
    }
}
