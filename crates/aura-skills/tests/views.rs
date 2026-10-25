//! A Skill's folder is shown, and opened, in its ordinary spelling. Core holds a workspace's
//! folders in Windows' verbatim spelling (`\\?\C:\…`), which the shell does not open.
use aura_skills::{FoundSkill, SkillPackageError, SkillSource, view};
use std::path::PathBuf;

fn shown(directory: &str) -> String {
    let found = FoundSkill {
        relative: "pdf".into(),
        directory: PathBuf::from(directory),
        scanned: Err(SkillPackageError::MissingSkillFile),
    };
    view(&found, SkillSource::Workspace, None, None).path
}

#[test]
fn a_skill_folder_is_named_in_its_ordinary_spelling() {
    assert_eq!(shown(r"\\?\C:\work\.agents\skills\pdf"), r"C:\work\.agents\skills\pdf");
    assert_eq!(shown(r"\\?\UNC\host\share\skills\pdf"), r"\\host\share\skills\pdf");
    assert_eq!(shown(r"C:\plain\pdf"), r"C:\plain\pdf");
}
