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

// The model reads one of these fixed sentences when a call is refused: the cause, that nothing
// was read, and what to do. The refusal is recorded as the line, a full stop and a sentence of its
// own, so a line ends without one. Nothing about the folders is echoed back; the answer must be a
// fixed text, so each tool names its own fields.
const UNAVAILABLE: &str = "skills.unavailable: 没有这个名字的可用 Skill（没有启用、工作区还没信任或已移走），什么都没读。请用 skills.search 查找可用的 Skill";
const AMBIGUOUS: &str = "skills.ambiguous: 有几个可用的 Skill 同名，什么都没读。请用 from 指明是哪一个（名字列表方括号里的写法，或 skills.search 结果里的 from）";
const ARGUMENTS_SEARCH: &str = "skills.arguments_invalid: skills.search 只接受 query（文字：要找的词，空字符串列出全部）和可选的 limit（整数：条数，默认 20，最多 50），什么都没读";
const ARGUMENTS_LOAD: &str = "skills.arguments_invalid: skills.load 只接受 name（文字：Skill 的名字）和可选的 from（文字：同名时指明是哪一个），什么都没读";
const ARGUMENTS: &str = "skills.arguments_invalid: 没有这个 Skill 工具，什么都没读。可用的是 skills.search 和 skills.load";
const WORKSPACE: &str = "skills.workspace_unavailable: 这个工作区现在读不了（可能被移动、删除或换了磁盘），什么都没读。请用户确认工作区还在后再试";
const NOT_NOW: &str =
    "skills.unavailable_now: Skill 功能现在没有在运行，什么都没读。请稍后再试；反复出现时告诉用户";

/// The fixed sentence for a refusal; anything unexpected is reported without its detail.
fn refusal(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    [UNAVAILABLE, AMBIGUOUS, ARGUMENTS_SEARCH, ARGUMENTS_LOAD, ARGUMENTS, WORKSPACE]
        .into_iter()
        .find(|known| *known == text)
        .unwrap_or(NOT_NOW)
}

fn input<T: serde::de::DeserializeOwned>(
    arguments: &Value,
    refused: &'static str,
) -> anyhow::Result<T> {
    serde_json::from_value(arguments.clone()).map_err(|_| anyhow::anyhow!(refused))
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
    let runtime = runtime(service).map_err(|_| anyhow::anyhow!(NOT_NOW))?;
    let workspace = Some(service.skills_workspace(call.snapshot.workspace_id));
    // A listing fails only when the workspace's root cannot be resolved or is no longer the
    // folder it was.
    let unreadable = |_: anyhow::Error| anyhow::anyhow!(WORKSPACE);
    Ok(match name {
        "skills.search" => {
            let input: SkillsSearchInput = input(arguments, ARGUMENTS_SEARCH)?;
            let limit = input
                .limit
                .map_or(DEFAULT_RESULTS, |limit| usize::try_from(limit).unwrap_or(MAX_RESULTS))
                .clamp(1, MAX_RESULTS);
            let (found, truncated) =
                loading::search(&runtime, workspace, &input.query, limit).map_err(unreadable)?;
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
            let input: SkillsLoadInput = input(arguments, ARGUMENTS_LOAD)?;
            let loaded = loading::load(&runtime, workspace, &input.name, input.from.as_deref())
                .map_err(unreadable)?
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

#[cfg(test)]
mod tests {
    use super::{
        AMBIGUOUS, ARGUMENTS, ARGUMENTS_LOAD, ARGUMENTS_SEARCH, NOT_NOW, UNAVAILABLE, WORKSPACE,
        refusal,
    };

    #[test]
    fn every_refusal_names_its_cause_and_says_nothing_was_read() {
        let fixed =
            [UNAVAILABLE, AMBIGUOUS, ARGUMENTS_SEARCH, ARGUMENTS_LOAD, ARGUMENTS, WORKSPACE];
        for line in fixed {
            assert_eq!(refusal(&anyhow::anyhow!(line)), line, "each cause keeps its own line");
        }
        for line in fixed.into_iter().chain([NOT_NOW]) {
            let (code, sentence) = line.split_once(": ").expect("a code comes first");
            assert!(code.starts_with("skills.") && !code.contains(' '), "{line}");
            assert!(sentence.contains("什么都没读"), "{line}");
            assert!(line.len() <= 512 && !line.contains(['{', '}']), "{line}");
            assert!(!line.ends_with(['。', '.']), "the record adds its own full stop: {line}");
        }
        assert!(ARGUMENTS_SEARCH.contains("query") && ARGUMENTS_SEARCH.contains("limit"));
        assert!(ARGUMENTS_LOAD.contains("name") && ARGUMENTS_LOAD.contains("from"));
        assert_eq!(refusal(&anyhow::anyhow!("workspace.unavailable")), NOT_NOW);
    }
}
