//! `skills.json` also keeps the workspaces whose Skills the user trusted and the Skills pinned
//! into every request; an older file without them still reads.
use aura_config::{AuraPaths, ConfigModule, SettingsFacade};
use aura_domain_common::{SkillInstanceId, WorkspaceId};
use aura_skills::{SkillSetting, SkillSource, skill_id};

fn facade() -> (tempfile::TempDir, AuraPaths, std::sync::Arc<SettingsFacade>) {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    (temp, paths, facade)
}

#[test]
fn trusted_workspaces_and_pins_are_kept_beside_the_skills_and_survive_a_restart() {
    let (_temp, paths, facade) = facade();
    let start = facade.skill_registry().revision;
    let workspace = WorkspaceId::new();
    let pinned = skill_id(SkillSource::ClaudeGlobal, None, "review");
    let revision = facade
        .update_skill_choices(start, |choices| {
            choices.trusted_workspaces.push(workspace);
            choices.pinned.push(pinned);
        })
        .expect("written");
    let reopened = SettingsFacade::open(paths).expect("reopened").skill_registry();
    assert_eq!(reopened.revision, revision);
    assert_eq!(reopened.trusted_workspaces, [workspace]);
    assert_eq!(reopened.pinned, [pinned]);
    assert!(reopened.skills.is_empty() && reopened.issue.is_none());
    let stale = facade.update_skill_choices(start, |choices| choices.pinned.clear());
    assert!(stale.is_err(), "a stale revision is refused");
}

#[test]
fn a_workspace_or_pin_named_twice_is_refused_before_it_is_written() {
    let (_temp, _paths, facade) = facade();
    let start = facade.skill_registry().revision;
    let workspace = WorkspaceId::new();
    assert!(
        facade
            .update_skill_choices(start, |c| c.trusted_workspaces.extend([workspace, workspace]))
            .is_err()
    );
    let pin = SkillInstanceId::new();
    assert!(facade.update_skill_choices(start, |c| c.pinned.extend([pin, pin])).is_err());
    let registry = facade.skill_registry();
    assert!(registry.trusted_workspaces.is_empty() && registry.pinned.is_empty());
}

#[test]
fn a_skill_in_a_workspace_claude_folder_names_its_workspace() {
    let (_temp, _paths, facade) = facade();
    let start = facade.skill_registry().revision;
    let workspace = WorkspaceId::new();
    let setting = |workspace_id: Option<WorkspaceId>| SkillSetting {
        id: skill_id(SkillSource::ClaudeWorkspace, workspace_id, "notes"),
        source: SkillSource::ClaudeWorkspace,
        workspace_id,
        relative: "notes".into(),
        enabled: false,
        trusted_fingerprint: None,
        available_for_subagents: false,
    };
    assert!(facade.replace_skill_registry(start, &[setting(None)]).is_err(), "no workspace");
    let written = facade.replace_skill_registry(start, &[setting(Some(workspace))]);
    assert!(written.is_ok(), "{written:?}");
}

#[test]
fn an_older_file_without_the_new_lists_still_reads() {
    let (_temp, paths, facade) = facade();
    drop(facade);
    let id = skill_id(SkillSource::AgentGlobal, None, "notes");
    let older = format!(
        r#"{{"schema_version":1,"revision":3,"skills":[{{"id":"{id}","source":"agent_global","relative":"notes","enabled":false}}]}}"#
    );
    std::fs::write(paths.config_path(ConfigModule::Skills), older).expect("older file");
    let registry = SettingsFacade::open(paths).expect("reopened").skill_registry();
    assert!(registry.issue.is_none(), "{:?}", registry.issue);
    assert_eq!(registry.skills.len(), 1);
    assert!(registry.trusted_workspaces.is_empty() && registry.pinned.is_empty());
}
