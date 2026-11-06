//! Where Skills are read from, and what each needs before the model may use it: the user's own
//! folders (Aura's, `.agents\skills`, `.claude\skills`) are usable as found; a workspace's
//! `.agents\skills` and `.claude\skills` wait until the user trusts that workspace once.
use super::support::{Fixture, skill, write};
use serde_json::json;

#[test]
fn the_users_agents_and_claude_skills_are_usable_without_trust_and_after_they_change() {
    let f = Fixture::new();
    let agents = skill(&f.global(), "notes");
    skill(&f.claude(), "review");
    let notes = f.view("notes");
    assert_eq!(notes["source"], "agent_global");
    assert_eq!(notes["state"], "ready");
    assert_eq!(notes["enabled"], true);
    let review = f.view("review");
    assert_eq!(review["source"], "claude_global");
    assert_eq!(review["state"], "ready");
    let roots = f.list()["roots"].clone();
    assert!(
        roots.as_array().expect("roots").iter().any(|r| r["source"] == "claude_global"
            && r["exists"] == true
            && r["path"].as_str().is_some_and(|p| p.ends_with(r".claude\skills"))),
        "{roots}"
    );
    write(&agents.join("references/guide.md"), "Step two.\n");
    assert_eq!(f.view("notes")["state"], "ready", "a change needs no new trust");
    f.dispatch(
        "agent.skills.set_enabled",
        json!({"id": notes["id"], "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("off");
    assert_eq!(f.view("notes")["state"], "disabled");
    assert_eq!(f.view("review")["state"], "ready", "only the one switched off");
}

#[test]
fn an_imported_skill_is_usable_at_once() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().expect("outside");
    let original = skill(outside.path(), "pdf");
    f.dispatch("agent.skills.import", json!({"path": original, "expected_revision": f.revision()}))
        .expect("imported");
    let imported = f.view("pdf");
    assert_eq!(imported["source"], "aura_store");
    assert_eq!(imported["state"], "ready");
}

#[test]
fn a_workspaces_skills_wait_until_the_workspace_is_trusted_once() {
    let (f, workspace) = Fixture::with_workspace();
    skill(&workspace.folder.join(".agents").join("skills"), "style");
    skill(&workspace.folder.join(".claude").join("skills"), "deploy");
    assert_eq!(f.view_in(&workspace, "workspace", "style")["state"], "not_trusted");
    assert_eq!(f.view_in(&workspace, "claude_workspace", "deploy")["state"], "not_trusted");
    assert_eq!(f.listed_in(&workspace)["workspace_trusted"], false);
    let revision = f.listed_in(&workspace)["revision"].clone();
    let trusted = f
        .dispatch(
            "agent.skills.trust_workspace",
            json!({"workspace_id": workspace.id, "expected_revision": revision}),
        )
        .expect("trusted");
    assert_eq!(trusted["workspace_trusted"], true);
    assert_eq!(f.view_in(&workspace, "workspace", "style")["state"], "ready");
    assert_eq!(f.view_in(&workspace, "claude_workspace", "deploy")["state"], "ready");
    write(&workspace.folder.join(".agents/skills/style/references/guide.md"), "Changed.\n");
    assert_eq!(f.view_in(&workspace, "workspace", "style")["state"], "ready");

    let revision = f.listed_in(&workspace)["revision"].clone();
    f.dispatch(
        "agent.skills.untrust_workspace",
        json!({"workspace_id": workspace.id, "expected_revision": revision}),
    )
    .expect("untrusted");
    assert_eq!(f.view_in(&workspace, "workspace", "style")["state"], "not_trusted");
    assert_eq!(f.listed_in(&workspace)["workspace_trusted"], false);
    let stale = f.dispatch(
        "agent.skills.trust_workspace",
        json!({"workspace_id": workspace.id, "expected_revision": 0}),
    );
    assert!(stale.expect_err("stale").to_string().starts_with("skills.revision_changed"));
}

#[test]
fn withdrawing_a_workspaces_trust_also_withdraws_an_earlier_trust_of_one_of_its_skills() {
    let (f, workspace) = Fixture::with_workspace();
    skill(&workspace.folder.join(".agents").join("skills"), "style");
    skill(&workspace.folder.join(".agents").join("skills"), "other");
    let style = f.view_in(&workspace, "workspace", "style");
    let revision = f.listed_in(&workspace)["revision"].clone();
    f.dispatch(
        "agent.skills.trust",
        json!({"id": style["id"], "workspace_id": workspace.id, "expected_fingerprint": style["fingerprint"], "expected_revision": revision}),
    )
    .expect("one Skill trusted the earlier way");
    assert_eq!(f.view_in(&workspace, "workspace", "style")["state"], "ready");
    assert_eq!(
        f.view_in(&workspace, "workspace", "other")["state"],
        "not_trusted",
        "that trust is not widened to the workspace"
    );
    let revision = f.listed_in(&workspace)["revision"].clone();
    f.dispatch(
        "agent.skills.untrust_workspace",
        json!({"workspace_id": workspace.id, "expected_revision": revision}),
    )
    .expect("untrusted");
    assert_eq!(f.view_in(&workspace, "workspace", "style")["state"], "not_trusted");
}
