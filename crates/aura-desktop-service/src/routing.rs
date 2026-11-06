//! The two desktop connections own distinct hosts, catalogs, refs and input leases.
use super::*;
use crate::{
    ProgramHost,
    child::ChildHost,
    pip::{NativePip, PipSession},
};
use aura_desktop_domain::{ComputerMode, ComputerPipStatus};

impl DesktopControl {
    pub fn catalog_issue(&self) -> Option<String> {
        self.lock().catalog_issue.clone()
    }
    pub(super) async fn ensure_target(&self) -> Result<(), HostError> {
        let Some(pip) = &self.pip else { return Ok(()) };
        let settings = self.settings();
        if !settings.settings.enabled || settings.issue.is_some() {
            return Err(crate::pip::failure("电脑操控已停用；没有执行"));
        }
        pip.ensure(&settings.settings.pip_stop_keys).await?;
        let generation = pip.generation();
        let mut seen =
            self.pip_generation.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *seen != generation {
            let mut state = self.lock();
            state.catalog = None;
            state.catalog_issue = None;
            state.icons.clear();
            self.memory().clear();
            *seen = generation;
        }
        Ok(())
    }
    /// Assembly only: neither window nor host starts until the first request needs it.
    #[must_use]
    pub fn beside_core(settings: Arc<SettingsFacade>, folder: std::path::PathBuf) -> Arc<Self> {
        match ProgramHost::beside_core(folder.clone()) {
            Ok(host) => {
                let child = ChildHost::new();
                let pip = PipSession::new(
                    Arc::new(NativePip { program: host.program.clone(), folder }),
                    child.clone(),
                );
                Self::with_child(settings, Arc::new(host), Arc::new(child), pip)
            }
            Err(why) => Arc::new(Self::new(settings, Arc::new(crate::launch::Missing(why)))),
        }
    }
    #[must_use]
    pub fn with_child(
        settings: Arc<SettingsFacade>,
        foreground: Arc<dyn HostLauncher>,
        child: Arc<dyn HostLauncher>,
        pip: Arc<PipSession>,
    ) -> Arc<Self> {
        let mut child = Self::new(Arc::clone(&settings), child);
        child.mode = ComputerMode::ChildSession;
        child.pip = Some(pip);
        let root = Arc::new(Self::new(settings, foreground));
        let numbers = Arc::new(std::sync::atomic::AtomicU32::new(0));
        *root.memory() = crate::calls::Memory::with_numbers(Arc::clone(&numbers));
        *child.memory() = crate::calls::Memory::with_numbers(numbers);
        let _ = root.child.set(Arc::new(child));
        root
    }
    pub fn select(self: &Arc<Self>, mode: ComputerMode) -> Result<Arc<Self>, HostError> {
        match mode {
            ComputerMode::Foreground if self.mode == ComputerMode::Foreground => {
                Ok(Arc::clone(self))
            }
            ComputerMode::ChildSession if self.mode == ComputerMode::ChildSession => {
                Ok(Arc::clone(self))
            }
            ComputerMode::ChildSession => self
                .child
                .get()
                .cloned()
                .ok_or_else(|| crate::pip::failure("小窗连接不可用；没有执行")),
            _ => Err(crate::pip::failure("小窗不能切换到前台桌面；没有执行")),
        }
    }
    pub fn mode(&self) -> ComputerMode {
        self.mode
    }
    pub fn update_stop_keys(&self, keys: &str) {
        if let Some(pip) = &self.pip {
            pip.stop_keys(keys);
        }
        if let Some(child) = self.child.get() {
            child.update_stop_keys(keys);
        }
    }
    pub fn pip_status(&self) -> Option<ComputerPipStatus> {
        self.pip
            .as_ref()
            .map(|pip| pip.status())
            .or_else(|| self.child.get().and_then(|child| child.pip_status()))
    }
    pub async fn turn_ended(&self, session: SessionId, turn: TurnTaskId) {
        self.turn_ended_one(session, turn).await;
        if let Some(child) = self.child.get() {
            child.turn_ended_one(session, turn).await;
        }
    }
    pub async fn release(&self, session: SessionId) {
        self.release_one(session).await;
        if let Some(child) = self.child.get() {
            child.release_one(session).await;
        }
    }
    pub async fn stop_host(&self) {
        self.stop_host_one().await;
        if let Some(child) = self.child.get() {
            child.stop_host_one().await;
        }
    }
}
