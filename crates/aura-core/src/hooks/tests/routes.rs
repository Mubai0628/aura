use super::support::Fixture;
use serde_json::json;

#[test]
fn trust_is_given_only_at_the_previewed_fingerprint_and_lost_when_content_changes() {
    let f = Fixture::new();
    let path = f.package("check", &["turn.completed"], &["continue"], "");
    let preview = f
        .dispatch("agent.hooks.preview", json!({"path": path.display().to_string()}))
        .expect("preview");
    assert_eq!(preview["definition"]["name"], "check");
    assert_eq!(preview["registered"], false);
    assert_eq!(
        preview["files"].as_array().map(|files| files.len()),
        Some(2),
        "the manifest and the program are listed"
    );
    std::fs::write(path.join("README.md"), "changed after the preview").expect("edit");
    let stale = f.dispatch(
        "agent.hooks.trust",
        json!({"path": path.display().to_string(), "expected_fingerprint": preview["fingerprint"], "enabled": true, "expected_revision": f.revision()}),
    );
    assert!(stale.expect_err("stale").to_string().starts_with("hooks.content_changed"));
    f.trust(&path);
    let list = f.dispatch("agent.hooks.list", json!({})).expect("list");
    assert_eq!(list["entries"][0]["state"], "ready");
    std::fs::write(path.join("README.md"), "changed after trust").expect("edit");
    let list = f.dispatch("agent.hooks.list", json!({})).expect("list");
    assert_eq!(list["entries"][0]["state"], "content_changed");
}

#[test]
fn enable_untrust_reorder_and_remove_change_only_the_registry() {
    let f = Fixture::new();
    let first = f.package("first", &["turn.completed"], &["continue"], "");
    let second = f.package("second", &["turn.completed"], &["continue"], "");
    f.trust(&first);
    f.trust(&second);
    let path = |p: &std::path::Path| p.display().to_string();
    let moved = f
        .dispatch(
            "agent.hooks.move",
            json!({"path": path(&second), "direction": "up", "expected_revision": f.revision()}),
        )
        .expect("moved");
    assert_eq!(moved["entries"][0]["definition"]["name"], "second", "order is execution order");
    let disabled = f
        .dispatch(
            "agent.hooks.set_enabled",
            json!({"path": path(&first), "enabled": false, "expected_revision": f.revision()}),
        )
        .expect("disabled");
    assert_eq!(disabled["entries"][1]["state"], "disabled");
    let untrusted = f
        .dispatch(
            "agent.hooks.untrust",
            json!({"path": path(&second), "expected_revision": f.revision()}),
        )
        .expect("untrusted");
    assert_eq!(untrusted["entries"][0]["state"], "not_trusted");
    let removed = f
        .dispatch(
            "agent.hooks.remove",
            json!({"path": path(&second), "expected_revision": f.revision()}),
        )
        .expect("removed");
    assert_eq!(removed["entries"].as_array().map(Vec::len), Some(1));
    assert!(second.join("hook.toml").is_file(), "removing never deletes the folder");
}

#[test]
fn a_stale_revision_or_a_relative_path_is_refused() {
    let f = Fixture::new();
    let path = f.package("check", &["turn.completed"], &["continue"], "");
    f.trust(&path);
    let stale = f.dispatch(
        "agent.hooks.set_enabled",
        json!({"path": path.display().to_string(), "enabled": false, "expected_revision": 1}),
    );
    assert!(stale.expect_err("stale").to_string().starts_with("hooks.revision_changed"));
    let relative = f.dispatch("agent.hooks.preview", json!({"path": "hooks/check"}));
    assert!(relative.expect_err("relative").to_string().starts_with("hooks.invalid_path"));
    let missing = f.dispatch(
        "agent.hooks.remove",
        json!({"path": "C:/not/registered", "expected_revision": f.revision()}),
    );
    assert!(missing.expect_err("missing").to_string().starts_with("hooks.not_found"));
}
