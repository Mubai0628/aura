//! `skills.*` model tools: the Skills usable now, found by words and loaded by name. Every
//! answer is read from the folders now, under the user's switches and the workspace's trust; a
//! Skill is guidance and grants nothing.
use super::*;
use aura_agent_domain::{
    LoadedSkillEntry, SkillContent, SkillFound, SkillOrigin, SkillsFoundResult, SkillsLoadInput,
    SkillsSearchInput,
};
use aura_extension_service::skills::loading::{self, NotLoaded};
use aura_extension_service::skills::{LocatedSkill, SkillRuntime, instructions};
use aura_skills::{SkillSource, SkillState};
use serde_json::Value;

/// The most Skills one message may choose.
pub const MAX_CHOSEN: usize = 20;
const DEFAULT_RESULTS: usize = 20;
const MAX_RESULTS: usize = 50;

pub(in crate::agent_execution) fn is_skill_tool(action_kind: &str) -> bool {
    action_kind.starts_with("skills.")
}

/// The process's Skills; absent only in tests that never set them up.
pub fn runtime(service: &AgentExecution) -> anyhow::Result<Arc<SkillRuntime>> {
    service
        .skills
        .get()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("skills.unavailable: 这个进程里没有 Skill 服务"))
}

pub(crate) const fn origin(source: SkillSource) -> SkillOrigin {
    match source {
        // A Skill that comes with Aura is in a folder of Aura's, as an imported one is.
        SkillSource::AuraStore | SkillSource::AuraBuiltIn => SkillOrigin::AuraStore,
        // A message's chosen Skills know three origins; a `.claude` folder is reported as the
        // user's or the workspace's own folder like `.agents`.
        SkillSource::AgentGlobal | SkillSource::ClaudeGlobal => SkillOrigin::AgentGlobal,
        SkillSource::Workspace | SkillSource::ClaudeWorkspace => SkillOrigin::Workspace,
        // A plugin's Skill was imported into Aura with its plugin.
        SkillSource::Plugin => SkillOrigin::AuraStore,
    }
}

// The model reads one of these fixed sentences; nothing about the folders is echoed back.
const UNAVAILABLE: &str = "skills.unavailable: 没有这个名字的可用 Skill（没有启用、工作区还没信任或已移走），请用 skills.search 查找可用的 Skill";
const AMBIGUOUS: &str = "skills.ambiguous: 有几个可用的 Skill 同名；请用 from 指明是哪一个（名字列表方括号里的写法，或 skills.search 结果里的 from）";
const ARGUMENTS: &str = "skills.arguments_invalid: 参数不对，请按工具说明填写";
const NOT_NOW: &str = "skills.unavailable_now: 暂时读不到 Skill，请稍后再试";

/// The fixed sentence for a refusal; anything unexpected is reported without its detail.
fn refusal(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    [UNAVAILABLE, AMBIGUOUS, ARGUMENTS].into_iter().find(|known| *known == text).unwrap_or(NOT_NOW)
}

fn input<T: serde::de::DeserializeOwned>(arguments: &Value) -> anyhow::Result<T> {
    serde_json::from_value(arguments.clone()).map_err(|_| anyhow::anyhow!(ARGUMENTS))
}

fn entry(skill: &LocatedSkill) -> anyhow::Result<LoadedSkillEntry> {
    Ok(LoadedSkillEntry {
        skill_id: skill.view.id,
        name: skill.view.name.clone().unwrap_or_default(),
        origin: origin(skill.view.source),
        fingerprint: skill.view.fingerprint.clone().ok_or_else(|| anyhow::anyhow!(UNAVAILABLE))?,
    })
}

/// The Skills the user chose in the composer, read when the message is sent: what the message
/// keeps of each, and each written out whole as it follows what the user typed.
#[derive(Clone, Debug, Default)]
pub struct ChosenSkills {
    pub entries: Vec<LoadedSkillEntry>,
    pub texts: Vec<String>,
}

/// Checks the chosen Skills: each must be usable now in this session's workspace; the same one
/// twice counts once; at most [`MAX_CHOSEN`].
pub fn selected(
    runtime: &SkillRuntime,
    workspace: Option<WorkspaceId>,
    ids: &[aura_domain_common::SkillInstanceId],
) -> anyhow::Result<ChosenSkills> {
    let mut chosen = Vec::new();
    for id in ids {
        if !chosen.contains(id) {
            chosen.push(*id);
        }
    }
    anyhow::ensure!(chosen.len() <= MAX_CHOSEN, "agent.skills_too_many: 一次最多选择 20 个 Skill");
    let unavailable =
        || anyhow::anyhow!("agent.skills_unavailable: 选择的 Skill 现在不能用，请重新选择");
    let listed = runtime.list(workspace)?.2;
    let mut skills = ChosenSkills::default();
    for id in &chosen {
        let skill = listed
            .iter()
            .find(|skill| skill.view.id == *id && skill.view.state == SkillState::Ready)
            .ok_or_else(unavailable)?;
        skills.entries.push(entry(skill)?);
        skills.texts.push(instructions::chosen_text(skill).ok_or_else(unavailable)?);
    }
    Ok(skills)
}

/// What one `skills.*` call answers, or the fixed sentence the model reads when it is refused.
/// The folders are read before the store transaction, so reading them never holds the journal.
pub(in crate::agent_execution) fn prepare(
    service: &AgentExecution,
    call: &CallContext,
    name: &str,
    arguments: &Value,
) -> Result<Value, &'static str> {
    answer(service, call, name, arguments).map_err(|error| refusal(&error))
}

fn answer(
    service: &AgentExecution,
    call: &CallContext,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let runtime = runtime(service)?;
    let workspace = Some(call.snapshot.workspace_id);
    Ok(match name {
        "skills.search" => {
            let input: SkillsSearchInput = input(arguments)?;
            let limit = input
                .limit
                .map_or(DEFAULT_RESULTS, |limit| usize::try_from(limit).unwrap_or(MAX_RESULTS))
                .clamp(1, MAX_RESULTS);
            let (found, truncated) = loading::search(&runtime, workspace, &input.query, limit)?;
            let skills = found
                .into_iter()
                .map(|skill| SkillFound {
                    name: skill.name,
                    from: skill.from,
                    description: skill.description,
                })
                .collect();
            serde_json::to_value(SkillsFoundResult { skills, truncated })?
        }
        "skills.load" => {
            let input: SkillsLoadInput = input(arguments)?;
            let loaded = loading::load(&runtime, workspace, &input.name, input.from.as_deref())?
                .map_err(|not| match not {
                    NotLoaded::Unavailable => anyhow::anyhow!(UNAVAILABLE),
                    NotLoaded::Ambiguous => anyhow::anyhow!(AMBIGUOUS),
                })?;
            serde_json::to_value(SkillContent {
                name: loaded.name,
                from: loaded.from,
                description: loaded.description,
                folder: loaded.folder,
                files: loaded.files,
                more_files: loaded.more_files,
                allowed_tools: loaded.allowed_tools,
                instructions: loaded.instructions,
            })?
        }
        _ => anyhow::bail!(ARGUMENTS),
    })
}
