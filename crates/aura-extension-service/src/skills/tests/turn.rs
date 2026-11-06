//! What a turn's instructions carry about Skills: the pinned Skills whole, then the names of the
//! other usable ones. The run's first step decides both; its later steps read the pinned Skills
//! again from their folders.
use super::super::instructions::{self, TurnSkills};
use super::support::{Fixture, skill};
use aura_domain_common::SkillInstanceId;
use serde_json::{Value, json};

fn id(view: &Value) -> SkillInstanceId {
    serde_json::from_value(view["id"].clone()).expect("id")
}

fn listed_names(list: &str) -> Vec<&str> {
    list.lines().filter(|line| line.starts_with("- ")).collect()
}

#[test]
fn the_list_names_each_usable_skill_in_name_order_and_leaves_out_the_pinned() {
    let f = Fixture::new();
    skill(&f.global(), "zeta");
    skill(&f.global(), "alpha");
    skill(&f.claude(), "alpha");
    skill(&f.global(), "mid");
    let mid = f.view("mid");
    f.dispatch(
        "agent.skills.set_pinned",
        json!({"id": mid["id"], "pinned": true, "expected_revision": f.revision()}),
    )
    .expect("pinned");
    let turn = instructions::for_turn(&f.runtime, None).expect("decided");
    assert_eq!(turn.pinned, [id(&mid)]);
    let list = turn.list.expect("a list");
    assert_eq!(
        listed_names(&list),
        ["- alpha [agent_global]", "- alpha [claude_global]", "- zeta"]
    );
    assert!(list.contains("skills.load") && list.contains("skills.search"), "{list}");
}

#[test]
fn a_switched_off_or_untrusted_skill_is_not_listed_and_no_skill_means_no_list() {
    let (f, workspace) = Fixture::with_workspace();
    let none = instructions::for_turn(&f.runtime, Some(workspace.id)).expect("decided");
    assert_eq!(none, TurnSkills::default());
    skill(&f.global(), "notes");
    skill(&workspace.folder.join(".agents").join("skills"), "style");
    let notes = f.view_in(&workspace, "agent_global", "notes");
    f.dispatch(
        "agent.skills.set_enabled",
        json!({"id": notes["id"], "enabled": false, "expected_revision": f.listed_in(&workspace)["revision"]}),
    )
    .expect("off");
    let decided = instructions::for_turn(&f.runtime, Some(workspace.id)).expect("decided");
    assert_eq!(decided.list, None, "off and untrusted: nothing to name");
    f.dispatch(
        "agent.skills.trust_workspace",
        json!({"workspace_id": workspace.id, "expected_revision": f.listed_in(&workspace)["revision"]}),
    )
    .expect("trusted");
    let list = instructions::for_turn(&f.runtime, Some(workspace.id))
        .expect("decided")
        .list
        .expect("a list");
    assert_eq!(listed_names(&list), ["- style"], "{list}");
}

#[test]
fn a_long_list_stops_at_its_limit_from_the_end_and_says_how_many_more() {
    let names: Vec<(String, Option<String>)> =
        (0..40).map(|i| (format!("skill-{i:02}"), None)).collect();
    let list = instructions::list_text(&names, 120).expect("a list");
    let shown = listed_names(&list);
    assert!(!shown.is_empty() && shown.len() < 40, "{list}");
    assert_eq!(shown[0], "- skill-00", "the start is kept");
    assert!(list.contains(&format!("{} more", 40 - shown.len())), "{list}");
    assert!(aura_context_runtime::character_class_tokens(&list) <= 120, "{list}");
    assert_eq!(instructions::list_text(&names, 120), Some(list), "the same bytes again");
    assert_eq!(instructions::list_text(&[], 120), None);
}

#[test]
fn a_pinned_skill_is_given_whole_with_its_folder_and_files_and_leaves_when_switched_off() {
    let f = Fixture::new();
    skill(&f.global(), "notes");
    let notes = f.view("notes");
    f.dispatch(
        "agent.skills.set_pinned",
        json!({"id": notes["id"], "pinned": true, "expected_revision": f.revision()}),
    )
    .expect("pinned");
    let texts = instructions::pinned_texts(&f.runtime, None, &[id(&notes)]).expect("texts");
    assert_eq!(texts.len(), 1);
    let text = &texts[0];
    assert!(text.contains("Pinned Skill \"notes\""), "{text}");
    assert!(text.contains(notes["path"].as_str().expect("folder")), "{text}");
    assert!(text.contains("references/guide.md"), "{text}");
    assert!(text.contains("Use [the guide](references/guide.md)."), "{text}");
    assert_eq!(
        instructions::pinned_texts(&f.runtime, None, &[id(&notes)]).expect("again"),
        texts,
        "read again, the same bytes"
    );
    f.dispatch(
        "agent.skills.set_enabled",
        json!({"id": notes["id"], "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("off");
    assert!(instructions::pinned_texts(&f.runtime, None, &[id(&notes)]).expect("off").is_empty());
}
