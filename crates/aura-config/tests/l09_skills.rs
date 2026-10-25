//! `skills.json`: the user's choices per Skill, revision-guarded, and a file that cannot be
//! read offers no Skill instead of falling back to older choices.
use aura_config::{AuraPaths, ConfigModule, SettingsFacade, SettingsFacadeError};
use aura_domain_common::WorkspaceId;
use aura_skills::{SkillSetting, SkillSource, skill_id};

fn facade() -> (tempfile::TempDir, AuraPaths, std::sync::Arc<SettingsFacade>) {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    (temp, paths, facade)
}

fn setting(source: SkillSource, workspace: Option<WorkspaceId>, relative: &str) -> SkillSetting {
    SkillSetting {
        id: skill_id(source, workspace, relative),
        source,
        workspace_id: workspace,
        relative: relative.into(),
        enabled: true,
        trusted_fingerprint: Some("c".repeat(64)),
        available_for_subagents: false,
    }
}

#[test]
fn choices_survive_a_restart_at_each_revision_and_a_stale_one_is_refused() {
    let (_temp, paths, facade) = facade();
    let empty = facade.skill_registry();
    assert!(empty.skills.is_empty() && empty.issue.is_none());
    let workspace = WorkspaceId::new();
    let skills = vec![
        setting(SkillSource::AgentGlobal, None, "notes"),
        setting(SkillSource::Workspace, Some(workspace), "writing/style"),
    ];
    let revision = facade.replace_skill_registry(empty.revision, &skills).expect("written");
    let reopened = SettingsFacade::open(paths).expect("reopened").skill_registry();
    assert_eq!(reopened.skills, skills);
    assert_eq!(reopened.revision, revision);
    let stale = facade.replace_skill_registry(empty.revision, &[]).expect_err("stale");
    assert!(matches!(stale, SettingsFacadeError::RevisionConflict { .. }), "{stale:?}");
}

#[test]
fn inconsistent_choices_are_refused_before_they_are_written() {
    let (_temp, _paths, facade) = facade();
    let start = facade.skill_registry().revision;
    let mut wrong_id = setting(SkillSource::AgentGlobal, None, "notes");
    wrong_id.id = skill_id(SkillSource::AgentGlobal, None, "other");
    let mut outside = setting(SkillSource::AgentGlobal, None, "notes");
    outside.relative = "../notes".into();
    outside.id = skill_id(SkillSource::AgentGlobal, None, "../notes");
    let no_workspace = SkillSetting {
        workspace_id: None,
        ..setting(SkillSource::Workspace, Some(WorkspaceId::new()), "a")
    };
    let mut short = setting(SkillSource::AuraStore, None, "pdf");
    short.trusted_fingerprint = Some("abc".into());
    for bad in [wrong_id, outside, no_workspace, short] {
        assert!(
            facade.replace_skill_registry(start, std::slice::from_ref(&bad)).is_err(),
            "{bad:?}"
        );
    }
    let twice = setting(SkillSource::AuraStore, None, "pdf");
    assert!(facade.replace_skill_registry(start, &[twice.clone(), twice]).is_err(), "duplicate id");
    assert!(facade.skill_registry().skills.is_empty(), "nothing was written");
}

#[test]
fn a_hand_edited_file_that_cannot_be_read_offers_no_skill() {
    let (_temp, paths, facade) = facade();
    drop(facade);
    for bad in [
        r#"{"schema_version":1,"revision":3,"extra":1}"#,
        r#"{"schema_version":1,"revision":3,"skills":[{"id":"skl_01ARZ3NDEKTSV4RRFFQ69G5FAV","source":"agent_global","relative":"notes","enabled":true}]}"#,
        r#"{"schema_version":1,"revision":3,"skills":[{"source":"agent_global","relative":"notes"}]}"#,
    ] {
        std::fs::write(paths.config_path(ConfigModule::Skills), bad).expect("hand edit");
        let reopened = SettingsFacade::open(paths.clone()).expect("other domains stay available");
        let registry = reopened.skill_registry();
        assert!(registry.skills.is_empty(), "{bad}");
        assert!(registry.issue.as_deref().is_some_and(|i| i.contains("都不提供给模型")), "{bad}");
        assert_eq!(
            std::fs::read_to_string(paths.config_path(ConfigModule::Skills)).expect("kept"),
            bad
        );
    }
}
