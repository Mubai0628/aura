//! A usable Skill's folder is read by the file tools without asking, in every mode and also where
//! it sits in Aura's own data; once the Skill is switched off a read asks again, a write there
//! still asks, and the rest of Aura's data still asks.
use super::support::*;
use super::*;
use serde_json::Value;
use std::path::Path;

const FILES: &[&str] = &["skills.search", "skills.load", "fs.read", "fs.list", "fs.write"];

fn read(path: &Path) -> Value {
    json!({"path": path, "range": {"range_kind": "full"}, "explicit_encoding": null})
}

fn asks(outcome: Option<BatchOutcome>) -> bool {
    matches!(outcome, Some(BatchOutcome::AwaitingApproval { .. }))
}

#[tokio::test]
async fn l15_a_usable_skills_files_are_read_without_asking_until_it_is_switched_off() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, FILES).await;
    let folder = skill(&agents(&f), "notes", "Follow references/guide.md.");
    let guide = folder.join("references").join("guide.md");
    let (read_id, _) = call(&mut f, "fs.read", read(&guide)).await;
    assert_eq!(f.output(read_id)["text"], "Guide of notes.\n");
    let (listed, _) =
        call(&mut f, "fs.list", json!({"path": folder, "limit": 100, "cursor": null})).await;
    assert!(f.output(listed).to_string().contains("SKILL.md"), "{}", f.output(listed));

    set_enabled(&f, None, "notes", false).await;
    let batch = f.admit(&[("fs.read", read(&guide))]);
    assert!(asks(f.execute_rejecting(batch).await), "a switched-off Skill's files ask again");
}

#[tokio::test]
async fn l15_a_write_in_a_usable_skills_folder_still_asks() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, FILES).await;
    let folder = skill(&agents(&f), "notes", "Follow references/guide.md.");
    let batch = f.admit(&[(
        "fs.write",
        json!({"path": folder.join("added.md"), "text": "x", "expected": {"expected_kind": "absent"}}),
    )]);
    assert!(asks(f.execute_rejecting(batch).await));
    assert!(!folder.join("added.md").exists());
}

#[tokio::test]
async fn l15_an_imported_skill_is_read_without_asking_while_the_rest_of_auras_data_still_asks() {
    let mut f = Fixture::with_exposure(PermissionMode::FullAccess, FILES).await;
    let source = skill(&f._host.root.path().join("skill-source"), "pdf", "Use the guide.");
    let list = route(&f, "agent.skills.list", json!({})).await;
    route(
        &f,
        "agent.skills.import",
        json!({"path": source, "expected_revision": list["revision"]}),
    )
    .await;
    let list = route(&f, "agent.skills.list", json!({})).await;
    let imported = list["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .find(|skill| skill["source"] == "aura_store" && skill["name"] == "pdf")
        .cloned()
        .unwrap_or_else(|| panic!("imported: {list}"));
    let folder = std::path::PathBuf::from(imported["path"].as_str().expect("folder"));
    let (read_id, _) =
        call(&mut f, "fs.read", read(&folder.join("references").join("guide.md"))).await;
    assert_eq!(f.output(read_id)["text"], "Guide of pdf.\n");

    // Beside the Skill, in Aura's own data, a read asks even in FullAccess.
    let stray = folder.parent().expect("Aura's Skill folder").join("stray.txt");
    std::fs::write(&stray, "Aura's own data.\n").expect("stray file");
    let batch = f.admit(&[("fs.read", read(&stray))]);
    assert!(asks(f.execute_rejecting(batch).await));
}
