//! Which workspace an unprovable writer reaches, and whose spelling another session's row carries.
use super::*;

fn shell(relation: ActivityResourceRelation, folder: &str) -> Vec<ActivityResource> {
    vec![resource(
        ActivityResourceTarget::Unproven { hint: "shell write set not provable".to_owned() },
        relation,
        folder,
    )]
}

/// A command whose script names places outside its workspace is the one unprovable writer every
/// workspace keeps seeing, as the minimum fact and without its folder.
#[test]
fn an_unknown_writer_outside_its_workspace_is_seen_from_every_workspace() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    publish_agent_tool(
        &index,
        runtime_instance_id,
        SessionId::new(),
        WorkspaceId::new(),
        ActivityToolGroup::Shell,
        1,
        shell(ActivityResourceRelation::UnknownWriteSetOutsideWorkspace, "tools"),
    );
    let reader = view(runtime_instance_id, SessionId::new(), WorkspaceId::new());
    let snapshot = index.snapshot(&reader);
    assert_eq!(snapshot.header.coverage.total_in_scope, 1, "in every reader's scope");
    assert_eq!(snapshot.rows.len(), 1, "and admitted as the minimum fact");
    let resource = &snapshot.rows[0].resources[0];
    assert_eq!(resource.relation, ActivityResourceRelation::UnknownWriteSetOutsideWorkspace);
    assert!(resource.display_name.is_empty(), "never the owner's folder: {resource:?}");
    let body = snapshot.select_summary(&reader, ActivitySummaryBudget::default()).body;
    assert!(body.contains("unknown-write-set-outside-workspace"), "{body}");
}

/// A sibling session in the same workspace learns the folder an unprovable writer runs in; a
/// reader in another workspace learns nothing about it.
#[test]
fn a_sibling_sees_the_folder_of_an_unknown_writer_in_its_workspace() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let workspace = WorkspaceId::new();
    publish_agent_tool(
        &index,
        runtime_instance_id,
        SessionId::new(),
        workspace,
        ActivityToolGroup::Shell,
        1,
        shell(ActivityResourceRelation::UnknownWriteSet, "crates/tools"),
    );
    let sibling = index.snapshot(&view(runtime_instance_id, SessionId::new(), workspace));
    assert_eq!(sibling.rows.len(), 1);
    assert_eq!(sibling.rows[0].resources[0].display_name, "crates/tools");
    assert!(sibling.rows[0].parameters.is_empty(), "never its parameters");
    let elsewhere =
        index.snapshot(&view(runtime_instance_id, SessionId::new(), WorkspaceId::new()));
    assert_eq!(elsewhere.header.coverage.total_in_scope, 0, "out of another workspace's scope");
    assert!(elsewhere.rows.is_empty());
}

/// Another session's row on a file this reader touched carries the reader's own name for it,
/// never the name its owner used.
#[test]
fn another_sessions_row_on_a_touched_file_carries_the_readers_name() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let workspace = WorkspaceId::new();
    let target = object_target(7, 42);
    publish_agent_tool(
        &index,
        runtime_instance_id,
        SessionId::new(),
        WorkspaceId::new(),
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(target.clone(), ActivityResourceRelation::Write, "C:/theirs/notes.md")],
    );
    let mut reader = view(runtime_instance_id, SessionId::new(), workspace);
    reader.relevant_targets.insert(ActivityResourceKey::of(&target));
    reader.relevant_resources.push(target.clone());
    reader.relevant_names.insert(ActivityResourceKey::of(&target), "docs/notes.md".to_owned());
    let snapshot = index.snapshot(&reader);
    assert_eq!(snapshot.rows.len(), 1, "the same physical file is visible across workspaces");
    assert_eq!(snapshot.rows[0].resources[0].display_name, "docs/notes.md");
    let mut unnamed = view(runtime_instance_id, SessionId::new(), workspace);
    unnamed.relevant_targets.insert(ActivityResourceKey::of(&target));
    unnamed.relevant_resources.push(target);
    let snapshot = index.snapshot(&unnamed);
    assert!(snapshot.rows[0].resources[0].display_name.is_empty(), "no name it did not supply");
}
