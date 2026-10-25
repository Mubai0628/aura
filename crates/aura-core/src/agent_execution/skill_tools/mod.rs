//! `skills.*` model tools: the user's trusted Skills, found, read and loaded for this turn.
//! Every answer is read from the folders now and checked against the fingerprint the user
//! trusted; a Skill is guidance and grants nothing.
use super::*;
use crate::skills::{LocatedSkill, SkillRuntime};
use aura_agent_domain::{
    LoadedSkillEntry, SkillDescription, SkillFileEntry, SkillOrigin, SkillSummary,
    SkillsDescribeInput, SkillsLoadInput, SkillsLoadedResult, SkillsReadResourceInput,
    SkillsReadResourceResult, SkillsSearchInput, SkillsSearchResult, SkillsUnloadInput,
};
use aura_skills::{SkillSource, SkillState};
use serde_json::Value;

/// The most Skills one turn may have loaded.
pub(crate) const MAX_LOADED: usize = 20;
const DEFAULT_RESULTS: usize = 20;
const MAX_RESULTS: usize = 50;

pub(in crate::agent_execution) fn is_skill_tool(action_kind: &str) -> bool {
    action_kind.starts_with("skills.")
}

/// The process's Skills; absent only in tests that never set them up.
pub(crate) fn runtime(service: &AgentExecution) -> anyhow::Result<Arc<SkillRuntime>> {
    service
        .skills
        .get()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("skills.unavailable: 这个进程里没有 Skill 服务"))
}

pub(crate) const fn origin(source: SkillSource) -> SkillOrigin {
    match source {
        SkillSource::AuraStore => SkillOrigin::AuraStore,
        SkillSource::AgentGlobal => SkillOrigin::AgentGlobal,
        SkillSource::Workspace => SkillOrigin::Workspace,
        // A plugin's Skill was imported into Aura with its plugin.
        SkillSource::Plugin => SkillOrigin::AuraStore,
    }
}

/// What a tool answers, and the loaded set to record when it changed. Computed before the
/// store transaction, so reading Skill folders never holds the journal.
pub(in crate::agent_execution) struct Prepared {
    pub output: Value,
    pub loaded: Option<Vec<LoadedSkillEntry>>,
}

// The model reads one of these fixed sentences; nothing about the folders is echoed back.
const UNAVAILABLE: &str = "skills.unavailable: 这个 Skill 现在不能用（没有启用、没有信任、内容在信任后变了或已移走），请用 skills.search 查找可用的 Skill";
const NOT_LOADED: &str = "skills.not_loaded: 先用 skills.load 加载这个 Skill，再读取它的文件";
const LIMIT: &str = "skills.limit: 本轮最多加载 20 个 Skill；先用 skills.unload 卸下不再需要的";
const CHANGED: &str =
    "skills.changed: 这个 Skill 的内容在加载后变了，不能读取；请用户重新信任后再加载";
const RESOURCE: &str = "skills.resource: 只能读取 skills.describe 列出的 Skill 文件夹内的文本文件（相对路径，最大 1 MiB）";
const ARGUMENTS: &str = "skills.arguments_invalid: 参数不对，请按工具说明填写";
const NOT_NOW: &str = "skills.unavailable_now: 暂时读不到 Skill，请稍后再试";

/// The fixed sentence for a refusal; anything unexpected is reported without its detail.
fn refusal(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    [UNAVAILABLE, NOT_LOADED, LIMIT, CHANGED, RESOURCE, ARGUMENTS]
        .into_iter()
        .find(|known| *known == text)
        .unwrap_or(NOT_NOW)
}

fn input<T: serde::de::DeserializeOwned>(arguments: &Value) -> anyhow::Result<T> {
    serde_json::from_value(arguments.clone()).map_err(|_| anyhow::anyhow!(ARGUMENTS))
}

fn ready(runtime: &SkillRuntime, workspace: WorkspaceId) -> anyhow::Result<Vec<LocatedSkill>> {
    Ok(runtime
        .list(Some(workspace))?
        .2
        .into_iter()
        .filter(|skill| skill.view.state == SkillState::Ready)
        .collect())
}

fn entry(skill: &LocatedSkill) -> anyhow::Result<LoadedSkillEntry> {
    Ok(LoadedSkillEntry {
        skill_id: skill.view.id,
        name: skill.view.name.clone().unwrap_or_default(),
        origin: origin(skill.view.source),
        fingerprint: skill.view.fingerprint.clone().ok_or_else(|| anyhow::anyhow!(UNAVAILABLE))?,
    })
}

/// The Skills the user chose in the composer, checked when the message is sent: each must be
/// usable now in this session's workspace; the same one twice counts once; at most
/// [`MAX_LOADED`].
pub(crate) fn selected(
    runtime: &SkillRuntime,
    workspace: Option<WorkspaceId>,
    ids: &[aura_domain_common::SkillInstanceId],
) -> anyhow::Result<Vec<LoadedSkillEntry>> {
    let mut chosen = Vec::new();
    for id in ids {
        if !chosen.contains(id) {
            chosen.push(*id);
        }
    }
    anyhow::ensure!(chosen.len() <= MAX_LOADED, "agent.skills_too_many: 一次最多选择 20 个 Skill");
    let listed = runtime.list(workspace)?.2;
    chosen
        .iter()
        .map(|id| {
            let skill = listed
                .iter()
                .find(|skill| skill.view.id == *id && skill.view.state == SkillState::Ready)
                .ok_or_else(|| {
                    anyhow::anyhow!("agent.skills_unavailable: 选择的 Skill 现在不能用，请重新选择")
                })?;
            entry(skill)
        })
        .collect()
}

fn matches(skill: &LocatedSkill, words: &[String]) -> bool {
    let name = skill.view.name.clone().unwrap_or_default().to_lowercase();
    let description = skill.view.description.clone().unwrap_or_default().to_lowercase();
    words.iter().all(|word| name.contains(word) || description.contains(word))
}

/// What one `skills.*` call answers, or the fixed sentence the model reads when it is refused.
pub(in crate::agent_execution) fn prepare(
    service: &AgentExecution,
    call: &CallContext,
    name: &str,
    arguments: &Value,
) -> Result<Prepared, &'static str> {
    answer(service, call, name, arguments).map_err(|error| refusal(&error))
}

fn answer(
    service: &AgentExecution,
    call: &CallContext,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Prepared> {
    let runtime = runtime(service)?;
    let run = call.batch.invocation_identity.agent_run_id;
    let workspace = call.snapshot.workspace_id;
    let loaded = {
        let store = service.journal.lock()?;
        aura_persistence_v2::agent::skills::loaded(store.database.connection(), run)?
    };
    let is_loaded = |id| loaded.iter().any(|entry: &LoadedSkillEntry| entry.skill_id == id);
    let answer = |value: Value| Prepared { output: value, loaded: None };
    Ok(match name {
        "skills.search" => {
            let input: SkillsSearchInput = input(arguments)?;
            let words: Vec<String> =
                input.query.split_whitespace().map(str::to_lowercase).collect();
            let limit = input
                .limit
                .map_or(DEFAULT_RESULTS, |limit| usize::try_from(limit).unwrap_or(MAX_RESULTS))
                .clamp(1, MAX_RESULTS);
            let mut found: Vec<_> =
                ready(&runtime, workspace)?.into_iter().filter(|s| matches(s, &words)).collect();
            found.sort_by_key(|skill| skill.view.name.clone().unwrap_or_default());
            let truncated = found.len() > limit;
            let skills = found
                .into_iter()
                .take(limit)
                .map(|skill| SkillSummary {
                    skill_id: skill.view.id,
                    name: skill.view.name.clone().unwrap_or_default(),
                    description: skill.view.description.clone().unwrap_or_default(),
                    origin: origin(skill.view.source),
                    allowed_tools: skill.view.allowed_tools.clone(),
                    loaded: is_loaded(skill.view.id),
                })
                .collect();
            answer(serde_json::to_value(SkillsSearchResult { skills, truncated })?)
        }
        "skills.describe" => {
            let input: SkillsDescribeInput = input(arguments)?;
            let skill = ready(&runtime, workspace)?
                .into_iter()
                .find(|skill| skill.view.id == input.skill_id)
                .ok_or_else(|| anyhow::anyhow!(UNAVAILABLE))?;
            let package = skill.found.scanned.as_ref().map_err(|_| anyhow::anyhow!(UNAVAILABLE))?;
            answer(serde_json::to_value(SkillDescription {
                skill_id: skill.view.id,
                name: package.frontmatter.name.clone(),
                description: package.frontmatter.description.clone(),
                origin: origin(skill.view.source),
                license: package.frontmatter.license.clone(),
                compatibility: package.frontmatter.compatibility.clone(),
                allowed_tools: package.frontmatter.allowed_tools.clone(),
                files: package
                    .files
                    .iter()
                    .map(|file| SkillFileEntry { path: file.path.clone(), bytes: file.bytes })
                    .collect(),
                loaded: is_loaded(skill.view.id),
            })?)
        }
        "skills.load" => {
            let input: SkillsLoadInput = input(arguments)?;
            let available = ready(&runtime, workspace)?;
            let mut next = loaded.clone();
            for id in input.skill_ids {
                let skill = available
                    .iter()
                    .find(|skill| skill.view.id == id)
                    .ok_or_else(|| anyhow::anyhow!(UNAVAILABLE))?;
                let fresh = entry(skill)?;
                // Loading again refreshes the content fingerprint the turn uses.
                next.retain(|existing| existing.skill_id != id);
                next.push(fresh);
            }
            anyhow::ensure!(next.len() <= MAX_LOADED, LIMIT);
            Prepared {
                output: serde_json::to_value(SkillsLoadedResult {
                    loaded: next.clone(),
                    limit: MAX_LOADED as u32,
                })?,
                loaded: Some(next),
            }
        }
        "skills.unload" => {
            let input: SkillsUnloadInput = input(arguments)?;
            let next: Vec<_> =
                loaded.iter().filter(|e| !input.skill_ids.contains(&e.skill_id)).cloned().collect();
            Prepared {
                output: serde_json::to_value(SkillsLoadedResult {
                    loaded: next.clone(),
                    limit: MAX_LOADED as u32,
                })?,
                loaded: Some(next),
            }
        }
        "skills.list_loaded" => {
            let _: aura_agent_domain::EmptyToolInput = input(arguments)?;
            answer(serde_json::to_value(SkillsLoadedResult {
                loaded: loaded.clone(),
                limit: MAX_LOADED as u32,
            })?)
        }
        "skills.read_resource" => {
            let input: SkillsReadResourceInput = input(arguments)?;
            let loaded_entry = loaded
                .iter()
                .find(|entry| entry.skill_id == input.skill_id)
                .ok_or_else(|| anyhow::anyhow!(NOT_LOADED))?;
            let skill = ready(&runtime, workspace)?
                .into_iter()
                .find(|skill| skill.view.id == input.skill_id)
                .ok_or_else(|| anyhow::anyhow!(UNAVAILABLE))?;
            let read =
                aura_skills::read_resource(&skill.found.directory, &runtime.hashes, &input.path)
                    .map_err(|error| match error {
                        aura_skills::SkillResourceError::Package(_) => anyhow::anyhow!(UNAVAILABLE),
                        _ => anyhow::anyhow!(RESOURCE),
                    })?;
            // The text comes from the same pass that fingerprinted the folder: it is the trusted
            // content, and the content this turn loaded.
            anyhow::ensure!(
                Some(&read.fingerprint) == skill.view.trusted_fingerprint.as_ref()
                    && read.fingerprint == loaded_entry.fingerprint,
                CHANGED
            );
            answer(serde_json::to_value(SkillsReadResourceResult {
                skill_id: input.skill_id,
                path: aura_skills::resource_path(&input.path).unwrap_or(input.path),
                text: read.resource.unwrap_or_default(),
            })?)
        }
        _ => anyhow::bail!(ARGUMENTS),
    })
}

/// One loaded Skill as the next step's task state shows it: its instructions when it is still
/// enabled, trusted and unchanged since it was loaded, otherwise why it was left out.
pub(crate) struct LoadedForContext {
    pub entry: LoadedSkillEntry,
    pub body: Result<String, &'static str>,
}

pub(crate) fn for_context(
    runtime: &SkillRuntime,
    workspace: WorkspaceId,
    loaded: &[LoadedSkillEntry],
) -> Vec<LoadedForContext> {
    let listed = runtime.list(Some(workspace)).map(|listed| listed.2).unwrap_or_default();
    loaded
        .iter()
        .map(|entry| {
            let body = match listed.iter().find(|skill| skill.view.id == entry.skill_id) {
                None => Err("已移走或读取失败"),
                Some(skill) if skill.view.state == SkillState::ContentChanged => {
                    Err("内容在信任后变了")
                }
                Some(skill) if skill.view.state != SkillState::Ready => Err("已停用或撤销信任"),
                Some(skill) => match &skill.found.scanned {
                    Ok(package) if package.fingerprint == entry.fingerprint => {
                        Ok(package.body.clone())
                    }
                    _ => Err("内容在加载后变了"),
                },
            };
            LoadedForContext { entry: entry.clone(), body }
        })
        .collect()
}
