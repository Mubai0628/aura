//! What `skills.search` and `skills.load` answer, from the Skills usable now: the ones found by
//! words in their name or description, and one loaded by its name. Where a Skill is from is
//! named only when another usable Skill has its name, in the form the list of names uses.
use super::instructions::{from_among, place, sendable, usable};
use super::runtime::{LocatedSkill, SkillRuntime};
use aura_domain_common::WorkspaceId;
use aura_skills::SKILL_FILE;
use std::path::PathBuf;

/// The most files a loaded Skill's answer names; the rest are counted.
pub const LISTED_FILES: usize = 200;

/// One usable Skill a search found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Found {
    pub name: String,
    pub from: Option<String>,
    pub description: String,
}

/// A usable Skill as `skills.load` gives it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Content {
    pub name: String,
    pub from: Option<String>,
    pub description: String,
    /// The Skill's folder on this computer.
    pub folder: String,
    /// The folder's other files, relative and `/`-separated, in name order.
    pub files: Vec<String>,
    /// Files past [`LISTED_FILES`] that are not named.
    pub more_files: u32,
    /// What the Skill says it needs; it allows nothing.
    pub allowed_tools: Vec<String>,
    pub instructions: String,
}

/// Why `skills.load` loaded nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotLoaded {
    /// No usable Skill has that name there.
    Unavailable,
    /// More than one does; where it is from tells them apart.
    Ambiguous,
}

fn matches(skill: &LocatedSkill, words: &[String]) -> bool {
    let name = skill.view.name.clone().unwrap_or_default().to_lowercase();
    let description = skill.view.description.clone().unwrap_or_default().to_lowercase();
    words.iter().all(|word| name.contains(word) || description.contains(word))
}

/// The usable Skills whose name or description holds every word, in name order, at most
/// `limit`, and whether more matched.
pub fn search(
    runtime: &SkillRuntime,
    workspace: Option<WorkspaceId>,
    query: &str,
    limit: usize,
) -> anyhow::Result<(Vec<Found>, bool)> {
    let (_, _, skills) = runtime.list(workspace)?;
    let usable = usable(&skills);
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let found: Vec<Found> = usable
        .iter()
        .filter(|skill| matches(skill, &words))
        .map(|skill| Found {
            name: skill.view.name.clone().unwrap_or_default(),
            from: from_among(&usable, &skill.view),
            description: skill.view.description.clone().unwrap_or_default(),
        })
        .collect();
    let truncated = found.len() > limit;
    Ok((found.into_iter().take(limit).collect(), truncated))
}

/// The usable Skill named `name`; `from` is its place (`agent_global`, `plugin:<name>`…), or its
/// place and folder (`agent_global:<folder>`) when the place has two.
pub fn load(
    runtime: &SkillRuntime,
    workspace: Option<WorkspaceId>,
    name: &str,
    from: Option<&str>,
) -> anyhow::Result<Result<Content, NotLoaded>> {
    let (_, _, skills) = runtime.list(workspace)?;
    let usable = usable(&skills);
    let name = name.trim().to_ascii_lowercase();
    let from = from.map(str::trim).filter(|from| !from.is_empty());
    let named: Vec<&LocatedSkill> = usable
        .iter()
        .copied()
        .filter(|skill| skill.view.name.as_deref() == Some(name.as_str()))
        .filter(|skill| {
            from.is_none_or(|from| {
                let at = place(&skill.view);
                from == at || from == format!("{at}:{}", skill.view.relative)
            })
        })
        .collect();
    Ok(match named.as_slice() {
        [skill] => content(skill, from_among(&usable, &skill.view)).ok_or(NotLoaded::Unavailable),
        [] => Err(NotLoaded::Unavailable),
        _ => Err(NotLoaded::Ambiguous),
    })
}

/// The folders of the Skills usable now, which the file tools read without asking.
pub fn usable_folders(
    runtime: &SkillRuntime,
    workspace: Option<WorkspaceId>,
) -> anyhow::Result<Vec<PathBuf>> {
    let (_, _, skills) = runtime.list(workspace)?;
    Ok(usable(&skills).iter().map(|skill| skill.found.directory.clone()).collect())
}

fn content(skill: &LocatedSkill, from: Option<String>) -> Option<Content> {
    let package = skill.found.scanned.as_ref().ok()?;
    let files: Vec<String> =
        package.files.iter().map(|f| f.path.clone()).filter(|p| p != SKILL_FILE).collect();
    let more_files = u32::try_from(files.len().saturating_sub(LISTED_FILES)).unwrap_or(u32::MAX);
    Some(Content {
        name: package.frontmatter.name.clone(),
        from,
        description: package.frontmatter.description.clone(),
        folder: skill.view.path.clone(),
        files: files.into_iter().take(LISTED_FILES).collect(),
        more_files,
        allowed_tools: package.frontmatter.allowed_tools.clone(),
        instructions: sendable(package.body.trim_end()),
    })
}
