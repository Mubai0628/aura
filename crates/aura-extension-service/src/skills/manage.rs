//! Settings → Skill: list, preview, trust, untrust, switch, pin, import and remove. Every change
//! names the `skills.json` revision it was made against. A workspace's Skills are trusted once
//! for the workspace; trust of one Skill alone is given only at the fingerprint the user was
//! shown. Importing copies a folder, or unpacks a ZIP file, into Aura's own; nothing else is
//! written.
use super::instructions::{self, PINNED_LIMIT_TOKENS};
use super::runtime::{LocatedSkill, SkillRuntime};
use aura_config::SettingsFacadeError;
use aura_core_base::{append_core_log, recycle};
use aura_domain_common::SkillInstanceId;
use aura_skills::{
    SkillChangeRequest, SkillImportRequest, SkillListRequest, SkillPreview, SkillRegistryView,
    SkillRequest, SkillSetEnabledRequest, SkillSetPinnedRequest, SkillSetting, SkillSource,
    SkillTrustRequest, SkillWorkspaceTrustRequest, skill_id,
};
use std::path::Path;
use std::sync::Arc;

pub struct SkillContext {
    runtime: Arc<SkillRuntime>,
}

impl SkillContext {
    pub fn new(runtime: Arc<SkillRuntime>) -> Self {
        Self { runtime }
    }

    /// Runs one `agent.skills.*` Core route.
    pub fn dispatch(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(match method {
            "agent.skills.list" => {
                let request: SkillListRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.view(request.workspace_id)?)?
            }
            "agent.skills.preview" => {
                let request: SkillRequest = serde_json::from_value(params)?;
                let located = self.runtime.locate(request.id, request.workspace_id)?;
                let (skill_md, files) = match &located.found.scanned {
                    Ok(package) => (package.skill_md.clone(), package.files.clone()),
                    Err(_) => (String::new(), Vec::new()),
                };
                serde_json::to_value(SkillPreview { skill: located.view, skill_md, files })?
            }
            "agent.skills.trust" => {
                let request: SkillTrustRequest = serde_json::from_value(params)?;
                let located = self.runtime.locate(request.id, request.workspace_id)?;
                let fingerprint = located.view.fingerprint.clone().ok_or_else(|| {
                    anyhow::anyhow!("skills.invalid: {}", located.view.problems.join("；"))
                })?;
                anyhow::ensure!(
                    fingerprint == request.expected_fingerprint,
                    "skills.content_changed: Skill 的内容在你查看之后变了，请重新查看再信任"
                );
                self.change(&located, request.expected_revision, request.workspace_id, |s| {
                    s.enabled = true;
                    s.trusted_fingerprint = Some(fingerprint);
                })?
            }
            "agent.skills.untrust" => {
                let request: SkillChangeRequest = serde_json::from_value(params)?;
                let located = self.runtime.locate(request.id, request.workspace_id)?;
                self.change(&located, request.expected_revision, request.workspace_id, |s| {
                    s.trusted_fingerprint = None;
                })?
            }
            "agent.skills.trust_workspace" | "agent.skills.untrust_workspace" => {
                let request: SkillWorkspaceTrustRequest = serde_json::from_value(params)?;
                let trust = method == "agent.skills.trust_workspace";
                serde_json::to_value(self.trust_workspace(&request, trust)?)?
            }
            "agent.skills.set_enabled" => {
                let request: SkillSetEnabledRequest = serde_json::from_value(params)?;
                let located = self.runtime.locate(request.id, request.workspace_id)?;
                if let Some(plugin) = &located.view.plugin {
                    let plugins = self
                        .runtime
                        .plugin_runtime()
                        .ok_or_else(|| anyhow::anyhow!("skills.from_plugin: 插件暂时不可用"))?;
                    let relative = &located.view.relative;
                    let folder =
                        relative.strip_prefix(&format!("{plugin}/")).unwrap_or(relative.as_str());
                    plugins.switch_part(
                        plugin,
                        crate::plugins::PartKind::Skill,
                        folder,
                        request.enabled,
                    )?;
                    return Ok(serde_json::to_value(self.view(request.workspace_id)?)?);
                }
                self.change(&located, request.expected_revision, request.workspace_id, |s| {
                    s.enabled = request.enabled;
                })?
            }
            "agent.skills.set_pinned" => {
                serde_json::to_value(self.set_pinned(&serde_json::from_value(params)?)?)?
            }
            "agent.skills.import" => {
                serde_json::to_value(self.import(serde_json::from_value(params)?)?)?
            }
            "agent.skills.remove" => {
                serde_json::to_value(self.remove(serde_json::from_value(params)?)?)?
            }
            other => anyhow::bail!("skills.unknown_route: {other}"),
        })
    }

    fn view(
        &self,
        workspace: Option<aura_domain_common::WorkspaceId>,
    ) -> anyhow::Result<SkillRegistryView> {
        let (registry, roots, skills) = self.runtime.list(workspace)?;
        Ok(SkillRegistryView {
            revision: registry.revision,
            pinned_tokens: instructions::pinned_tokens(&skills),
            pinned_limit: PINNED_LIMIT_TOKENS,
            skills: skills.into_iter().map(|skill| skill.view).collect(),
            roots,
            issue: registry.issue,
            workspace_trusted: workspace
                .is_some_and(|id| registry.trusted_workspaces.contains(&id)),
        })
    }

    fn write(&self, expected_revision: u64, skills: &[SkillSetting]) -> anyhow::Result<()> {
        written(self.runtime.settings().replace_skill_registry(expected_revision, skills))
    }

    /// Pins a usable Skill while the pinned Skills sent now and it fit in the limit together, or
    /// unpins one. Pins apply from the next turn.
    fn set_pinned(&self, request: &SkillSetPinnedRequest) -> anyhow::Result<SkillRegistryView> {
        let (registry, _, skills) = self.runtime.list(request.workspace_id)?;
        anyhow::ensure!(
            registry.issue.is_none(),
            "skills.registry_unavailable: {}",
            registry.issue.clone().unwrap_or_default()
        );
        let skill = skills.iter().find(|skill| skill.view.id == request.id).ok_or_else(|| {
            anyhow::anyhow!("skills.not_found: 这个 Skill 不在列表里（文件夹可能已移走），请刷新")
        })?;
        if request.pinned && !skill.view.pinned {
            let text = instructions::pinned_text(skill).ok_or_else(|| {
                anyhow::anyhow!("skills.not_ready: 这个 Skill 现在不能用，不能固定加载")
            })?;
            let (used, size) = (instructions::pinned_tokens(&skills), instructions::tokens(&text));
            anyhow::ensure!(
                used + size <= PINNED_LIMIT_TOKENS,
                "skills.pin_limit: 固定加载的 Skill 合计不能超过 {PINNED_LIMIT_TOKENS} token（已固定约 {used}，这个约 {size}），请先取消别的固定加载"
            );
        }
        written(self.runtime.settings().update_skill_choices(request.expected_revision, |c| {
            c.pinned.retain(|id| *id != request.id);
            if request.pinned {
                c.pinned.push(request.id);
            }
        }))?;
        self.view(request.workspace_id)
    }

    /// Drops a removed Skill's choices and its pin.
    fn forget(&self, expected_revision: u64, id: SkillInstanceId) -> anyhow::Result<()> {
        written(self.runtime.settings().update_skill_choices(expected_revision, |choices| {
            choices.skills.retain(|skill| skill.id != id);
            choices.pinned.retain(|pinned| *pinned != id);
        }))
    }

    /// Trusts, or stops trusting, every Skill in one workspace's folders. Withdrawing also clears
    /// a trust given earlier to one of its Skills alone, so none of them stays offered.
    fn trust_workspace(
        &self,
        request: &SkillWorkspaceTrustRequest,
        trust: bool,
    ) -> anyhow::Result<SkillRegistryView> {
        let workspace = request.workspace_id;
        self.runtime.workspace_root(workspace)?;
        let registry = self.runtime.settings().skill_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "skills.registry_unavailable: {}",
            registry.issue.clone().unwrap_or_default()
        );
        written(self.runtime.settings().update_skill_choices(
            request.expected_revision,
            |choices| {
                choices.trusted_workspaces.retain(|id| *id != workspace);
                if trust {
                    choices.trusted_workspaces.push(workspace);
                    return;
                }
                for skill in &mut choices.skills {
                    if skill.source.in_workspace() && skill.workspace_id == Some(workspace) {
                        skill.trusted_fingerprint = None;
                    }
                }
            },
        ))?;
        self.view(Some(workspace))
    }

    /// Saves one Skill's choices, starting from its saved ones or its source's defaults.
    fn change(
        &self,
        located: &LocatedSkill,
        expected_revision: u64,
        workspace: Option<aura_domain_common::WorkspaceId>,
        edit: impl FnOnce(&mut SkillSetting),
    ) -> anyhow::Result<serde_json::Value> {
        let registry = self.runtime.settings().skill_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "skills.registry_unavailable: {}",
            registry.issue.clone().unwrap_or_default()
        );
        let view = &located.view;
        from_plugin(view)?;
        let mut setting = located.setting.clone().unwrap_or(SkillSetting {
            id: view.id,
            source: view.source,
            workspace_id: view.workspace_id,
            relative: view.relative.clone(),
            enabled: view.enabled,
            trusted_fingerprint: None,
            available_for_subagents: false,
        });
        edit(&mut setting);
        let mut skills = registry.skills;
        skills.retain(|skill| skill.id != setting.id);
        skills.push(setting);
        self.write(expected_revision, &skills)?;
        Ok(serde_json::to_value(self.view(workspace)?)?)
    }

    /// Copies a Skill folder, or unpacks a ZIP file, into Aura's own folder, switched on; the
    /// user chose it, so it needs no trust. The copy must have exactly the content that was
    /// checked.
    fn import(&self, request: SkillImportRequest) -> anyhow::Result<SkillRegistryView> {
        let registry = self.runtime.settings().skill_registry();
        anyhow::ensure!(
            registry.revision == request.expected_revision,
            "skills.revision_changed: Skill 设置已被修改，请刷新后再操作"
        );
        let staged = super::import::stage(&self.runtime, Path::new(&request.path))?;
        let store = self.runtime.store_root();
        let name = super::import::free_name(&store, &staged.package.frontmatter.name);
        let moved = std::fs::rename(&staged.folder, store.join(&name));
        let _ = std::fs::remove_dir_all(&staged.staging);
        moved?;
        let id = skill_id(SkillSource::AuraStore, None, &name);
        let mut skills = registry.skills;
        skills.retain(|skill| skill.id != id);
        skills.push(SkillSetting {
            id,
            source: SkillSource::AuraStore,
            workspace_id: None,
            relative: name.clone(),
            enabled: true,
            trusted_fingerprint: None,
            available_for_subagents: false,
        });
        if let Err(error) = self.write(request.expected_revision, &skills) {
            let _ = std::fs::remove_dir_all(store.join(&name));
            return Err(error);
        }
        self.view(None)
    }

    /// Removes a Skill. Aura's own copy is deleted outright; a Skill in the user's or a
    /// workspace's `.agents` or `.claude` folder goes to the Recycle Bin, because it may be the
    /// only copy and other tools read that folder too. A plugin's Skill goes only with its plugin.
    fn remove(&self, request: SkillChangeRequest) -> anyhow::Result<SkillRegistryView> {
        let located = self.runtime.locate(request.id, request.workspace_id)?;
        from_plugin(&located.view)?;
        anyhow::ensure!(
            located.view.source != SkillSource::AuraBuiltIn,
            "skills.not_removable: Aura 自带的 Skill 不能删除；不想用可以停用"
        );
        let users_folder =
            !matches!(located.view.source, SkillSource::AuraStore | SkillSource::Plugin);
        anyhow::ensure!(
            located.view.source == SkillSource::AuraStore || users_folder,
            "skills.not_removable: 这个 Skill 不能在这里删除，请在它的文件夹里处理"
        );
        let registry = self.runtime.settings().skill_registry();
        anyhow::ensure!(
            registry.revision == request.expected_revision,
            "skills.revision_changed: Skill 设置已被修改，请刷新后再操作"
        );
        let directory = &located.found.directory;
        let metadata = std::fs::symlink_metadata(directory)?;
        anyhow::ensure!(
            metadata.is_dir() && !crate::skills::reparse_point(&metadata),
            "skills.not_removable: 这个文件夹是链接或挂载点，请在资源管理器中处理"
        );
        if users_folder {
            use aura_windows_platform::RecycleOutcome;
            match recycle::move_to_recycle_bin(directory) {
                Ok(RecycleOutcome::Recycled) => {}
                Ok(RecycleOutcome::NotRecyclable) => anyhow::bail!(
                    "skills.not_recyclable: 回收站放不下这个文件夹（或它所在的位置没有回收站），没有删除，它还在原处；请在资源管理器中处理。"
                ),
                Err(error) => anyhow::bail!(
                    "skills.remove_busy: 这个 Skill 的文件夹正被其他程序使用（例如资源管理器或编辑器打开着它或里面的文件），关闭后再删除；Skill 没有改变（{error}）"
                ),
            }
            self.forget(request.expected_revision, request.id)?;
            return self.view(request.workspace_id);
        }
        let store = self.runtime.store_root();
        sweep_removed(&store);
        let trash = store.join(format!(".removed-{}", aura_domain_common::CorrelationId::new()));
        // Windows refuses to move a folder another program has open
        // (Explorer after 打开文件夹, an editor), and the raw OS error read as "try again".
        std::fs::rename(directory, &trash).map_err(|error| {
            anyhow::anyhow!(
                "skills.remove_busy: 这个 Skill 的文件夹正被其他程序使用（例如资源管理器或编辑器打开着它或里面的文件），关闭后再移除；Skill 没有改变（{error}）"
            )
        })?;
        if let Err(error) = std::fs::remove_dir_all(&trash) {
            // Already out of the list (the scan skips dot folders): the Skill is gone, and the
            // hidden rest is swept at the next removal.
            append_core_log(
                "skills.remove_cleanup_failed",
                serde_json::json!({"directory": trash.display().to_string(), "error": error.to_string()}),
            );
        }
        self.forget(request.expected_revision, request.id)?;
        self.view(request.workspace_id)
    }
}

/// A concurrent change to `skills.json` is the user's to reread, never overwritten.
fn written(result: Result<u64, SettingsFacadeError>) -> anyhow::Result<()> {
    match result {
        Ok(_) => Ok(()),
        Err(SettingsFacadeError::RevisionConflict { .. }) => {
            anyhow::bail!("skills.revision_changed: Skill 设置已被修改，请刷新后再操作")
        }
        Err(error) => Err(error.into()),
    }
}

/// A plugin's Skill is trusted and removed with its plugin.
fn from_plugin(view: &aura_skills::SkillView) -> anyhow::Result<()> {
    match &view.plugin {
        Some(plugin) => anyhow::bail!(
            "skills.from_plugin: 这个 Skill 来自插件 {plugin}，它的信任和移除都随插件"
        ),
        None => Ok(()),
    }
}

/// Removes what an earlier removal could not delete (`.removed-*`; the scan never lists it).
fn sweep_removed(store: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(store) else { return };
    for entry in entries.filter_map(Result::ok) {
        if entry.file_name().to_string_lossy().starts_with(".removed-") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
