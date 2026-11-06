//! Where Skill folders are, and what each one is under the user's choices. Aura's own folder,
//! the folder of the Skills that come with Aura,
//! the user's `.agents\skills` and `.claude\skills`, and the selected workspace's two folders of
//! the same names are read; every folder on the way must be a real folder. A ready plugin's
//! Skills follow the plugin.
use aura_config::{SettingsFacade, SkillRegistry};
use aura_core_base::append_core_log;
use aura_domain_common::{SkillInstanceId, WorkspaceId};
use aura_skills::{
    FileHashCache, FoundSkill, RootStatus, SkillRootView, SkillSetting, SkillSource, SkillView,
    root_status, scan_root,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Under a user profile or a workspace.
pub(crate) const AGENTS_SKILLS: &str = ".agents/skills";
/// Under a user profile or a workspace, as Claude Code keeps them.
pub(crate) const CLAUDE_SKILLS: &str = ".claude/skills";
/// Under Aura's local data root.
pub(crate) const STORE_SKILLS: &str = "skills/packages";

/// One Skill as found now, with the saved choices it is shown under.
#[derive(Clone, Debug)]
pub struct LocatedSkill {
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

pub struct SkillRuntime {
    settings: Arc<SettingsFacade>,
    local_root: PathBuf,
    /// The folder holding the user's `.agents`: the user profile, or a TEMP stand-in in tests.
    profile: Option<PathBuf>,
    workspace: Option<Arc<aura_workspace::WorkspaceRuntime>>,
    pub hashes: FileHashCache,
    plugins: std::sync::OnceLock<Arc<crate::plugins::PluginRuntime>>,
    /// Held while the start's first reading of the Skill folders runs; a listing waits for
    /// it rather than reading the same files beside it.
    pub(super) warming: std::sync::Mutex<()>,
    /// Whether the Skills that come with Aura are listed: always, except in a test that has not
    /// asked for them, so that it lists only what it wrote.
    builtin: std::sync::atomic::AtomicBool,
    /// Set once this start has written them.
    builtin_written: std::sync::OnceLock<()>,
}

/// The user profile, except in tests: unit tests use a folder under their TEMP data root, and
/// an E2E build reads only `AURA_USER_PROFILE_TEST_ROOT` inside TEMP.
fn profile(local_root: &Path) -> Option<PathBuf> {
    #[cfg(any(test, feature = "test-hooks"))]
    {
        Some(local_root.join("test-profile"))
    }
    #[cfg(all(not(any(test, feature = "test-hooks")), feature = "memory-test-profile"))]
    {
        let _ = local_root;
        let root = PathBuf::from(std::env::var_os("AURA_USER_PROFILE_TEST_ROOT")?);
        let temp = std::env::temp_dir();
        (root.is_absolute() && root.starts_with(&temp) && root != temp).then_some(root)
    }
    #[cfg(all(not(any(test, feature = "test-hooks")), not(feature = "memory-test-profile")))]
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
pub fn warm_in_background(runtime: Arc<SkillRuntime>) {
    if cfg!(any(test, feature = "test-hooks")) {
        return;
    }
    let _ = std::thread::Builder::new().name("aura-skill-warmup".into()).spawn(move || {
        let reading = runtime.warming.lock();
        let _ = aura_windows_platform::lower_cpu_priority();
        let started = std::time::Instant::now();
        let skills = runtime.listing(None).map(|(_, _, skills)| skills.len()).ok();
        drop(reading);
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        append_core_log(
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
pub fn preread_in_background(settings: Arc<SettingsFacade>) {
    if cfg!(any(test, feature = "test-hooks")) {
        return;
    }
    let _ = std::thread::Builder::new().name("aura-skill-preread".into()).spawn(move || {
        let _ = aura_windows_platform::lower_cpu_priority();
        let started = std::time::Instant::now();
        let skills = SkillRuntime::new(settings, None).listing(None).map(|(_, _, s)| s.len()).ok();
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        append_core_log(
            "skills.preread",
            serde_json::json!({"elapsed_ms": elapsed_ms, "skills": skills}),
        );
    });
}

impl SkillRuntime {
    pub fn new(
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
            builtin: std::sync::atomic::AtomicBool::new(!cfg!(any(test, feature = "test-hooks"))),
            builtin_written: std::sync::OnceLock::new(),
        })
    }

    /// Lists the Skills that come with Aura in a test, as every other build does.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn include_builtin_skills(&self) {
        self.builtin.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn settings(&self) -> &SettingsFacade {
        &self.settings
    }

    /// Ready plugins' Skills are listed from now on.
    pub fn attach_plugins(&self, plugins: Arc<crate::plugins::PluginRuntime>) {
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
                let mut view =
                    aura_skills::view(&found, SkillSource::Plugin, None, Some(&setting), false);
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

    pub(crate) fn workspace_root(&self, id: WorkspaceId) -> anyhow::Result<PathBuf> {
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
        if self.builtin.load(std::sync::atomic::Ordering::Relaxed) {
            use super::builtin::{BUILTIN_SKILLS, write_all};
            self.builtin_written.get_or_init(|| {
                if let Err(error) = write_all(&self.local_root) {
                    append_core_log(
                        "skills.builtin",
                        serde_json::json!({"error": error.to_string()}),
                    );
                }
            });
            roots.push(Root {
                source: SkillSource::AuraBuiltIn,
                workspace: None,
                path: self.local_root.join(BUILTIN_SKILLS.replace('/', "\\")),
                status: root_status(&self.local_root, BUILTIN_SKILLS),
            });
        }
        let under = |base: &Path, relative: &str, source, workspace| Root {
            source,
            workspace,
            path: relative.split('/').fold(base.to_path_buf(), |path, part| path.join(part)),
            status: root_status(base, relative),
        };
        if let Some(profile) = &self.profile {
            roots.push(under(profile, AGENTS_SKILLS, SkillSource::AgentGlobal, None));
            roots.push(under(profile, CLAUDE_SKILLS, SkillSource::ClaudeGlobal, None));
        }
        if let Some(id) = workspace {
            let base = self.workspace_root(id)?;
            roots.push(under(&base, AGENTS_SKILLS, SkillSource::Workspace, Some(id)));
            roots.push(under(&base, CLAUDE_SKILLS, SkillSource::ClaudeWorkspace, Some(id)));
        }
        Ok(roots)
    }

    /// Every Skill the listing covers, under the choices in force. A listing asked for while the
    /// start's first reading runs waits for it, then finds the files already hashed.
    pub fn list(
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
        let workspace_trusted =
            workspace.is_some_and(|id| registry.trusted_workspaces.contains(&id));
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
                let view = aura_skills::view(
                    &found,
                    source,
                    workspace,
                    setting.as_ref(),
                    workspace_trusted,
                );
                skills.push(LocatedSkill { view, found, setting });
            }
        }
        self.plugin_skills(&mut skills);
        super::instructions::mark_pins(&registry.pinned, &mut skills);
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
