use super::classification::ControlResources;
use std::path::PathBuf;

impl ControlResources {
    pub(in crate::agent_execution) fn for_databases(
        persistence: &aura_persistence::PersistenceCoordinator,
        journal: &crate::broker::BrokerJournal,
    ) -> anyhow::Result<Self> {
        let registry = Self::default();
        for database in [persistence.database_path().to_path_buf(), journal.lock()?.database_path()]
        {
            registry.register(database.clone());
            for suffix in ["-wal", "-shm"] {
                let mut name = database.as_os_str().to_os_string();
                name.push(suffix);
                registry.register(PathBuf::from(name));
            }
        }
        registry.register(journal.lock()?.root().join("v2-development.json"));
        // The index and metadata-only audit files are Core-owned state. This
        // never registers the actual global/project Vault as a control path:
        // fs.write's ordinary external-path rule for the global Vault stays intact.
        std::fs::create_dir_all(&journal.memory().roots().cache)?;
        registry.register(journal.memory().roots().cache.clone());
        Ok(registry)
    }
    pub(crate) fn register_runtime(
        &self,
        paths: &aura_config::AuraPaths,
        release: &crate::CoreReleaseContext,
    ) {
        for module in aura_config::ConfigModule::ALL {
            self.register(paths.config_root.join(module.relative_path()));
        }
        for path in [
            &paths.config_history_dir,
            &paths.config_transaction_dir,
            &release.manifest_path,
            &release.elevated_helper_executable,
            &release.sandbox_host.executable,
            &release.egress_proxy.executable,
        ] {
            self.register(path.clone());
        }
        // This Core's data, and Aura's data of every
        // namespace, ask each time a model reaches into them. The global Memory Vault is not
        // among them (`%USERPROFILE%\.aura\memories`): it stays an ordinary external folder.
        for root in [&paths.config_root, &paths.local_root] {
            self.register_data_root(root.clone());
        }
        for base in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(base) {
                self.register_data_root(PathBuf::from(base).join("Aura"));
            }
        }
        // The running Core itself is an actual bound runtime resource, even
        // when the surrounding source checkout is a user's ordinary workspace.
        if let Ok(executable) = std::env::current_exe() {
            self.register(executable);
        }
        if let Ok(base) = aura_windows_platform::common_application_data_directory()
            && let Some(root) = aura_backend_windows_user::installation::discover_published(
                &base.join("Aura/R06-Spike"),
            )
        {
            self.register(root);
        }
    }
}
