//! Changing an installed plugin: import, trust (of the installed copy, or of a waiting update
//! that then replaces it), untrust, the switch, discarding an update and uninstalling. Folder
//! moves that must not be left half done are written to a small journal first; Core finishes or
//! rolls them back when it next starts. A plugin's servers stop whenever what it offers changes.
use super::import;
use super::runtime::{LocatedPlugin, PluginRuntime};
use aura_config::{PluginRegistry, SettingsFacadeError};
use aura_domain_common::{CorrelationId, CredentialId};
use aura_plugins::{
    PluginChangeRequest, PluginImportRequest, PluginPackage, PluginPendingSetting, PluginSecret,
    PluginSetEnabledRequest, PluginSetting, PluginTrustRequest, PluginTrustedParts,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const JOURNAL: &str = "journal.json";

/// How long moving an installed plugin's folder waits for its servers' programs to let go of
/// it: a stopped one is still exiting, or a tool listing started just before the stop runs to
/// its end.
const RELEASE_WAIT: Duration = Duration::from_secs(3);

#[derive(Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Journal {
    /// The installed copy moves to `previous` and the waiting copy takes its place.
    Update { name: String, previous: PathBuf },
    /// The plugin's folders moved to `folders` (installed copy first, then a waiting copy).
    /// Once `plugins.json` no longer names it, its credentials go.
    Uninstall {
        name: String,
        folders: Vec<PathBuf>,
        servers: Vec<String>,
        secrets: Vec<CredentialId>,
    },
}

fn busy(error: &std::io::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "plugins.folder_busy: 插件的文件夹正被其他程序使用（例如资源管理器或编辑器打开着它或里面的文件），关闭后再试；插件没有改变（{error}）"
    )
}

/// Moves the installed folder of a plugin whose servers were just stopped.
fn move_released(from: &Path, to: &Path) -> std::io::Result<()> {
    let deadline = Instant::now() + RELEASE_WAIT;
    loop {
        match std::fs::rename(from, to) {
            // Access denied or a sharing violation: a file in it is still open.
            Err(error)
                if matches!(error.raw_os_error(), Some(5 | 32)) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            moved => return moved,
        }
    }
}

fn remove_or_log(folder: &Path) {
    if let Err(error) = std::fs::remove_dir_all(folder) {
        // Hidden (`.removed-*`) and never listed: swept when Core next starts.
        crate::append_core_log(
            "plugins.cleanup_failed",
            serde_json::json!({"directory": folder.display().to_string(), "error": error.to_string()}),
        );
    }
}

pub(super) fn replaced(registry: &PluginRegistry, setting: PluginSetting) -> Vec<PluginSetting> {
    let mut plugins = registry.plugins.clone();
    plugins.retain(|plugin| plugin.name != setting.name);
    plugins.push(setting);
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

fn server_ids(package: Option<&PluginPackage>) -> Vec<String> {
    package.map(|p| p.servers.iter().map(|s| s.id.clone()).collect()).unwrap_or_default()
}

impl PluginRuntime {
    fn journal_path(&self) -> PathBuf {
        self.root().join(JOURNAL)
    }

    fn write_journal(&self, journal: &Journal) -> anyhow::Result<()> {
        std::fs::create_dir_all(self.root())?;
        let temporary = self.root().join(format!("{JOURNAL}.tmp"));
        std::fs::write(&temporary, serde_json::to_vec(journal)?)?;
        std::fs::rename(&temporary, self.journal_path())?;
        Ok(())
    }

    fn clear_journal(&self) {
        let _ = std::fs::remove_file(self.journal_path());
    }

    fn trash(&self) -> PathBuf {
        self.root().join(format!(".removed-{}", CorrelationId::new()))
    }

    pub(super) fn write_registry(
        &self,
        expected: u64,
        plugins: &[PluginSetting],
    ) -> anyhow::Result<()> {
        match self.settings().replace_plugin_registry(expected, plugins) {
            Ok(_) => Ok(()),
            Err(SettingsFacadeError::RevisionConflict { .. }) => {
                anyhow::bail!("plugins.revision_changed: 插件设置已被修改，请刷新后再操作")
            }
            Err(error) => Err(error.into()),
        }
    }

    /// The registry, unless it cannot be written to or changed since the user looked.
    fn current(&self, expected: u64) -> anyhow::Result<PluginRegistry> {
        let registry = self.settings().plugin_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "plugins.registry_unavailable: {}",
            registry.issue.clone().unwrap_or_default()
        );
        anyhow::ensure!(
            registry.revision == expected,
            "plugins.revision_changed: 插件设置已被修改，请刷新后再操作"
        );
        Ok(registry)
    }

    fn located(
        &self,
        name: &str,
        expected: u64,
    ) -> anyhow::Result<(PluginRegistry, LocatedPlugin)> {
        let (_, plugin) = self.locate(name)?;
        Ok((self.current(expected)?, plugin))
    }

    /// Stops every instance of the plugin's servers; they start again only as it now allows.
    pub(crate) fn stop_servers(&self, package: Option<&PluginPackage>) {
        if let Some(mcp) = &self.mcp {
            for server in server_ids(package) {
                mcp.stop_server(&server);
            }
        }
    }

    /// Lists the tools of the plugin's servers in the background, as trusting one of the user's
    /// own servers does, so the model is offered them without the MCP page being opened first.
    fn discover_servers(&self, package: &PluginPackage) {
        let Some(mcp) = &self.mcp else { return };
        // The servers are read from the ready plugins, which must see this change.
        self.changed();
        let ids = server_ids(Some(package));
        for server in mcp.configured().servers.into_iter().filter(|s| ids.contains(&s.id)) {
            if server.trusted_fingerprint.is_some() {
                mcp.discover_later(server);
            }
        }
    }

    fn forget(&self, servers: &[String], secrets: &[CredentialId]) {
        let Some(mcp) = &self.mcp else { return };
        for reason in mcp.forget_servers(servers, secrets) {
            crate::append_core_log(
                "plugins.credential_cleanup_failed",
                serde_json::json!({"reason": reason}),
            );
        }
    }

    /// Records what is trusted with the plugin: its fingerprint, each part's own fingerprint and
    /// a credential for every server secret. Returns the credentials no longer named.
    fn trust_parts(
        &self,
        package: &PluginPackage,
        setting: &mut PluginSetting,
    ) -> Vec<CredentialId> {
        let mut kept = Vec::new();
        for server in &package.servers {
            for key in server.secret_keys() {
                let secret_id = setting.secret(&server.id, &key).unwrap_or_default();
                kept.push(PluginSecret { server: server.id.clone(), key, secret_id });
            }
        }
        let gone = setting
            .secrets
            .iter()
            .filter(|old| !kept.iter().any(|new| new.secret_id == old.secret_id))
            .map(|old| old.secret_id)
            .collect();
        setting.secrets = kept;
        let mut parts = PluginTrustedParts::default();
        for found in &package.skills {
            if let Ok(skill) = &found.scanned {
                parts.skills.insert(found.relative.clone(), skill.fingerprint.clone());
            }
        }
        for plugin in &package.commands {
            if plugin.command.issue.is_none()
                && let Some(revision) = &plugin.command.revision
            {
                parts.commands.insert(plugin.command.name.clone(), revision.clone());
            }
        }
        for hook in &package.hooks {
            if let Ok(hook_package) = &hook.package {
                parts.hooks.insert(hook.folder.clone(), hook_package.fingerprint.clone());
            }
        }
        if let Some(mcp) = &self.mcp {
            for server in package.servers.iter().filter(|server| server.problems.is_empty()) {
                let config =
                    server.config(&|key| setting.secret(&server.id, key).unwrap_or_default());
                if let Ok(fingerprint) = mcp.current_fingerprint(&config) {
                    parts.servers.insert(server.id.clone(), fingerprint);
                }
            }
        }
        setting.trusted_fingerprint = Some(package.fingerprint.clone());
        setting.trusted_parts = parts;
        gone
    }

    /// Copies a plugin in. A new name is installed (switched on, not trusted); a trusted plugin's
    /// name gets the copy as its waiting update, and the installed copy keeps working.
    pub(crate) fn import(&self, request: &PluginImportRequest) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let registry = self.current(request.expected_revision)?;
        let staged = import::stage(self, Path::new(&request.path))?;
        let placed = self.place(&registry, &staged.folder, &staged.package, &request.path);
        let _ = std::fs::remove_dir_all(&staged.staging);
        placed
    }

    fn place(
        &self,
        registry: &PluginRegistry,
        staged: &Path,
        package: &PluginPackage,
        source: &str,
    ) -> anyhow::Result<()> {
        let name = &package.manifest.name;
        let folder = self.folder(name);
        let saved = registry.plugin(name).cloned();
        if let Some(mut setting) = saved.clone().filter(|s| s.trusted_fingerprint.is_some())
            && folder.exists()
        {
            let pending = self.pending_folder(name);
            std::fs::create_dir_all(pending.parent().unwrap_or(&self.root()))?;
            if pending.exists() {
                let trash = self.trash();
                std::fs::rename(&pending, &trash).map_err(|error| busy(&error))?;
                remove_or_log(&trash);
            }
            std::fs::rename(staged, &pending)?;
            setting.pending = Some(PluginPendingSetting { imported_from: Some(source.to_owned()) });
            if let Err(error) = self.write_registry(registry.revision, &replaced(registry, setting))
            {
                let _ = std::fs::remove_dir_all(&pending);
                return Err(error);
            }
            return Ok(());
        }
        // A new plugin, or one never trusted or without its folder: installed as imported.
        if folder.exists() {
            let trash = self.trash();
            std::fs::rename(&folder, &trash).map_err(|error| busy(&error))?;
            remove_or_log(&trash);
        }
        std::fs::rename(staged, &folder)?;
        let mut setting = saved.unwrap_or_else(|| PluginSetting::imported(name, None));
        setting.enabled = true;
        setting.imported_from = Some(source.to_owned());
        setting.trusted_fingerprint = None;
        setting.trusted_parts = PluginTrustedParts::default();
        setting.pending = None;
        if let Err(error) = self.write_registry(registry.revision, &replaced(registry, setting)) {
            let _ = std::fs::remove_dir_all(&folder);
            return Err(error);
        }
        Ok(())
    }

    /// Trusts exactly the content the user saw, and switches the plugin on.
    pub(crate) fn trust(&self, request: &PluginTrustRequest) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let (registry, located) = self.located(&request.name, request.expected_revision)?;
        if request.pending {
            return self.apply_update(&registry, &located, &request.expected_fingerprint);
        }
        let package = located
            .scanned
            .as_ref()
            .map_err(|error| anyhow::anyhow!("plugins.invalid: {error}"))?;
        anyhow::ensure!(
            package.fingerprint == request.expected_fingerprint,
            "plugins.content_changed: 插件的内容在你查看之后变了，请重新查看再信任"
        );
        let mut setting =
            located.setting.clone().unwrap_or_else(|| PluginSetting::imported(&located.name, None));
        let trusted_before = setting.trusted_parts.servers.clone();
        let gone = self.trust_parts(package, &mut setting);
        self.settle_changes(package, &mut setting, &trusted_before);
        setting.enabled = true;
        self.stop_servers(Some(package));
        self.write_registry(registry.revision, &replaced(&registry, setting))?;
        self.forget(&[], &gone);
        self.discover_servers(package);
        Ok(())
    }

    /// The waiting copy replaces the installed one and is trusted; on any failure the installed
    /// copy stays as it was.
    fn apply_update(
        &self,
        registry: &PluginRegistry,
        located: &LocatedPlugin,
        expected: &str,
    ) -> anyhow::Result<()> {
        let name = &located.name;
        let pending = self.pending_folder(name);
        let waiting =
            self.scan(&pending).map_err(|error| anyhow::anyhow!("plugins.invalid: {error}"))?;
        anyhow::ensure!(
            waiting.fingerprint == expected,
            "plugins.content_changed: 新版本的内容在你查看之后变了，请重新查看再信任"
        );
        let folder = self.folder(name);
        let previous = self.trash();
        self.write_journal(&Journal::Update { name: name.clone(), previous: previous.clone() })?;
        self.stop_servers(located.scanned.as_ref().ok());
        let had_folder = folder.exists();
        if had_folder && let Err(error) = move_released(&folder, &previous) {
            self.clear_journal();
            return Err(busy(&error));
        }
        let restore = |error: anyhow::Error| {
            let _ = std::fs::rename(&folder, &pending);
            if had_folder {
                let _ = std::fs::rename(&previous, &folder);
            }
            self.clear_journal();
            error
        };
        if let Err(error) = std::fs::rename(&pending, &folder) {
            return Err(restore(busy(&error)));
        }
        let package = match self.scan(&folder) {
            Ok(package) if package.fingerprint == expected => package,
            Ok(_) => {
                return Err(restore(anyhow::anyhow!(
                    "plugins.content_changed: 新版本的内容在你查看之后变了，请重新查看再信任"
                )));
            }
            Err(error) => return Err(restore(anyhow::anyhow!("plugins.invalid: {error}"))),
        };
        let mut setting =
            located.setting.clone().unwrap_or_else(|| PluginSetting::imported(name, None));
        if let Some(update) = setting.pending.take() {
            setting.imported_from = update.imported_from;
        }
        let trusted_before = setting.trusted_parts.servers.clone();
        let gone_secrets = self.trust_parts(&package, &mut setting);
        self.settle_changes(&package, &mut setting, &trusted_before);
        setting.enabled = true;
        if let Err(error) = self.write_registry(registry.revision, &replaced(registry, setting)) {
            return Err(restore(error));
        }
        if had_folder {
            remove_or_log(&previous);
        }
        let now = server_ids(Some(&package));
        let gone_servers: Vec<String> = server_ids(located.scanned.as_ref().ok())
            .into_iter()
            .filter(|server| !now.contains(server))
            .collect();
        self.forget(&gone_servers, &gone_secrets);
        self.clear_journal();
        self.discover_servers(&package);
        Ok(())
    }

    /// Records a plugin server's trust at `fingerprint`: the user trusted it again on the MCP
    /// page after what it runs changed (an updated `node`, say).
    pub(crate) fn trust_server(
        &self,
        plugin: &str,
        server: &str,
        fingerprint: String,
    ) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let registry = self.settings().plugin_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "plugins.registry_unavailable: {}",
            registry.issue.clone().unwrap_or_default()
        );
        let mut setting = registry
            .plugin(plugin)
            .cloned()
            .filter(|setting| setting.trusted_fingerprint.is_some())
            .ok_or_else(|| anyhow::anyhow!("plugins.not_trusted: 插件 {plugin} 还没有信任"))?;
        setting.trusted_parts.servers.insert(server.to_owned(), fingerprint);
        self.write_registry(registry.revision, &replaced(&registry, setting))?;
        self.changed();
        Ok(())
    }

    /// Nothing the plugin brings is offered until it is trusted again; its secrets are kept.
    pub(crate) fn untrust(&self, request: &PluginChangeRequest) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let (registry, located) = self.located(&request.name, request.expected_revision)?;
        let mut setting = located
            .setting
            .clone()
            .ok_or_else(|| anyhow::anyhow!("plugins.not_trusted: 这个插件还没有信任"))?;
        setting.trusted_fingerprint = None;
        setting.trusted_parts = PluginTrustedParts::default();
        self.stop_servers(located.scanned.as_ref().ok());
        self.write_registry(registry.revision, &replaced(&registry, setting))
    }

    pub(crate) fn set_enabled(&self, request: &PluginSetEnabledRequest) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let (registry, located) = self.located(&request.name, request.expected_revision)?;
        let mut setting =
            located.setting.clone().unwrap_or_else(|| PluginSetting::imported(&located.name, None));
        setting.enabled = request.enabled;
        self.stop_servers(located.scanned.as_ref().ok());
        self.write_registry(registry.revision, &replaced(&registry, setting))?;
        if let (true, Ok(package)) = (request.enabled, &located.scanned) {
            self.discover_servers(package);
        }
        Ok(())
    }

    /// Removes the waiting copy; the installed one is unchanged.
    pub(crate) fn discard_update(&self, request: &PluginChangeRequest) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let (registry, located) = self.located(&request.name, request.expected_revision)?;
        let pending = self.pending_folder(&located.name);
        let saved = located.setting.clone().filter(|setting| setting.pending.is_some());
        anyhow::ensure!(
            pending.exists() || saved.is_some(),
            "plugins.no_update: 没有等待信任的新版本"
        );
        if pending.exists() {
            let trash = self.trash();
            std::fs::rename(&pending, &trash).map_err(|error| busy(&error))?;
            remove_or_log(&trash);
        }
        if let Some(mut setting) = saved {
            setting.pending = None;
            self.write_registry(registry.revision, &replaced(&registry, setting))?;
        }
        Ok(())
    }

    /// Stops its servers, removes it from `plugins.json`, its folders, its servers' secrets and
    /// sign-ins. A folder another program holds stops the uninstall before anything changes.
    pub(crate) fn uninstall(&self, request: &PluginChangeRequest) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let (registry, located) = self.located(&request.name, request.expected_revision)?;
        let name = located.name.clone();
        let servers = server_ids(located.scanned.as_ref().ok());
        let secrets: Vec<CredentialId> = located
            .setting
            .iter()
            .flat_map(|setting| setting.secrets.iter().map(|secret| secret.secret_id))
            .collect();
        let (folder, pending) = (self.folder(&name), self.pending_folder(&name));
        let (installed, waiting) = (self.trash(), self.trash());
        self.write_journal(&Journal::Uninstall {
            name: name.clone(),
            folders: vec![installed.clone(), waiting.clone()],
            servers: servers.clone(),
            secrets: secrets.clone(),
        })?;
        self.stop_servers(located.scanned.as_ref().ok());
        let moved = folder.exists();
        if moved && let Err(error) = move_released(&folder, &installed) {
            self.clear_journal();
            return Err(busy(&error));
        }
        let moved_pending = pending.exists() && std::fs::rename(&pending, &waiting).is_ok();
        let mut plugins = registry.plugins.clone();
        plugins.retain(|plugin| plugin.name != name);
        if registry.plugin(&name).is_some()
            && let Err(error) = self.write_registry(registry.revision, &plugins)
        {
            if moved {
                let _ = std::fs::rename(&installed, &folder);
            }
            if moved_pending {
                let _ = std::fs::rename(&waiting, &pending);
            }
            self.clear_journal();
            return Err(error);
        }
        self.forget(&servers, &secrets);
        for trash in [installed, waiting] {
            if trash.exists() {
                remove_or_log(&trash);
            }
        }
        self.clear_journal();
        Ok(())
    }

    /// When Core starts: finishes or rolls back a change a crash interrupted, then removes
    /// staging and removed leftovers.
    pub(crate) fn recover(&self) {
        if let Ok(bytes) = std::fs::read(self.journal_path()) {
            match serde_json::from_slice::<Journal>(&bytes) {
                Ok(Journal::Update { name, previous }) => {
                    let folder = self.folder(&name);
                    if !folder.exists() && previous.exists() {
                        let _ = std::fs::rename(&previous, &folder);
                    }
                }
                Ok(Journal::Uninstall { name, folders, servers, secrets }) => {
                    let registry = self.settings().plugin_registry();
                    if registry.issue.is_none() && registry.plugin(&name).is_none() {
                        self.forget(&servers, &secrets);
                    } else {
                        // Not done: what moved goes back.
                        let places = [self.folder(&name), self.pending_folder(&name)];
                        for (moved, place) in folders.iter().zip(places) {
                            if moved.exists() && !place.exists() {
                                let _ = std::fs::rename(moved, place);
                            }
                        }
                    }
                }
                Err(error) => crate::append_core_log(
                    "plugins.journal_unreadable",
                    serde_json::json!({"error": error.to_string()}),
                ),
            }
            self.clear_journal();
        }
        let Ok(entries) = std::fs::read_dir(self.root()) else { return };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".staging-") || name.starts_with(".removed-") {
                remove_or_log(&entry.path());
            }
        }
    }
}
