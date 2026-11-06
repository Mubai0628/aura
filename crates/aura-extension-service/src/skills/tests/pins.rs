//! Pinned Skills: their instructions go whole into every request where they are usable. A pin is
//! refused past the shared limit; one that grew past it later is left out of the requests and
//! shown so, and the others stay.
use super::support::{Fixture, skill, write};
use serde_json::{Value, json};

/// A Skill in the user's `.agents\skills` whose instructions are `chars` ASCII characters, about
/// a third as many tokens.
fn sized(f: &Fixture, name: &str, chars: usize) {
    write(
        &f.global().join(name).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: Sized {name}\n---\n\n{}\n", "x".repeat(chars)),
    );
}

fn pin(f: &Fixture, view: &Value, pinned: bool) -> anyhow::Result<Value> {
    f.dispatch(
        "agent.skills.set_pinned",
        json!({"id": view["id"], "pinned": pinned, "expected_revision": f.revision()}),
    )
}

#[test]
fn a_pinned_skill_shows_its_size_and_unpinning_takes_it_out() {
    let f = Fixture::new();
    skill(&f.global(), "notes");
    let notes = f.view("notes");
    assert_eq!(notes["pinned"], false);
    let listed = pin(&f, &notes, true).expect("pinned");
    let pinned = f.view("notes");
    assert_eq!(pinned["pinned"], true);
    assert_eq!(pinned["pinned_left_out"], false);
    let tokens = pinned["pin_tokens"].as_u64().expect("size");
    assert!(tokens > 0, "{pinned}");
    assert_eq!(listed["pinned_tokens"], tokens);
    assert_eq!(listed["pinned_limit"], 20_000);
    let stale = f.dispatch(
        "agent.skills.set_pinned",
        json!({"id": notes["id"], "pinned": false, "expected_revision": f.revision() - 1}),
    );
    assert!(stale.expect_err("stale").to_string().starts_with("skills.revision_changed"));
    pin(&f, &pinned, false).expect("unpinned");
    assert_eq!(f.view("notes")["pinned"], false);
    assert_eq!(f.list()["pinned_tokens"], 0);
}

#[test]
fn a_pin_past_the_limit_is_refused_and_nothing_changes() {
    let f = Fixture::new();
    sized(&f, "first", 36_000);
    sized(&f, "second", 36_000);
    pin(&f, &f.view("first"), true).expect("the first fits");
    let revision = f.revision();
    let message = pin(&f, &f.view("second"), true).expect_err("over the limit").to_string();
    assert!(message.starts_with("skills.pin_limit"), "{message}");
    assert!(message.contains("20000"), "{message}");
    assert_eq!(f.revision(), revision, "nothing was written");
    assert_eq!(f.view("second")["pinned"], false);
}

#[test]
fn a_skill_not_usable_now_cannot_be_pinned() {
    let (f, workspace) = Fixture::with_workspace();
    skill(&workspace.folder.join(".agents").join("skills"), "style");
    let style = f.view_in(&workspace, "workspace", "style");
    let refused = f.dispatch(
        "agent.skills.set_pinned",
        json!({"id": style["id"], "workspace_id": workspace.id, "pinned": true, "expected_revision": f.listed_in(&workspace)["revision"]}),
    );
    assert!(refused.expect_err("not trusted").to_string().starts_with("skills.not_ready"));
}

#[test]
fn a_pinned_skill_that_grew_past_the_limit_is_left_out_and_the_others_stay() {
    let f = Fixture::new();
    sized(&f, "alpha", 300);
    sized(&f, "beta", 300);
    pin(&f, &f.view("alpha"), true).expect("alpha");
    pin(&f, &f.view("beta"), true).expect("beta");
    sized(&f, "alpha", 66_000);
    let alpha = f.view("alpha");
    assert_eq!(alpha["pinned"], true);
    assert_eq!(alpha["pinned_left_out"], true, "{alpha}");
    let beta = f.view("beta");
    assert_eq!(beta["pinned_left_out"], false, "{beta}");
    assert_eq!(f.list()["pinned_tokens"], beta["pin_tokens"]);
}

#[test]
fn removing_a_pinned_skill_drops_its_pin() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().expect("outside");
    let original = skill(outside.path(), "pdf");
    f.dispatch("agent.skills.import", json!({"path": original, "expected_revision": f.revision()}))
        .expect("imported");
    let pdf = f.view("pdf");
    pin(&f, &pdf, true).expect("pinned");
    f.dispatch("agent.skills.remove", json!({"id": pdf["id"], "expected_revision": f.revision()}))
        .expect("removed");
    assert!(f.settings.skill_registry().pinned.is_empty());
}

#[test]
fn a_pinned_skill_chosen_with_a_message_is_only_named_there() {
    let f = Fixture::new();
    skill(&f.global(), "notes");
    pin(&f, &f.view("notes"), true).expect("pinned");
    let (_, _, skills) = f.runtime.list(None).expect("listed");
    let notes = skills.iter().find(|s| s.view.relative == "notes").expect("notes");
    let text = super::super::instructions::chosen_text(notes).expect("usable");
    assert!(text.starts_with("Chosen Skill \"notes\" [agent_global]"), "{text}");
    assert!(text.contains("already at the start of this conversation"), "{text}");
    assert!(!text.contains("references/guide.md"), "named, not repeated: {text}");
    // One pinned but left out of the requests is given whole, as any other.
    sized(&f, "notes", 66_000);
    let (_, _, skills) = f.runtime.list(None).expect("listed");
    let grown = skills.iter().find(|s| s.view.relative == "notes").expect("notes");
    assert!(grown.view.pinned_left_out);
    let whole = super::super::instructions::chosen_text(grown).expect("usable");
    assert!(whole.contains("it is here whole"), "{whole}");
}
