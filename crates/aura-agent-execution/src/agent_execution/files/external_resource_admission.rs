//! The external destination's **real resource**, which is what replaces the old textual lock.
//!
//! The defect these cover is the pre-fix table: a key of canonical path text folded to lowercase.
//! Both halves were wrong and each wrong in the direction that matters, so both are tested against
//! the real shared derivation rather than against a second implementation of the same guess:
//!
//! * two **names for one object** — a hardlink — produced two keys and both writers entered, so the
//!   entry is now the object's own `FileIdentity`; and
//! * lowercasing folded two genuinely different files together on a case-sensitive directory, so the
//!   leaf rule now comes from the parent's real volume rather than from a fold.
use super::*;

fn existing_target(path: &std::path::Path, relative: &str) -> aura_fs_tools::ExternalWriteTarget {
    let file = aura_windows_platform::open_verified_existing(
        path,
        aura_windows_platform::NativeFileAccess::Read,
    )
    .expect("verified existing target");
    aura_fs_tools::ExternalWriteTarget {
        grant_id: None,
        session_id: SessionId::new(),
        display_path: path.display().to_string(),
        relative_path: WorkspacePath::parse(relative).expect("relative"),
        kind: aura_fs_tools::ExternalWriteKind::Existing(file),
    }
}

/// A hardlink's second name is the **same object**, so it must name the same resource.
///
/// This is the case the textual key could not see: two different canonical paths, one file
/// identity, so the pre-fix table let two writers in at once.
#[test]
fn a_hardlink_under_another_name_names_the_same_object_resource() {
    let root = tempfile::tempdir().expect("root");
    let first = root.path().join("shared.txt");
    let second = root.path().join("alias.txt");
    std::fs::write(&first, "before\n").expect("target");
    std::fs::hard_link(&first, &second).expect("hardlink");

    let parent = aura_windows_platform::open_verified_existing(
        root.path(),
        aura_windows_platform::NativeFileAccess::Metadata,
    )
    .expect("parent");
    let label = aura_workspace_domain::ResourceOwner {
        workspace_id: WorkspaceId::new(),
        session_id: SessionId::new(),
        path: WorkspacePath::parse("shared.txt").expect("path"),
    };
    let first_info =
        aura_windows_platform::inspect_existing_path(&first, true).expect("first identity");
    let second_info =
        aura_windows_platform::inspect_existing_path(&second, true).expect("second identity");
    assert_eq!(
        first_info.identity, second_info.identity,
        "the fixture must really be one object under two names"
    );
    let first_resource = aura_workspace::external_object(
        &parent,
        aura_workspace::domain_identity(&first_info.identity),
        label.clone(),
    )
    .expect("complete external object facts");
    let second_resource = aura_workspace::external_object(
        &parent,
        aura_workspace::domain_identity(&second_info.identity),
        label,
    )
    .expect("complete external object facts");
    assert!(
        aura_workspace_domain::conflict_between(
            &aura_workspace_domain::ResourceRequest::new(
                first_resource,
                aura_workspace_domain::ResourceAccess::Write,
            ),
            &aura_workspace_domain::ResourceRequest::new(
                second_resource,
                aura_workspace_domain::ResourceAccess::Write,
            ),
        )
        .is_some(),
        "one object under two names must be one resource"
    );
    assert!(
        first.to_string_lossy().to_lowercase() != second.to_string_lossy().to_lowercase(),
        "the two names really differ as text, which is why the old key missed this"
    );
}

/// Two genuinely different files in one directory stay **two** resources.
#[test]
fn two_different_files_in_one_directory_are_not_serialised() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("one.txt"), "one").expect("one");
    std::fs::write(root.path().join("two.txt"), "two").expect("two");
    let parent = aura_windows_platform::open_verified_existing(
        root.path(),
        aura_windows_platform::NativeFileAccess::Metadata,
    )
    .expect("parent");
    let label = aura_workspace_domain::ResourceOwner {
        workspace_id: WorkspaceId::new(),
        session_id: SessionId::new(),
        path: WorkspacePath::parse("one.txt").expect("path"),
    };
    let one = aura_workspace::external_slot(&parent, "one.txt", label.clone())
        .expect("complete first slot");
    let two =
        aura_workspace::external_slot(&parent, "two.txt", label).expect("complete second slot");
    assert!(
        aura_workspace_domain::conflict_between(
            &aura_workspace_domain::ResourceRequest::new(
                one,
                aura_workspace_domain::ResourceAccess::Write,
            ),
            &aura_workspace_domain::ResourceRequest::new(
                two,
                aura_workspace_domain::ResourceAccess::Write,
            ),
        )
        .is_none(),
        "two different files must not wait for one another"
    );
}

/// The entry one existing target computes is the object **and** its naming slot, so a create of
/// the same name serializes against a replacement of it.
#[test]
fn an_existing_target_names_both_its_object_and_its_slot() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("shared.txt");
    std::fs::write(&path, "before\n").expect("target");
    let target = existing_target(&path, "shared.txt");
    let resources = external_target_resources_for(WorkspaceId::new(), SessionId::new(), &target)
        .expect("resources");
    assert_eq!(resources.len(), 2, "an overwrite names the object and the entry");
    let kinds = resources
        .iter()
        .map(|request| match &request.resource {
            aura_workspace_domain::FileResource::Object { .. } => "object",
            aura_workspace_domain::FileResource::Slot { .. } => "slot",
            other => panic!("an existing external target named {other:?}"),
        })
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"object") && kinds.contains(&"slot"), "{kinds:?}");
}
