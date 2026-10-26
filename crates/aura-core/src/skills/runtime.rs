//! Where Skill folders are, and what each one is under the user's choices. Aura's own folder,
//! the user's `.agents\skills` and the selected workspace's `.agents\skills` are read; every
//! folder on the way must be a real folder. A ready plugin's Skills follow the plugin.
use aura_config::{SettingsFacade, SkillRegistry};
use aura_domain_common::{SkillInstanceId, WorkspaceId};
use aura_skills::{
    FileHashCache, FoundSkill, RootStatus, SkillRootView, SkillSetting, SkillSource, SkillView,
    root_status, scan_root,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Under a user profile or a workspace.
pub(crate) const AGENTS_SKILLS: &str = ".agents/skills";
/// Under Aura's local data root.
pub(crate) const STORE_SKILLS: &str = "skills/packages";

/// One Skill as found now, with the saved choices it is shown under.
#[derive(Clone, Debug)]
pub(crate) struct LocatedSkill {
    pub view: SkillView,
    pub found: FoundSkill,
    pub setting: Option<SkillSetting>,
}

/// One folder a listing covers, and whether it can be read.
struct Root {
    source: SkillSource,
    workspace: Option<WorkspaceId>,
    path: PathBuf,
    status: RootStatus,
}

pub(crate) struct SkillRuntime {
    settings: Arc<SettingsFacade>,
    local_root: PathBuf,
    /// The folder holding the user's `.agents`: the user profile, or a TEMP stand-in in tests.
    profile: Option<PathBuf>,
    workspace: Option<Arc<aura_workspace::WorkspaceRuntime>>,
    pub(crate) hashes: FileHashCache,
    plugins: std::sync::OnceLock<Arc<crate::plugins::PluginRuntime>>,
    /// Held while the start's first reading of the Skill folders runs; a listing waits for
    /// it rather than reading the same files beside it.
    pub(super) warming: std::sync::Mutex<()>,
}

/// The user profile, except in tests: unit tests use a folder under their TEMP data root, and
/// an E2E build reads only `AURA_USER_PROFILE_TEST_ROOT` inside TEMP.
fn profile(local_root: &Path) -> Option<PathBuf> {
    #[cfg(test)]
    {
        Some(local_root.join("test-profile"))
    }
    #[cfg(all(not(test), feature = "memory-test-profile"))]
    {
        let _ = local_root;
        let root = PathBuf::from(std::env::var_os("AURA_USER_PROFILE_TEST_ROOT")?);
        let temp = std::env::temp_dir();
        (root.is_absolute() && root.starts_with(&temp) && root != temp).then_some(root)
    }
    #[cfg(all(not(test), not(feature = "memory-test-profile")))]
    {
        let _ = local_root;
        aura_windows_platform::user_profile_directory().ok()
    }
}

/// Reads
/// the user's and Aura's Skill folders once, at low CPU priority, so the first open of 设置 → Skill
/// finds every file already hashed. Its reads keep normal I/O priority: at background I/O priority
/// the start's own reads held them back, and a page opened at once read the same files beside them
///. A failure here only means the page
/// reads them itself.
pub(crate) fn warm_in_background(runtime: Arc<SkillRuntime>) {
    if cfg!(test) {
        return;
    }
    let _ = std::thread::Builder::new().name("aura-skill-warmup".into()).spawn(move || {
        let reading = runtime.warming.lock();
        let _ = aura_windows_platform::lower_cpu_priority();
        let started = std::time::Instant::now();
        let skills = runtime.listing(None).map(|(_, _, skills)| skills.len()).ok();
        drop(reading);
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        crate::append_core_log(
            "skills.warmup",
            serde_json::json!({"elapsed_ms": elapsed_ms, "skills": skills}),
        );
    });
}

/// The user's and Aura's Skill folders are read once at the beginning of Core's start, on
/// their own, so their first reading after a build or an install — measured at 2.5 s for 227
/// files, against 67 ms on later starts — is done while the rest of the start runs (about 20 s
/// then) rather than when 设置 → Skill is first opened. Only the files are read here; the warm-up
/// at the end of the start keeps their hashes.
pub(crate) fn preread_in_background(settings: Arc<SettingsFacade>) {
    if cfg!(test) {
        return;
    }
    let _ = std::thread::Builder::new().name("aura-skill-preread".into()).spawn(move || {
        let _ = aura_windows_platform::lower_cpu_priority();
        let started = std::time::Instant::now();
        let skills = SkillRuntime::new(settings, None).listing(None).map(|(_, _, s)| s.len()).ok();
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        crate::append_core_log(
            "skills.preread",
            serde_json::json!({"elapsed_ms": elapsed_ms, "skills": skills}),
        );
    });
}

impl SkillRuntime {
    pub(crate) fn new(
        settings: Arc<SettingsFacade>,
        workspace: Option<Arc<aura_workspace::WorkspaceRuntime>>,
    ) -> Arc<Self> {
        let local_root = settings.paths().local_root.clone();
        let profile = profile(&local_root);
        Arc::new(Self {
            settings,
            local_root,
            profile,
            workspace,
            hashes: FileHashCache::default(),
            plugins: std::sync::OnceLock::new(),
            warming: std::sync::Mutex::new(()),
        })
    }

    pub(crate) fn settings(&self) -> &SettingsFacade {
        &self.settings
    }

    /// Ready plugins' Skills are listed from now on.
    pub(crate) fn attach_plugins(&self, plugins: Arc<crate::plugins::PluginRuntime>) {
        let _ = self.plugins.set(plugins);
    }

    pub(crate) fn plugin_runtime(&self) -> Option<&Arc<crate::plugins::PluginRuntime>> {
        self.plugins.get()
    }

    /// A ready plugin's Skills, each read now and offered only at the fingerprint it had when
    /// the plugin was trusted.
    fn plugin_skills(&self, skills: &mut Vec<LocatedSkill>) {
        let Some(plugins) = self.plugins.get() else { return };
        for plugin in plugins.ready().iter() {
            let name = &plugin.package.manifest.name;
            let root = plugin.package.root.join("skills");
            if !root.is_dir() {
                continue;
            }
            for found in scan_root(&root, &self.hashes) {
                let relative = format!("{name}/{}", found.relative);
                let id = aura_skills::skill_id(SkillSource::Plugin, None, &relative);
                let setting = SkillSetting {
                    id,
                    source: SkillSource::Plugin,
                    workspace_id: None,
                    relative,
                    enabled: !plugin.setting.changes.switched_off.skills.contains(&found.relative),
                    trusted_fingerprint: plugin
                        .setting
                        .trusted_parts
                        .skills
                        .get(&found.relative)
                        .cloned(),
                    available_for_subagents: false,
                };
                let mut view = aura_skills::view(&found, SkillSource::Plugin, None, Some(&setting));
                view.id = id;
                view.plugin = Some(name.clone());
                skills.push(LocatedSkill { view, found, setting: Some(setting) });
            }
        }
    }

    /// Aura's own Skill folder, where imports are copied.
    pub(crate) fn store_root(&self) -> PathBuf {
        self.local_root.join(STORE_SKILLS.replace('/', "\\"))
    }

    fn workspace_root(&self, id: WorkspaceId) -> anyhow::Result<PathBuf> {
        let runtime =
            self.workspace.as_ref().ok_or_else(|| anyhow::anyhow!("workspace.unavailable"))?;
        let resolver = runtime.resolver(id)?;
        resolver.verify_root_identity()?;
        Ok(resolver.canonical_root().to_path_buf())
    }

    /// The roots a listing covers, each with whether it can be read.
    fn roots(&self, workspace: Option<WorkspaceId>) -> anyhow::Result<Vec<Root>> {
        let mut roots = vec![Root {
            source: SkillSource::AuraStore,
            workspace: None,
            path: self.store_root(),
            status: root_status(&self.local_root, STORE_SKILLS),
        }];
        if let Some(profile) = &self.profile {
            roots.push(Root {
                source: SkillSource::AgentGlobal,
                workspace: None,
                path: profile.join(".agents").join("skills"),
                status: root_status(profile, AGENTS_SKILLS),
            });
        }
        if let Some(id) = workspace {
            let base = self.workspace_root(id)?;
            roots.push(Root {
                source: SkillSource::Workspace,
                workspace: Some(id),
                path: base.join(".agents").join("skills"),
                status: root_status(&base, AGENTS_SKILLS),
            });
        }
        Ok(roots)
    }

    /// Every Skill the listing covers, under the choices in force. A listing asked for while the
    /// start's first reading runs waits for it, then finds the files already hashed.
    pub(crate) fn list(
        &self,
        workspace: Option<WorkspaceId>,
    ) -> anyhow::Result<(SkillRegistry, Vec<SkillRootView>, Vec<LocatedSkill>)> {
        drop(self.warming.lock());
        self.listing(workspace)
    }

    fn listing(
        &self,
        workspace: Option<WorkspaceId>,
    ) -> anyhow::Result<(SkillRegistry, Vec<SkillRootView>, Vec<LocatedSkill>)> {
        let registry = self.settings.skill_registry();
        let mut roots = Vec::new();
        let mut skills = Vec::new();
        for Root { source, workspace, path: root, status } in self.roots(workspace)? {
            roots.push(SkillRootView {
                source,
                path: shown(&root),
                exists: status == RootStatus::Ready,
            });
            if status != RootStatus::Ready {
                continue;
            }
            for found in scan_root(&root, &self.hashes) {
                let id = aura_skills::skill_id(source, workspace, &found.relative);
                let setting = registry.skills.iter().find(|s| s.id == id).cloned();
                let view = aura_skills::view(&found, source, workspace, setting.as_ref());
                skills.push(LocatedSkill { view, found, setting });
            }
        }
        self.plugin_skills(&mut skills);
        Ok((registry, roots, skills))
    }

    /// One Skill by its identity, as found now.
    pub(crate) fn locate(
        &self,
        id: SkillInstanceId,
        workspace: Option<WorkspaceId>,
    ) -> anyhow::Result<LocatedSkill> {
        self.list(workspace)?.2.into_iter().find(|skill| skill.view.id == id).ok_or_else(|| {
            anyhow::anyhow!("skills.not_found: 这个 Skill 不在列表里（文件夹可能已移走），请刷新")
        })
    }
}

/// The ordinary spelling of a root: a workspace root is held in Windows' verbatim form
///.
fn shown(root: &Path) -> String {
    let text = root.display().to_string();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else {
        text.strip_prefix(r"\\?\").map_or_else(|| text.clone(), str::to_owned)
    }
}

#[cfg(test)]
mod shown_tests {
    use std::path::Path;

    #[test]
    fn a_root_is_shown_in_its_ordinary_spelling() {
        let shown = |path: &str| super::shown(Path::new(path));
        assert_eq!(shown(r"\\?\C:\work\demo\.agents\skills"), r"C:\work\demo\.agents\skills");
        assert_eq!(shown(r"\\?\UNC\host\share\skills"), r"\\host\share\skills");
        assert_eq!(shown(r"C:\Users\a\.agents\skills"), r"C:\Users\a\.agents\skills");
    }
}
