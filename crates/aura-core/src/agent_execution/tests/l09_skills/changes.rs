//! Withdrawal and updates take effect at the next step: a loaded Skill that is switched off,
//! untrusted or changed leaves the task state with the reason, and its files are not read.
use super::support::*;
use super::*;

#[tokio::test]
async fn l09_a_loaded_skill_leaves_the_task_state_when_it_changes_or_is_withdrawn() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, NAMES).await;
    let directory = skill(&agents(&f), "notes", "Write notes as short bullet lists.");
    let id = trust(&f, None, "notes").await;
    call(&mut f, "skills.load", json!({"skill_ids": [id]})).await;
    assert!(context(&f)[0].is_ok());

    // The content changes after it was trusted: left out, and its files are not read.
    write(&directory.join("references/guide.md"), "Changed guide.\n");
    assert_eq!(context(&f), [Err("内容在信任后变了")]);
    let (_, refused) = call(
        &mut f,
        "skills.read_resource",
        json!({"skill_id": id, "path": "references/guide.md"}),
    )
    .await;
    assert!(refusal(&refused).starts_with("skills.unavailable"), "{refused:?}");

    // Trusted again, it is still not the content this turn loaded until it is loaded again.
    trust(&f, None, "notes").await;
    assert_eq!(context(&f), [Err("内容在加载后变了")]);
    let (_, refused) = call(
        &mut f,
        "skills.read_resource",
        json!({"skill_id": id, "path": "references/guide.md"}),
    )
    .await;
    assert!(refusal(&refused).starts_with("skills.changed"), "{refused:?}");
    call(&mut f, "skills.load", json!({"skill_ids": [id]})).await;
    assert!(context(&f)[0].is_ok(), "loading again takes the new content");
    let (read, _) = call(
        &mut f,
        "skills.read_resource",
        json!({"skill_id": id, "path": "references/guide.md"}),
    )
    .await;
    assert_eq!(f.output(read)["text"], "Changed guide.\n");

    // Switched off: left out, no longer offered, and a load is refused.
    let list = route(&f, "agent.skills.list", json!({})).await;
    route(
        &f,
        "agent.skills.set_enabled",
        json!({"id": id, "enabled": false, "expected_revision": list["revision"]}),
    )
    .await;
    assert_eq!(context(&f), [Err("已停用或撤销信任")]);
    let (found, _) = call(&mut f, "skills.search", json!({"query": ""})).await;
    assert_eq!(f.output(found)["skills"], json!([]));
    let (_, refused) = call(&mut f, "skills.load", json!({"skill_ids": [id]})).await;
    assert!(refusal(&refused).starts_with("skills.unavailable"), "{refused:?}");

    // Moved away: left out with that reason.
    let list = route(&f, "agent.skills.list", json!({})).await;
    route(
        &f,
        "agent.skills.set_enabled",
        json!({"id": id, "enabled": true, "expected_revision": list["revision"]}),
    )
    .await;
    std::fs::remove_dir_all(&directory).expect("moved away");
    assert_eq!(context(&f), [Err("已移走或读取失败")]);
}
