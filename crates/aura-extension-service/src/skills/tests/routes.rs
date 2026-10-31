//! What is listed, what trust covers, importing and removing.
use super::support::{Fixture, skill, write};
use serde_json::json;

#[test]
fn a_skill_in_the_users_agents_folder_starts_off_and_trust_switches_it_on() {
    let f = Fixture::new();
    skill(&f.global(), "notes");
    let view = f.view("notes");
    assert_eq!(view["source"], "agent_global");
    assert_eq!(view["state"], "disabled", "found outside Aura: off until the user chooses");
    assert_eq!(view["name"], "notes");
    assert_eq!(view["file_count"], 2);
    let preview = f.dispatch("agent.skills.preview", json!({"id": view["id"]})).expect("preview");
    assert!(preview["skill_md"].as_str().expect("text").contains("Use [the guide]"));
    assert_eq!(preview["files"].as_array().expect("files").len(), 2);
    let trusted = f.trust("notes");
    let listed = &trusted["skills"][0];
    assert_eq!(listed["state"], "ready");
    assert_eq!(listed["enabled"], true);
    assert_eq!(listed["trusted_fingerprint"], view["fingerprint"]);
    let roots = f.list()["roots"].clone();
    assert!(
        roots
            .as_array()
            .expect("roots")
            .iter()
            .any(|r| r["source"] == "agent_global" && r["exists"] == true)
    );
}

#[test]
fn trust_is_given_only_at_the_shown_content_and_lost_when_it_changes() {
    let f = Fixture::new();
    let directory = skill(&f.global(), "notes");
    let view = f.view("notes");
    let refused = f.dispatch(
        "agent.skills.trust",
        json!({"id": view["id"], "expected_fingerprint": "0".repeat(64), "expected_revision": f.revision()}),
    );
    assert!(refused.expect_err("refused").to_string().starts_with("skills.content_changed"));
    f.trust("notes");
    write(&directory.join("references/guide.md"), "Step two.\n");
    assert_eq!(f.view("notes")["state"], "content_changed");
    f.trust("notes");
    assert_eq!(f.view("notes")["state"], "ready", "trusting the new content again");
    let id = f.view("notes")["id"].clone();
    f.dispatch("agent.skills.untrust", json!({"id": id, "expected_revision": f.revision()}))
        .expect("untrusted");
    assert_eq!(f.view("notes")["state"], "not_trusted");
    f.dispatch(
        "agent.skills.set_enabled",
        json!({"id": id, "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("off");
    assert_eq!(f.view("notes")["state"], "disabled");
    let stale = f.dispatch(
        "agent.skills.set_enabled",
        json!({"id": id, "enabled": true, "expected_revision": f.revision() - 1}),
    );
    assert!(stale.expect_err("stale").to_string().starts_with("skills.revision_changed"));
}

#[test]
fn importing_copies_exactly_the_checked_folder_and_removing_deletes_only_the_copy() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().expect("outside");
    let original = skill(outside.path(), "pdf");
    let before = std::fs::read(original.join("SKILL.md")).expect("original");
    f.dispatch("agent.skills.import", json!({"path": original, "expected_revision": f.revision()}))
        .expect("imported");
    let imported = f.view("pdf");
    assert_eq!(imported["source"], "aura_store");
    assert_eq!(imported["state"], "not_trusted", "imported: on, but not trusted yet");
    assert!(f.store.join("pdf").join("references").join("guide.md").is_file());
    f.dispatch("agent.skills.import", json!({"path": original, "expected_revision": f.revision()}))
        .expect("a second copy");
    assert_eq!(f.view("pdf-2")["source"], "aura_store", "a taken name gets a number");
    let id = imported["id"].clone();
    f.dispatch("agent.skills.remove", json!({"id": id, "expected_revision": f.revision()}))
        .expect("removed");
    assert!(!f.store.join("pdf").exists());
    assert_eq!(std::fs::read(original.join("SKILL.md")).expect("original"), before);
    assert!(f.list()["skills"].as_array().expect("skills").iter().all(|s| s["relative"] != "pdf"));

    // The user's own folder goes to the Recycle Bin (a bin beside it in tests), not away for good.
    skill(&f.global(), "mine");
    let global = f.view("mine")["id"].clone();
    f.dispatch("agent.skills.remove", json!({"id": global, "expected_revision": f.revision()}))
        .expect("deleted to the Recycle Bin");
    assert!(!f.global().join("mine").exists());
    assert!(f.global().join(".test-recycle-bin").join("mine").join("SKILL.md").is_file());
    assert!(f.list()["skills"].as_array().expect("skills").iter().all(|s| s["relative"] != "mine"));
}

/// With the folder open in another program, removing once said only "try
/// again". A file held without delete sharing is what Explorer or an editor can leave behind.
#[test]
fn a_folder_another_program_holds_is_kept_and_the_reason_is_named() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let outside = tempfile::tempdir().expect("outside");
    let original = skill(outside.path(), "held");
    f.dispatch("agent.skills.import", json!({"path": original, "expected_revision": f.revision()}))
        .expect("imported");
    let id = f.view("held")["id"].clone();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1) // FILE_SHARE_READ: the folder cannot be moved while this is open
        .open(f.store.join("held").join("SKILL.md"))
        .expect("held open");
    let refused =
        f.dispatch("agent.skills.remove", json!({"id": id, "expected_revision": f.revision()}));
    let message = refused.expect_err("refused while held").to_string();
    assert!(
        message.starts_with("skills.remove_busy") && message.contains("关闭后再移除"),
        "{message}"
    );
    assert!(f.store.join("held").join("SKILL.md").is_file(), "the Skill is unchanged");
    drop(held);
    f.dispatch("agent.skills.remove", json!({"id": id, "expected_revision": f.revision()}))
        .expect("removed once closed");
    assert!(!f.store.join("held").exists());
}

#[test]
fn invalid_folders_and_links_are_listed_with_their_reason_and_never_followed() {
    let f = Fixture::new();
    write(&f.global().join("broken").join("SKILL.md"), "no front matter\n");
    let outside = tempfile::tempdir().expect("outside");
    skill(outside.path(), "elsewhere");
    junction::create(outside.path().join("elsewhere"), f.global().join("linked"))
        .expect("junction");
    let broken = f.view("broken");
    assert_eq!(broken["state"], "invalid");
    assert!(broken["problems"].to_string().contains("YAML front matter"));
    assert_eq!(f.view("linked")["state"], "unavailable");
    let not_a_skill = tempfile::tempdir().expect("plain folder");
    let refused = f.dispatch(
        "agent.skills.import",
        json!({"path": not_a_skill.path(), "expected_revision": f.revision()}),
    );
    assert!(refused.expect_err("refused").to_string().starts_with("skills.invalid"));

    // A linked `.agents` makes the whole folder unreadable: nothing under it is a Skill.
    let g = Fixture::new();
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    skill(&elsewhere.path().join("skills"), "hidden");
    std::fs::create_dir_all(&g.profile).expect("profile");
    junction::create(elsewhere.path(), g.profile.join(".agents")).expect("junction");
    let listed = g.list();
    assert!(listed["skills"].as_array().expect("skills").is_empty());
    assert!(
        listed["roots"]
            .as_array()
            .expect("roots")
            .iter()
            .any(|r| r["source"] == "agent_global" && r["exists"] == false)
    );
}

/// A ZIP file with these entries, written in `dir`.
fn archive(dir: &std::path::Path, name: &str, entries: &[(&str, &str)]) -> std::path::PathBuf {
    use std::io::Write;
    let path = dir.join(name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("zip"));
    for (entry, text) in entries {
        zip.start_file(*entry, zip::write::SimpleFileOptions::default()).expect("entry");
        zip.write_all(text.as_bytes()).expect("bytes");
    }
    zip.finish().expect("finish");
    path
}

#[test]
fn a_zip_file_is_unpacked_into_aura_checked_like_a_folder_and_left_unchanged() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().expect("outside");
    let skill_md = "---\nname: pdf\ndescription: Test skill pdf\n---\n\nUse [the guide](references/guide.md).\n";
    let packed = archive(
        outside.path(),
        "pdf.skill",
        &[("pdf/SKILL.md", skill_md), ("pdf/references/guide.md", "Step one.\n")],
    );
    let before = std::fs::read(&packed).expect("archive");
    f.dispatch("agent.skills.import", json!({"path": packed, "expected_revision": f.revision()}))
        .expect("imported");
    let imported = f.view("pdf");
    assert_eq!(imported["source"], "aura_store");
    assert_eq!(imported["state"], "not_trusted", "unpacked: on, but not trusted yet");
    assert_eq!(imported["file_count"], 2);
    assert!(f.store.join("pdf").join("references").join("guide.md").is_file());
    assert_eq!(std::fs::read(&packed).expect("archive"), before, "the ZIP is not changed");

    // An entry that climbs out is refused before anything is written, and nothing is left.
    let evil = archive(outside.path(), "evil.zip", &[("SKILL.md", skill_md), ("../evil.md", "x")]);
    let refused =
        f.dispatch("agent.skills.import", json!({"path": evil, "expected_revision": f.revision()}));
    assert!(refused.expect_err("refused").to_string().starts_with("skills.invalid_archive"));
    let left: Vec<_> = std::fs::read_dir(&f.store)
        .expect("store")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(left, ["pdf"], "no staging folder is left behind");
    assert!(!outside.path().join("evil.md").exists());
}
