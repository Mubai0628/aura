//! Aura's plugin folder and what each installed plugin is under the choices in `plugins.json`.
//! The folders are the source: a plugin is a folder in `plugins\packages` named by the plugin,
//! and a newer copy waiting to be trusted is the same name in `plugins\pending`.
use aura_config::{PluginRegistry, SettingsFacade};
use aura_plugins::{
    PluginCounts, PluginLimits, PluginPackage, PluginPackageError, PluginPendingView,
    PluginRegistryView, PluginSetting, PluginState, PluginView, scan_plugin,
};
use aura_skills::FileHashCache;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Under Aura's local data root.
pub(crate) const PLUGINS: &str = "plugins";
const LIBRARY: &str = "packages";
const PENDING: &str = "pending";
/// How long the ready plugins are reused. Each part a plugin brings is checked again against
/// its own trusted fingerprint where it is used, so a change in this window is still refused.
const READY_FOR: Duration = Duration::from_secs(2);

/// A trusted, switched-on plugin whose content is still exactly what was trusted.
pub(crate) struct ReadyPlugin {
    pub package: PluginPackage,
    pub setting: PluginSetting,
}

impl ReadyPlugin {
    /// What each process tree the plugin starts may use.
    pub(crate) fn job_limits(&self) -> aura_windows_platform::JobLimits {
        let limits = self.package.manifest.limits;
        aura_windows_platform::JobLimits {
            memory_bytes: u64::from(limits.memory_mb) * 1024 * 1024,
            active_processes: limits.processes,
        }
    }
}

/// One installed plugin as found now.
pub(crate) struct LocatedPlugin {
    pub name: String,
    pub folder: PathBuf,
    pub setting: Option<PluginSetting>,
    pub scanned: Result<PluginPackage, PluginPackageError>,
    pub state: PluginState,
}

pub(crate) struct PluginRuntime {
    settings: Arc<SettingsFacade>,
    local_root: PathBuf,
    pub(crate) hashes: FileHashCache,
    pub(crate) mcp: Option<Arc<crate::mcp::McpRuntime>>,
    /// One change at a time (import, trust, update, uninstall), so folder moves never interleave.
    pub(crate) changing: Mutex<()>,
    ready_cache: Mutex<Option<(Instant, Arc<Vec<ReadyPlugin>>)>>,
}

impl PluginRuntime {
    pub(crate) fn new(
        settings: Arc<SettingsFacade>,
        mcp: Option<Arc<crate::mcp::McpRuntime>>,
    ) -> Arc<Self> {
        let local_root = settings.paths().local_root.clone();
        let runtime = Arc::new(Self {
            settings,
            local_root,
            hashes: FileHashCache::default(),
            mcp,
            changing: Mutex::new(()),
            ready_cache: Mutex::new(None),
        });
        runtime.recover();
        runtime
    }

    pub(crate) fn settings(&self) -> &SettingsFacade {
        &self.settings
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.local_root.join(PLUGINS)
    }

    pub(crate) fn library(&self) -> PathBuf {
        self.root().join(LIBRARY)
    }

    pub(crate) fn folder(&self, name: &str) -> PathBuf {
        self.library().join(name)
    }

    pub(crate) fn pending_folder(&self, name: &str) -> PathBuf {
        self.root().join(PENDING).join(name)
    }

    /// Reads the plugin in `folder` and holds it to be the plugin named by the folder.
    pub(crate) fn scan(&self, folder: &Path) -> Result<PluginPackage, PluginPackageError> {
        let package = scan_plugin(folder, &self.hashes)?;
        let named = folder.file_name().map(|name| name.to_string_lossy().into_owned());
        if named.as_deref() != Some(package.manifest.name.as_str()) {
            return Err(PluginPackageError::Invalid(vec![format!(
                "文件夹名 {} 与插件名 {} 不同",
                named.unwrap_or_default(),
                package.manifest.name
            )]));
        }
        Ok(package)
    }

    /// Every installed plugin (every folder in the plugin folder, and every saved plugin whose
    /// folder is gone), by name.
    pub(crate) fn locate_all(&self) -> (PluginRegistry, Vec<LocatedPlugin>) {
        let registry = self.settings.plugin_registry();
        let mut names: Vec<String> = std::fs::read_dir(self.library())
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| !name.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default();
        names.extend(registry.plugins.iter().map(|plugin| plugin.name.clone()));
        names.sort();
        names.dedup();
        let located = names
            .into_iter()
            .map(|name| {
                let folder = self.folder(&name);
                let setting = registry.plugin(&name).cloned();
                let scanned = self.scan(&folder);
                let state = state(&scanned, setting.as_ref());
                LocatedPlugin { name, folder, setting, scanned, state }
            })
            .collect();
        (registry, located)
    }

    pub(crate) fn locate(&self, name: &str) -> anyhow::Result<(PluginRegistry, LocatedPlugin)> {
        let (registry, located) = self.locate_all();
        let plugin = located
            .into_iter()
            .find(|plugin| plugin.name == name)
            .ok_or_else(|| anyhow::anyhow!("plugins.not_found: 没有名为 {name} 的插件，请刷新"))?;
        Ok((registry, plugin))
    }

    pub(crate) fn view(&self, plugin: &LocatedPlugin) -> PluginView {
        let enabled = plugin.setting.as_ref().is_none_or(|setting| setting.enabled);
        let mut view = PluginView {
            name: plugin.name.clone(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            layout: None,
            state: plugin.state,
            enabled,
            path: aura_domain_common::display_path(&plugin.folder),
            fingerprint: None,
            trusted_fingerprint: plugin
                .setting
                .as_ref()
                .and_then(|s| s.trusted_fingerprint.clone()),
            limits: PluginLimits::default(),
            counts: PluginCounts::default(),
            problems: Vec::new(),
            warnings: Vec::new(),
            unsupported: Vec::new(),
            pending_update: self.pending_view(plugin),
            file_count: 0,
            bytes: 0,
        };
        match &plugin.scanned {
            Ok(package) => {
                view.version = Some(package.manifest.version.clone());
                view.description = Some(package.manifest.description.clone());
                view.author.clone_from(&package.manifest.author);
                view.homepage.clone_from(&package.manifest.homepage);
                view.layout = Some(package.layout);
                view.fingerprint = Some(package.fingerprint.clone());
                view.limits = package.manifest.limits;
                view.counts = package.counts();
                view.warnings.clone_from(&package.warnings);
                let user = self.settings.mcp_registry().servers;
                for server in &package.servers {
                    if user.iter().any(|mine| mine.id == server.id) {
                        view.warnings.push(format!(
                            "MCP 服务器 {} 与你自己配置的服务器同名，插件里的这个不会启用",
                            server.id
                        ));
                    }
                }
                view.unsupported.clone_from(&package.unsupported);
                view.file_count = u32::try_from(package.files.len()).unwrap_or(u32::MAX);
                view.bytes = package.bytes;
            }
            Err(error) => view.problems = problems(error),
        }
        view
    }

    fn pending_view(&self, plugin: &LocatedPlugin) -> Option<PluginPendingView> {
        let folder = self.pending_folder(&plugin.name);
        if !folder.is_dir() {
            return None;
        }
        Some(match self.scan(&folder) {
            Ok(package) => PluginPendingView {
                version: Some(package.manifest.version.clone()),
                fingerprint: Some(package.fingerprint.clone()),
                counts: package.counts(),
                problems: Vec::new(),
                changed_servers: super::changes::changed_servers(
                    plugin.scanned.as_ref().ok(),
                    &package,
                    plugin.setting.as_ref(),
                ),
            },
            Err(error) => PluginPendingView {
                version: None,
                fingerprint: None,
                counts: PluginCounts::default(),
                problems: problems(&error),
                changed_servers: Vec::new(),
            },
        })
    }

    /// The ready plugins, by name. Nothing is ready while `plugins.json` cannot be used.
    pub(crate) fn ready(&self) -> Arc<Vec<ReadyPlugin>> {
        if let Ok(cache) = self.ready_cache.lock()
            && let Some((at, ready)) = cache.as_ref()
            && at.elapsed() < READY_FOR
        {
            return Arc::clone(ready);
        }
        let (registry, located) = self.locate_all();
        let ready: Vec<ReadyPlugin> = if registry.issue.is_some() {
            Vec::new()
        } else {
            located
                .into_iter()
                .filter(|plugin| plugin.state == PluginState::Ready)
                .filter_map(|plugin| {
                    Some(ReadyPlugin { package: plugin.scanned.ok()?, setting: plugin.setting? })
                })
                .collect()
        };
        let ready = Arc::new(ready);
        if let Ok(mut cache) = self.ready_cache.lock() {
            *cache = Some((Instant::now(), Arc::clone(&ready)));
        }
        ready
    }

    /// Something changed here: the next reader reads the plugins again.
    pub(crate) fn changed(&self) {
        if let Ok(mut cache) = self.ready_cache.lock() {
            *cache = None;
        }
    }

    pub(crate) fn registry_view(&self) -> PluginRegistryView {
        let (registry, located) = self.locate_all();
        PluginRegistryView {
            revision: registry.revision,
            plugins: located.iter().map(|plugin| self.view(plugin)).collect(),
            library: aura_domain_common::display_path(&self.library()),
            issue: registry.issue,
        }
    }
}

pub(crate) fn problems(error: &PluginPackageError) -> Vec<String> {
    match error {
        PluginPackageError::Invalid(problems) => problems.clone(),
        other => vec![other.to_string()],
    }
}

/// What a plugin is under its saved choices.
pub(crate) fn state(
    scanned: &Result<PluginPackage, PluginPackageError>,
    setting: Option<&PluginSetting>,
) -> PluginState {
    match scanned {
        Err(PluginPackageError::NotAFolder | PluginPackageError::Io(_)) => PluginState::Unavailable,
        Err(_) => PluginState::Invalid,
        Ok(package) => match setting.and_then(|s| s.trusted_fingerprint.as_ref().map(|t| (s, t))) {
            None => PluginState::NotTrusted,
            Some((_, trusted)) if *trusted != package.fingerprint => PluginState::ContentChanged,
            Some((setting, _)) if !setting.enabled => PluginState::Disabled,
            Some(_) => PluginState::Ready,
        },
    }
}
