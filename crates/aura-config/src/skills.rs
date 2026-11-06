//! The user's choices in `skills.json`: each Skill switched on or off (and the content trusted
//! alone, the earlier way), the workspaces whose Skills the user trusted, and the Skills pinned
//! into every request. Nothing about a Skill's content is stored here; the folders are the source.
use crate::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, SettingsFacade, SettingsFacadeError,
};
use aura_domain_common::{SkillInstanceId, WorkspaceId};
use aura_skills::{SkillSetting, SkillSource, skill_id};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const SKILLS: &str = "skills";
const TRUSTED_WORKSPACES: &str = "trusted_workspaces";
const PINNED: &str = "pinned";

/// The choices `skills.json` holds, as one value to change together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillChoices {
    pub skills: Vec<SkillSetting>,
    /// Workspaces whose `.agents\skills` and `.claude\skills` the user trusted.
    pub trusted_workspaces: Vec<WorkspaceId>,
    /// Skills whose instructions go in full into every request, in the order they were pinned.
    pub pinned: Vec<SkillInstanceId>,
}

/// The choices in force. When `skills.json` cannot be used, no Skill is offered to the model
/// and `issue` says why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillRegistry {
    pub skills: Vec<SkillSetting>,
    pub trusted_workspaces: Vec<WorkspaceId>,
    pub pinned: Vec<SkillInstanceId>,
    pub revision: u64,
    pub issue: Option<String>,
}

pub(crate) fn validate_skills_document(
    document: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    choices(&document.values).map(|_| ())
}

/// One list of identities under `key`, each named once.
fn unique_list<T>(
    values: &BTreeMap<String, Value>,
    key: &str,
) -> Result<Vec<T>, ConfigValidationError>
where
    T: serde::de::DeserializeOwned + Ord + Copy + std::fmt::Display,
{
    let Some(items) = values.get(key) else { return Ok(Vec::new()) };
    let list: Vec<T> = serde_json::from_value(items.clone())
        .map_err(|error| invalid(format!("{key} 格式不对：{error}")))?;
    let mut seen = BTreeSet::new();
    if let Some(twice) = list.iter().find(|item| !seen.insert(**item)) {
        return Err(invalid(format!("{key} 里 {twice} 出现了两次")));
    }
    Ok(list)
}

fn choices(values: &BTreeMap<String, Value>) -> Result<SkillChoices, ConfigValidationError> {
    Ok(SkillChoices {
        skills: settings(values)?,
        trusted_workspaces: unique_list(values, TRUSTED_WORKSPACES)?,
        pinned: unique_list(values, PINNED)?,
    })
}

fn put(values: &mut BTreeMap<String, Value>, key: &str, items: Vec<Value>) {
    if items.is_empty() {
        values.remove(key);
    } else {
        values.insert(key.to_owned(), Value::Array(items));
    }
}

fn to_values<T: serde::Serialize>(items: &[T]) -> Vec<Value> {
    items.iter().map(|item| serde_json::to_value(item).unwrap_or(Value::Null)).collect()
}

fn invalid(message: impl Into<String>) -> ConfigValidationError {
    ConfigValidationError::Semantic(format!("skills.json: {}", message.into()))
}

fn relative_ok(relative: &str) -> bool {
    !relative.is_empty()
        && relative.split('/').count() <= 2
        && relative.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.starts_with('.')
                && !part.chars().any(|c| c.is_control() || matches!(c, '\\' | ':'))
        })
}

fn settings(values: &BTreeMap<String, Value>) -> Result<Vec<SkillSetting>, ConfigValidationError> {
    if let Some(field) =
        values.keys().find(|key| ![SKILLS, TRUSTED_WORKSPACES, PINNED].contains(&key.as_str()))
    {
        return Err(ConfigValidationError::UnknownModuleField {
            module: ConfigModule::Skills,
            field: field.clone(),
        });
    }
    let Some(items) = values.get(SKILLS) else { return Ok(Vec::new()) };
    let skills: Vec<SkillSetting> = serde_json::from_value(items.clone())
        .map_err(|error| invalid(format!("skills 格式不对：{error}")))?;
    let mut seen = BTreeSet::new();
    for skill in &skills {
        if !seen.insert(skill.id) {
            return Err(invalid(format!("{} 出现了两次", skill.id)));
        }
        if !relative_ok(&skill.relative) {
            return Err(invalid(format!(
                "{} 的 relative 不是来源目录里的一层或两层文件夹",
                skill.id
            )));
        }
        if skill.source == SkillSource::Plugin {
            return Err(invalid(format!(
                "{} 是插件里的 Skill，它随插件信任和启用，不写在这里",
                skill.id
            )));
        }
        if skill.source.in_workspace() != skill.workspace_id.is_some() {
            return Err(invalid(format!(
                "{} 的 workspace_id 只用于工作区 Skill，且必须给出",
                skill.id
            )));
        }
        if skill.id != skill_id(skill.source, skill.workspace_id, &skill.relative) {
            return Err(invalid(format!("{} 与它的来源和路径不符", skill.id)));
        }
        if skill
            .trusted_fingerprint
            .as_ref()
            .is_some_and(|f| f.len() != 64 || !f.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(invalid(format!(
                "{} 的 trusted_fingerprint 必须是 64 位十六进制",
                skill.id
            )));
        }
    }
    Ok(skills)
}

impl SettingsFacade {
    /// The choices in force. When the domain is not healthy, or the served document is a
    /// last-known-good fallback rather than the file on disk, nothing is offered: a failed edit
    /// may have been the user switching a Skill off or withdrawing trust.
    pub fn skill_registry(&self) -> SkillRegistry {
        let module = ConfigModule::Skills;
        let snapshot = match self.snapshot(module) {
            Ok(snapshot)
                if self.health(module).state == crate::DomainHealthState::Ready
                    && crate::memory::current_input_matches(self, module, &snapshot) =>
            {
                snapshot
            }
            _ => {
                return SkillRegistry {
                    issue: Some(
                        "skills.json 当前无法使用，所有 Skill 都不提供给模型。请在设置里重试、恢复上一版有效配置或重置。".into(),
                    ),
                    ..SkillRegistry::default()
                };
            }
        };
        let revision = snapshot.document.revision;
        match choices(&snapshot.document.values) {
            Ok(choices) => SkillRegistry {
                skills: choices.skills,
                trusted_workspaces: choices.trusted_workspaces,
                pinned: choices.pinned,
                revision,
                issue: None,
            },
            Err(error) => SkillRegistry {
                revision,
                issue: Some(format!("{error}；所有 Skill 都不提供给模型")),
                ..SkillRegistry::default()
            },
        }
    }

    /// Changes the choices as they are at `expected_revision`, all in one write. A concurrent
    /// change is a conflict for the caller to reread, never a silent overwrite; a file that
    /// cannot be read is not changed.
    pub fn update_skill_choices(
        &self,
        expected_revision: u64,
        change: impl FnOnce(&mut SkillChoices),
    ) -> Result<u64, SettingsFacadeError> {
        let mut readable = true;
        let snapshot = self.update(ConfigModule::Skills, expected_revision, |values| {
            let Ok(mut current) = choices(values) else {
                readable = false;
                return;
            };
            change(&mut current);
            put(values, SKILLS, to_values(&current.skills));
            put(values, TRUSTED_WORKSPACES, to_values(&current.trusted_workspaces));
            put(values, PINNED, to_values(&current.pinned));
        })?;
        if !readable {
            return Err(SettingsFacadeError::Validation {
                module: ConfigModule::Skills,
                source: invalid("当前内容无法读取，没有修改"),
            });
        }
        Ok(snapshot.document.revision)
    }

    /// Replaces the choices at `expected_revision`. A concurrent change is a conflict for the
    /// caller to reread, never a silent overwrite.
    pub fn replace_skill_registry(
        &self,
        expected_revision: u64,
        skills: &[SkillSetting],
    ) -> Result<u64, SettingsFacadeError> {
        let items = to_values(skills);
        let snapshot = self.update(ConfigModule::Skills, expected_revision, |values| {
            put(values, SKILLS, items);
        })?;
        Ok(snapshot.document.revision)
    }
}
