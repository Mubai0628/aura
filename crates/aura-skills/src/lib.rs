//! Agent Skills. A Skill is a folder with `SKILL.md` (YAML front matter and Markdown
//! instructions) and static files. Aura reads it as the user's content: context the model may
//! load, never an executable tool and never a grant.
mod archive;
mod catalog;
mod frontmatter;
mod hash_cache;
mod package;
mod views;

pub use archive::{
    ArchiveRules, MAX_ARCHIVE_BYTES, SKILL_ARCHIVE, SkillArchiveError, unpack_archive,
    unpack_skill_archive,
};
pub use catalog::{FoundSkill, MAX_SKILLS_PER_ROOT, RootStatus, root_status, scan_root};
pub use frontmatter::{
    MAX_DESCRIPTION_CHARS, MAX_NAME_CHARS, ParsedSkill, SkillFrontmatter, parse_skill_markdown,
};
pub use hash_cache::{FileHashCache, FileIdentity};
pub use package::{
    MAX_RESOURCE_BYTES, MAX_SKILL_BYTES, MAX_SKILL_FILES, MAX_SKILL_MD_BYTES, SKILL_FILE,
    SkillFile, SkillPackage, SkillPackageError, SkillResourceError, read_resource, resource_path,
    scan_skill,
};
pub use views::*;

use aura_domain_common::{SkillInstanceId, WorkspaceId};

/// The stable identity of a Skill folder: its source, the workspace of a workspace Skill, and
/// its path under the source's root. Two folders with the same name in different places are
/// different Skills.
#[must_use]
pub fn skill_id(
    source: SkillSource,
    workspace: Option<WorkspaceId>,
    relative: &str,
) -> SkillInstanceId {
    let material = format!(
        "aura.skill.instance-id.v1\0{}\0{}\0{}",
        source.key(),
        workspace.map(|id| id.to_string()).unwrap_or_default(),
        relative.to_ascii_lowercase()
    );
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&blake3::hash(material.as_bytes()).as_bytes()[..16]);
    SkillInstanceId::from_bytes(bytes)
}

/// Whether a Skill is switched on when the user has not chosen: one imported into Aura is, one
/// found elsewhere is not.
#[must_use]
pub const fn enabled_by_default(source: SkillSource) -> bool {
    matches!(source, SkillSource::AuraStore)
}

/// What a found Skill is under its saved choices.
#[must_use]
pub fn view(
    found: &FoundSkill,
    source: SkillSource,
    workspace: Option<WorkspaceId>,
    setting: Option<&SkillSetting>,
) -> SkillView {
    let enabled = setting.map_or(enabled_by_default(source), |s| s.enabled);
    let trusted_fingerprint = setting.and_then(|s| s.trusted_fingerprint.clone());
    let mut view = SkillView {
        id: skill_id(source, workspace, &found.relative),
        source,
        plugin: None,
        workspace_id: workspace,
        relative: found.relative.clone(),
        path: aura_domain_common::display_path(&found.directory),
        state: SkillState::Invalid,
        enabled,
        name: None,
        description: None,
        license: None,
        compatibility: None,
        allowed_tools: Vec::new(),
        fingerprint: None,
        trusted_fingerprint: trusted_fingerprint.clone(),
        warnings: Vec::new(),
        missing_references: Vec::new(),
        problems: Vec::new(),
        file_count: 0,
        bytes: 0,
    };
    match &found.scanned {
        Err(error) => {
            view.state = match error {
                SkillPackageError::MissingSkillFile | SkillPackageError::Invalid(_) => {
                    SkillState::Invalid
                }
                _ => SkillState::Unavailable,
            };
            view.problems = match error {
                SkillPackageError::Invalid(problems) => problems.clone(),
                other => vec![other.to_string()],
            };
        }
        Ok(package) => {
            let front = &package.frontmatter;
            view.name = Some(front.name.clone());
            view.description = Some(front.description.clone());
            view.license.clone_from(&front.license);
            view.compatibility.clone_from(&front.compatibility);
            view.allowed_tools.clone_from(&front.allowed_tools);
            view.fingerprint = Some(package.fingerprint.clone());
            view.warnings.clone_from(&package.warnings);
            view.missing_references.clone_from(&package.missing_references);
            view.file_count = u32::try_from(package.files.len()).unwrap_or(u32::MAX);
            view.bytes = package.bytes;
            view.state = if !enabled {
                SkillState::Disabled
            } else {
                match trusted_fingerprint {
                    None => SkillState::NotTrusted,
                    Some(trusted) if trusted == package.fingerprint => SkillState::Ready,
                    Some(_) => SkillState::ContentChanged,
                }
            };
        }
    }
    view
}
