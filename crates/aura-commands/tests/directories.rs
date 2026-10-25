//! Scanning a commands root, merging it into the catalog, and removing a user command.
mod support;
use aura_agent_domain::{CommandSource, builtin_commands};
use aura_commands::{CommandStoreError, delete_user_command, disabled_key, merge, scan};
use aura_domain_common::WorkspaceId;
use std::collections::BTreeSet;
use support::{MANIFEST, TEMPLATE, command, manifest_named, root};

#[test]
fn a_root_lists_its_command_directories_in_order_and_skips_staging_and_files() {
    let root = root();
    command(root.path(), "zeta", Some(&manifest_named("zeta")), Some(TEMPLATE));
    command(root.path(), "review", Some(MANIFEST), Some(TEMPLATE));
    command(root.path(), ".removing-old-1", Some(MANIFEST), Some(TEMPLATE));
    std::fs::write(root.path().join("README.md"), "notes").expect("a file in the root");
    let names =
        scan(root.path(), CommandSource::User).into_iter().map(|c| c.name).collect::<Vec<_>>();
    assert_eq!(names, ["review", "zeta"]);
    assert!(scan(&root.path().join("missing"), CommandSource::User).is_empty());
}

#[test]
fn a_linked_command_directory_is_listed_but_never_read() {
    let root = root();
    let elsewhere = tempfile::tempdir().expect("a directory outside the root");
    command(elsewhere.path(), "outside", Some(&manifest_named("outside")), Some(TEMPLATE));
    junction::create(elsewhere.path().join("outside"), root.path().join("outside"))
        .expect("a junction inside the commands root");
    let listed = scan(root.path(), CommandSource::Workspace);
    assert_eq!(listed.len(), 1);
    assert!(listed[0].issue.as_deref().is_some_and(|issue| issue.contains("链接或挂载点")));
    assert_eq!(listed[0].template, None, "nothing behind the link is read");
    let removal = delete_user_command(root.path(), "outside", "any");
    assert!(matches!(removal, Err(CommandStoreError::NotRemovable(_))), "{removal:?}");
    assert!(elsewhere.path().join("outside").join("template.md").exists());
}

#[test]
fn built_ins_win_and_a_working_workspace_command_takes_the_name_from_the_users() {
    let user_root = root();
    let workspace_root = root();
    command(user_root.path(), "review", Some(MANIFEST), Some(TEMPLATE));
    command(user_root.path(), "init", Some(&manifest_named("init")), Some(TEMPLATE));
    command(user_root.path(), "notes", Some(&manifest_named("notes")), Some(TEMPLATE));
    command(workspace_root.path(), "review", Some(MANIFEST), Some(TEMPLATE));
    command(workspace_root.path(), "notes", Some(&manifest_named("notes")), None);
    let user = scan(user_root.path(), CommandSource::User);
    let workspace = scan(workspace_root.path(), CommandSource::Workspace);
    let id = WorkspaceId::new();
    let entries = merge(builtin_commands(), &user, Some((id, &workspace)), &BTreeSet::new());
    let find = |source: CommandSource, name: &str| {
        entries.iter().find(|e| e.source == source && e.name == name).expect(name).clone()
    };
    assert_eq!(find(CommandSource::User, "init").issue.as_deref(), Some("与内置命令重名，未启用"));
    assert!(find(CommandSource::Builtin, "init").runnable());
    assert_eq!(
        find(CommandSource::User, "review").issue.as_deref(),
        Some("被当前工作区的同名命令覆盖")
    );
    assert!(find(CommandSource::Workspace, "review").runnable());
    assert!(
        find(CommandSource::User, "notes").runnable(),
        "a broken workspace command shadows nothing"
    );
    // Turning the workspace command off in this workspace gives the name back.
    let off = BTreeSet::from([disabled_key(CommandSource::Workspace, Some(id), "review")]);
    let entries = merge(builtin_commands(), &user, Some((id, &workspace)), &off);
    let user_review = entries
        .iter()
        .find(|e| e.source == CommandSource::User && e.name == "review")
        .expect("user review");
    assert!(user_review.runnable());
    assert_ne!(
        disabled_key(CommandSource::Workspace, Some(WorkspaceId::new()), "review"),
        off.iter().next().cloned().expect("key")
    );
}

#[test]
fn a_user_command_is_removed_only_at_the_revision_the_user_saw() {
    let root = root();
    command(root.path(), "review", Some(MANIFEST), Some(TEMPLATE));
    let seen = scan(root.path(), CommandSource::User)[0].revision.clone().expect("revision");
    std::fs::write(root.path().join("review/template.md"), "改过的模板 $ARGUMENTS\n")
        .expect("edited meanwhile");
    let stale = delete_user_command(root.path(), "review", &seen);
    assert!(matches!(stale, Err(CommandStoreError::RevisionChanged(_))), "{stale:?}");
    assert!(root.path().join("review").exists());
    let current = scan(root.path(), CommandSource::User)[0].revision.clone().expect("revision");
    delete_user_command(root.path(), "review", &current).expect("removed at its revision");
    assert!(!root.path().join("review").exists());
    assert!(std::fs::read_dir(root.path()).expect("root").next().is_none(), "no staging left");
    assert!(matches!(
        delete_user_command(root.path(), "review", &current),
        Err(CommandStoreError::NotFound(_))
    ));
}

#[test]
fn a_workspace_whose_commands_folder_is_linked_elsewhere_has_no_commands() {
    let workspace = root();
    let elsewhere = tempfile::tempdir().expect("a directory outside the workspace");
    command(&elsewhere.path().join("commands"), "review", Some(MANIFEST), Some(TEMPLATE));
    std::fs::create_dir(workspace.path().join(".aura")).expect(".aura");
    junction::create(elsewhere.path().join("commands"), workspace.path().join(".aura/commands"))
        .expect("a linked commands folder");
    assert!(
        aura_commands::scan_within(workspace.path(), ".aura/commands", CommandSource::Workspace)
            .is_empty()
    );
    let real = root();
    command(&real.path().join(".aura/commands"), "review", Some(MANIFEST), Some(TEMPLATE));
    assert_eq!(
        aura_commands::scan_within(real.path(), ".aura/commands", CommandSource::Workspace).len(),
        1
    );
}
