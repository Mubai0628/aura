//! What `skills.search` and `skills.load` answer: the usable Skills found by words, and one loaded
//! by its name, with where it is from only when another usable Skill has the same name.
use super::super::instructions;
use super::super::loading::{self, NotLoaded};
use super::support::{Fixture, skill, write};
use serde_json::json;

#[test]
fn a_skill_is_loaded_by_name_with_its_folder_files_and_instructions() {
    let f = Fixture::new();
    let folder = skill(&f.global(), "notes");
    write(&folder.join("scripts/run.ps1"), "Write-Output 1\n");
    let loaded = loading::load(&f.runtime, None, "notes", None).expect("read").expect("loaded");
    assert_eq!(loaded.name, "notes");
    assert_eq!(loaded.from, None, "the only Skill of its name");
    assert_eq!(loaded.description, "Test skill notes");
    assert_eq!(loaded.folder, f.view("notes")["path"].as_str().expect("folder"));
    assert_eq!(loaded.files, ["references/guide.md", "scripts/run.ps1"]);
    assert_eq!(loaded.more_files, 0);
    assert_eq!(loaded.instructions, "Use [the guide](references/guide.md).");
    // Names are lowercase; a name written otherwise still finds it.
    assert!(loading::load(&f.runtime, None, " Notes ", None).expect("read").is_ok());
}

#[test]
fn a_skill_switched_off_absent_or_untrusted_is_not_loaded() {
    let (f, workspace) = Fixture::with_workspace();
    skill(&f.global(), "notes");
    skill(&workspace.folder.join(".claude").join("skills"), "style");
    let notes = f.view_in(&workspace, "agent_global", "notes");
    f.dispatch(
        "agent.skills.set_enabled",
        json!({"id": notes["id"], "enabled": false, "expected_revision": f.listed_in(&workspace)["revision"]}),
    )
    .expect("off");
    let load = |name| loading::load(&f.runtime, Some(workspace.id), name, None).expect("read");
    assert_eq!(load("notes"), Err(NotLoaded::Unavailable), "switched off");
    assert_eq!(load("absent"), Err(NotLoaded::Unavailable));
    assert_eq!(load("style"), Err(NotLoaded::Unavailable), "the workspace is not trusted");
    f.dispatch(
        "agent.skills.trust_workspace",
        json!({"workspace_id": workspace.id, "expected_revision": f.listed_in(&workspace)["revision"]}),
    )
    .expect("trusted");
    assert!(load("style").is_ok());
}

#[test]
fn two_skills_with_one_name_are_told_apart_by_where_they_are_from() {
    let f = Fixture::new();
    skill(&f.global(), "notes");
    skill(&f.claude(), "notes");
    skill(&f.global().join("more"), "notes");
    let load = |from| loading::load(&f.runtime, None, "notes", from).expect("read");
    assert_eq!(load(None), Err(NotLoaded::Ambiguous));
    assert_eq!(load(Some("agent_global")), Err(NotLoaded::Ambiguous), "two there");
    let claude = load(Some("claude_global")).expect("one there");
    assert_eq!(claude.from.as_deref(), Some("claude_global"));
    assert!(claude.folder.contains(".claude"), "{}", claude.folder);
    let nested = load(Some("agent_global:more/notes")).expect("by its folder");
    assert_eq!(nested.from.as_deref(), Some("agent_global:more/notes"));
    assert_eq!(load(Some("workspace")), Err(NotLoaded::Unavailable));
    // The search names each the same way, and so does the list of names.
    let (found, truncated) = loading::search(&f.runtime, None, "", 20).expect("found");
    assert!(!truncated);
    let froms: Vec<_> = found.iter().map(|skill| skill.from.clone()).collect();
    assert_eq!(
        froms,
        [
            Some("agent_global:more/notes".to_owned()),
            Some("agent_global:notes".to_owned()),
            Some("claude_global".to_owned())
        ]
    );
    let list = instructions::for_turn(&f.runtime, None).expect("decided").list.expect("a list");
    for from in froms.into_iter().flatten() {
        assert!(list.contains(&format!("- notes [{from}]")), "{list}");
    }
}

#[test]
fn search_finds_every_word_in_a_name_or_description_up_to_its_limit() {
    let f = Fixture::new();
    for name in ["alpha", "beta", "gamma"] {
        skill(&f.global(), name);
    }
    let names = |query, limit| {
        let (found, truncated) = loading::search(&f.runtime, None, query, limit).expect("found");
        (found.into_iter().map(|skill| skill.name).collect::<Vec<_>>(), truncated)
    };
    assert_eq!(names("", 20), (vec!["alpha".into(), "beta".into(), "gamma".into()], false));
    assert_eq!(names("TEST beta", 20), (vec!["beta".into()], false));
    assert_eq!(names("", 2), (vec!["alpha".into(), "beta".into()], true));
    assert_eq!(names("delta", 20), (vec![], false));
    let (found, _) = loading::search(&f.runtime, None, "gamma", 20).expect("found");
    assert_eq!(found[0].description, "Test skill gamma");
    assert_eq!(found[0].from, None);
}

#[test]
fn a_chosen_skill_is_given_whole_as_the_users_choice_for_its_message() {
    let f = Fixture::new();
    skill(&f.global(), "notes");
    let (_, _, skills) = f.runtime.list(None).expect("listed");
    let notes = skills.iter().find(|s| s.view.relative == "notes").expect("notes");
    let text = instructions::chosen_text(notes).expect("usable");
    assert!(text.starts_with("Chosen Skill \"notes\" [agent_global]"), "{text}");
    assert!(text.contains("The user chose this Skill with this message"), "{text}");
    // Models loaded the chosen Skill again with skills.load: the text says it is already here.
    assert!(text.contains("do not load it again"), "{text}");
    assert!(text.contains("references/guide.md"), "{text}");
    assert!(text.ends_with("Use [the guide](references/guide.md)."), "{text}");
    assert!(instructions::pinned_text(notes).expect("usable").starts_with("Pinned Skill"));
}
