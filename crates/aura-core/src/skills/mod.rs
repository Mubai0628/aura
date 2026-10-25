//! Skills in Core: where Skill folders are, Settings → Skill, and what the model may load.
//! A Skill is the user's content; trusting it never grants a tool or a permission.
mod import;
mod manage;
mod runtime;

pub(crate) use manage::SkillContext;
pub(crate) use runtime::{LocatedSkill, SkillRuntime, preread_in_background, warm_in_background};

#[cfg(windows)]
pub(crate) fn reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests;
