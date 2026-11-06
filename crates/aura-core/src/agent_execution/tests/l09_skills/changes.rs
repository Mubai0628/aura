//! A load reads the Skill as it is at that moment: changed, it gives the new instructions without
//! asking for trust again; switched off or moved away, it is refused. An earlier load's result
//! stays as it was.
use super::support::*;
use super::*;

#[tokio::test]
async fn l15_a_load_reads_the_skill_as_it_is_now() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, NAMES).await;
    let directory = skill(&agents(&f), "notes", "Write notes as short bullet lists.");
    let (first, _) = call(&mut f, "skills.load", json!({"name": "notes"})).await;
    assert_eq!(f.output(first)["instructions"], "Write notes as short bullet lists.");

    write(
        &directory.join("SKILL.md"),
        "---\nname: notes\ndescription: Skill notes for tests\n---\n\nWrite notes as numbered lists.\n",
    );
    let (second, _) = call(&mut f, "skills.load", json!({"name": "notes"})).await;
    assert_eq!(f.output(second)["instructions"], "Write notes as numbered lists.");
    assert_eq!(f.output(first)["instructions"], "Write notes as short bullet lists.");

    // Switched off: no longer found, and a load is refused.
    set_enabled(&f, None, "notes", false).await;
    let (found, _) = call(&mut f, "skills.search", json!({"query": ""})).await;
    assert_eq!(f.output(found)["skills"], json!([]));
    let (_, refused) = call(&mut f, "skills.load", json!({"name": "notes"})).await;
    assert!(refusal(&refused).starts_with("skills.unavailable"), "{refused:?}");

    // Moved away: refused as well.
    set_enabled(&f, None, "notes", true).await;
    std::fs::remove_dir_all(&directory).expect("moved away");
    let (_, refused) = call(&mut f, "skills.load", json!({"name": "notes"})).await;
    assert!(refusal(&refused).starts_with("skills.unavailable"), "{refused:?}");
}
