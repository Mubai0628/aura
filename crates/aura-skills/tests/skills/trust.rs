//! What a found Skill needs before the model may use it. A Skill in the user's own folders is
//! usable as found; one in a workspace waits until the user trusts that workspace's Skills once.
//! An earlier trust of a single workspace Skill at its fingerprint still counts for that Skill.
use aura_domain_common::WorkspaceId;
use aura_skills::{
    FileHashCache, FoundSkill, SkillSetting, SkillSource, SkillState, scan_skill, skill_id, view,
};
use std::path::Path;

fn found(root: &Path, name: &str, body: &str) -> FoundSkill {
    let directory = root.join(name);
    std::fs::create_dir_all(&directory).expect("folder");
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill {name}\n---\n\n{body}\n"),
    )
    .expect("SKILL.md");
    let scanned = scan_skill(&directory, &FileHashCache::default());
    FoundSkill { relative: name.into(), directory, scanned }
}

fn setting(source: SkillSource, workspace: Option<WorkspaceId>, relative: &str) -> SkillSetting {
    SkillSetting {
        id: skill_id(source, workspace, relative),
        source,
        workspace_id: workspace,
        relative: relative.into(),
        enabled: true,
        trusted_fingerprint: None,
        available_for_subagents: false,
    }
}

#[test]
fn a_skill_in_the_users_own_folders_is_ready_as_found_and_after_it_changes() {
    let root = tempfile::tempdir().expect("root");
    for source in [
        SkillSource::AuraStore,
        SkillSource::AuraBuiltIn,
        SkillSource::AgentGlobal,
        SkillSource::ClaudeGlobal,
    ] {
        let first = found(root.path(), "notes", "First.");
        let shown = view(&first, source, None, None, false);
        assert_eq!(shown.state, SkillState::Ready, "{source:?}");
        assert!(shown.enabled, "{source:?}");
        let changed = found(root.path(), "notes", "Changed.");
        assert_eq!(view(&changed, source, None, None, false).state, SkillState::Ready);
        let mut off = setting(source, None, "notes");
        off.enabled = false;
        assert_eq!(view(&changed, source, None, Some(&off), false).state, SkillState::Disabled);
    }
}

#[test]
fn a_workspace_skill_waits_for_its_workspace_to_be_trusted_once() {
    let root = tempfile::tempdir().expect("root");
    let workspace = Some(WorkspaceId::new());
    for source in [SkillSource::Workspace, SkillSource::ClaudeWorkspace] {
        let skill = found(root.path(), "style", "Write plainly.");
        let untrusted = view(&skill, source, workspace, None, false);
        assert_eq!(untrusted.state, SkillState::NotTrusted, "{source:?}");
        assert!(untrusted.enabled, "on by default; the workspace trust is what it waits for");
        assert_eq!(view(&skill, source, workspace, None, true).state, SkillState::Ready);
        let changed = found(root.path(), "style", "Write very plainly.");
        assert_eq!(
            view(&changed, source, workspace, None, true).state,
            SkillState::Ready,
            "a trusted workspace's Skill stays usable when it changes"
        );
    }
}

#[test]
fn an_earlier_trust_of_one_workspace_skill_still_counts_for_that_skill_only() {
    let root = tempfile::tempdir().expect("root");
    let workspace = Some(WorkspaceId::new());
    let skill = found(root.path(), "style", "Write plainly.");
    let mut trusted = setting(SkillSource::Workspace, workspace, "style");
    trusted.trusted_fingerprint = skill.scanned.as_ref().ok().map(|p| p.fingerprint.clone());
    assert_eq!(
        view(&skill, SkillSource::Workspace, workspace, Some(&trusted), false).state,
        SkillState::Ready
    );
    let changed = found(root.path(), "style", "Something else.");
    assert_eq!(
        view(&changed, SkillSource::Workspace, workspace, Some(&trusted), false).state,
        SkillState::ContentChanged
    );
    let other = found(root.path(), "other", "Other.");
    assert_eq!(
        view(&other, SkillSource::Workspace, workspace, None, false).state,
        SkillState::NotTrusted
    );
}
