//! Deleting a Skill from the user's own folders: the folder goes to the Recycle Bin, never away for
//! good. In tests the bin is a folder beside the Skill, and a name with `not-recyclable` in it is
//! one the bin cannot take (`agent_execution::recycle`).
use super::support::{Fixture, skill};
use serde_json::json;

#[test]
fn a_folder_the_recycle_bin_cannot_take_stays_where_it_is_with_the_reason() {
    let f = Fixture::new();
    skill(&f.global(), "not-recyclable-notes");
    let id = f.view("not-recyclable-notes")["id"].clone();
    let refused =
        f.dispatch("agent.skills.remove", json!({"id": id, "expected_revision": f.revision()}));
    let reason = refused.expect_err("refused").to_string();
    assert!(reason.starts_with("skills.not_recyclable"), "{reason}");
    assert!(reason.contains("没有删除"), "{reason}");
    assert!(f.global().join("not-recyclable-notes").join("SKILL.md").is_file());
    assert_eq!(f.view("not-recyclable-notes")["id"], id, "still listed");
}

#[test]
fn a_stale_revision_deletes_nothing() {
    let f = Fixture::new();
    skill(&f.global(), "notes");
    let id = f.view("notes")["id"].clone();
    let stale = f.revision() + 1;
    let refused = f.dispatch("agent.skills.remove", json!({"id": id, "expected_revision": stale}));
    assert!(refused.expect_err("refused").to_string().starts_with("skills.revision_changed"));
    assert!(f.global().join("notes").join("SKILL.md").is_file());
}
