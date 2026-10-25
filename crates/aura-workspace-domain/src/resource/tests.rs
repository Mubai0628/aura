use aura_domain_common::{SessionId, WorkspaceId, WorkspacePath};

use super::*;

fn identity(volume: &str, id: &str) -> FileIdentity {
    FileIdentity { volume_serial_number: volume.to_owned(), file_id: id.to_owned() }
}

fn owner(path: &str) -> ResourceOwner {
    // The ids carry no identity meaning, so the test only has to give each owner a distinct
    // one; `WorkspaceId`/`SessionId` are deliberately absent from the comparison.
    ResourceOwner {
        workspace_id: WorkspaceId::new(),
        session_id: SessionId::new(),
        path: WorkspacePath::parse(path).expect("path"),
    }
}

/// A location the resolver really walked: `anchor` then `chain`, innermost last.
fn location(anchor: &str, chain: &[&str]) -> ResourceLocation {
    ResourceLocation {
        anchor: identity("vol", anchor),
        ancestry: chain.iter().map(|id| identity("vol", id)).collect(),
    }
}

fn object(
    id: &str,
    location: ResourceLocation,
    path: &str,
    access: ResourceAccess,
) -> ResourceRequest {
    ResourceRequest::new(
        FileResource::Object { identity: identity("vol", id), location, owner: owner(path) },
        access,
    )
}

fn slot(
    parent: &str,
    parent_location: ResourceLocation,
    leaf: &str,
    rule: SlotCaseRule,
    path: &str,
    access: ResourceAccess,
) -> ResourceRequest {
    ResourceRequest::new(
        FileResource::Slot {
            parent: SlotParent {
                identity: identity("vol", parent),
                location: parent_location,
                case_rule: rule,
                name_comparison: Some(ascii_comparison_fixture()),
            },
            leaf_name: leaf.to_owned(),
            owner: owner(path),
        },
        access,
    )
}

fn subtree(
    directory: &str,
    parent_location: ResourceLocation,
    path: &str,
    access: ResourceAccess,
) -> ResourceRequest {
    ResourceRequest::new(
        FileResource::Subtree {
            directory_identity: identity("vol", directory),
            location: parent_location,
            owner: owner(path),
        },
        access,
    )
}

/// An entry below a directory that does not exist yet: the verified parent plus the pending tail.
fn prospective(
    parent: &str,
    parent_location: ResourceLocation,
    pending: &[&str],
    path: &str,
) -> ResourceRequest {
    ResourceRequest::new(
        FileResource::ProspectiveEntry {
            parent: SlotParent {
                identity: identity("vol", parent),
                location: parent_location,
                case_rule: SlotCaseRule::InsensitivePreserving,
                name_comparison: Some(ascii_comparison_fixture()),
            },
            pending: pending.iter().map(|component| (*component).to_owned()).collect(),
            owner: owner(path),
        },
        ResourceAccess::Write,
    )
}

/// The confirmed cross-workspace rule: one physical file reached through two workspaces is one
/// resource. A `WorkspaceId`-keyed implementation fails here, which is exactly the defect this
/// replaces — two workspaces used to each obtain a writable claim on the same bytes.
#[test]
fn one_physical_object_is_one_resource_across_workspaces() {
    let first = object("aa", location("root", &[]), "note.txt", ResourceAccess::Write);
    let second = object("aa", location("root", &[]), "other/note.txt", ResourceAccess::Write);
    assert!(first.resource.same_resource(&second.resource), "one file, two workspaces");
    assert_eq!(
        conflict_between(&first, &second),
        Some(ConflictReason::SameObject),
        "the second workspace must not get an independent writable claim on the same bytes"
    );
}

/// The other half of the same rule: the same relative name in two unrelated workspaces is two
/// resources. A path-text-keyed implementation fails here.
#[test]
fn the_same_relative_name_in_unrelated_workspaces_is_not_one_resource() {
    let first = object("aa", location("root-one", &[]), "note.txt", ResourceAccess::Write);
    let second = object("bb", location("root-two", &[]), "note.txt", ResourceAccess::Write);
    assert!(!first.resource.same_resource(&second.resource));
    assert_eq!(
        conflict_between(&first, &second),
        None,
        "two different directories both holding a note.txt are unrelated and must run in \
         parallel"
    );
}

/// Case is folded by the rule the resolver established, and the rule is part of the request.
#[test]
fn one_slot_is_one_resource_however_it_is_spelled() {
    let upper = slot(
        "parent",
        location("root", &[]),
        "Notes.txt",
        SlotCaseRule::InsensitivePreserving,
        "Notes.txt",
        ResourceAccess::Write,
    );
    let lower = slot(
        "parent",
        location("root", &[]),
        "notes.txt",
        SlotCaseRule::InsensitivePreserving,
        "notes.txt",
        ResourceAccess::Write,
    );
    assert_eq!(conflict_between(&upper, &lower), Some(ConflictReason::SameSlot));
}

/// **The case rule is the volume's, not a self-declared lowercase.**
///
/// The pre-fix implementation called `leaf_name.to_lowercase()` inside `identity_of` and its
/// documentation asserted the Windows namespace is case-insensitive. That is a rule the module
/// asserted rather than one the resolver established, and it folds two spellings together on a
/// volume that really distinguishes them. Here the same two spellings are two entries under a
/// case-`Sensitive` rule, and under `Unknown` the request reports that it has no determinate
/// identity at all.
#[test]
fn the_case_rule_is_the_volumes_and_not_a_self_declared_lowercase() {
    let upper = slot(
        "parent",
        location("root", &[]),
        "Notes.txt",
        SlotCaseRule::Sensitive,
        "Notes.txt",
        ResourceAccess::Write,
    );
    let lower = slot(
        "parent",
        location("root", &[]),
        "notes.txt",
        SlotCaseRule::Sensitive,
        "notes.txt",
        ResourceAccess::Write,
    );
    assert_eq!(
        conflict_between(&upper, &lower),
        None,
        "a case-sensitive volume really has two distinct entries here"
    );

    let unknown = slot(
        "parent",
        location("root", &[]),
        "notes.txt",
        SlotCaseRule::Unknown,
        "notes.txt",
        ResourceAccess::Write,
    );
    assert!(
        !unknown.has_determinate_identity(),
        "an unestablished case rule is a missing fact, not a decision"
    );
    assert!(upper.has_determinate_identity());
    // Folding is the rule's, and it is visible as a rule rather than an incidental lowercase.
    assert_eq!(ascii_comparison_fixture().key("Notes.txt"), "NOTES.TXT");
    assert!(SlotCaseRule::Sensitive.is_determinate());
}

/// Unrelated leaf names under one parent are unrelated resources and stay parallel.
#[test]
fn unrelated_leaves_under_one_parent_do_not_conflict() {
    let first = slot(
        "parent",
        location("root", &[]),
        "one.txt",
        SlotCaseRule::InsensitivePreserving,
        "one.txt",
        ResourceAccess::Write,
    );
    let second = slot(
        "parent",
        location("root", &[]),
        "two.txt",
        SlotCaseRule::InsensitivePreserving,
        "two.txt",
        ResourceAccess::Write,
    );
    assert_eq!(conflict_between(&first, &second), None);
}

/// A read never takes a claim, and never conflicts with another read.
#[test]
fn reads_take_no_claim_and_never_conflict() {
    let first = object("aa", location("root", &[]), "note.txt", ResourceAccess::Read);
    let second = object("aa", location("root", &[]), "note.txt", ResourceAccess::Read);
    assert_eq!(conflict_between(&first, &second), None);
    let write = object("aa", location("root", &[]), "note.txt", ResourceAccess::Write);
    assert_eq!(
        conflict_between(&first, &write),
        Some(ConflictReason::SameObject),
        "a write still conflicts with a read of the same object"
    );
}

/// Renaming or deleting a directory conflicts with a write inside it, decided by verified
/// identity and not by path text.
#[test]
fn a_directory_scope_conflicts_with_a_write_inside_it() {
    let directory = subtree("root", location("root", &[]), "tree", ResourceAccess::SubtreeWrite);
    let inside = slot(
        "root",
        location("root", &[]),
        "file.txt",
        SlotCaseRule::InsensitivePreserving,
        "tree/file.txt",
        ResourceAccess::Write,
    );
    assert_eq!(conflict_between(&directory, &inside), Some(ConflictReason::SubtreeContains));

    let deeper =
        subtree("child", location("root", &["root"]), "tree/child", ResourceAccess::SubtreeWrite);
    assert_eq!(conflict_between(&directory, &deeper), Some(ConflictReason::SubtreeContains));
}

/// **The deep-descendant case an identity-only object cannot express.**
///
/// The pre-fix `FileResource::Object` carried no location at all, so a file several directories
/// below a directory being replaced could not be placed inside it: the object branch of
/// `subtree_reaches` was hard-coded `false`. The verified chain answers it.
///
/// The object's location is the chain **of its own parent directory**, so `["root", "a", "b"]`
/// means the file sits directly in `root/a/b`, and `root` is the scope below it.
#[test]
fn a_deep_descendant_object_is_inside_the_directory_being_replaced() {
    let scope = subtree("tree", location("root", &["root"]), "tree", ResourceAccess::SubtreeWrite);
    let deep = object(
        "deep-file",
        location("root", &["root", "tree", "a", "b"]),
        "tree/a/b/c.txt",
        ResourceAccess::Write,
    );
    assert_eq!(
        conflict_between(&scope, &deep),
        Some(ConflictReason::SubtreeContains),
        "a file under tree/a/b is inside tree and must be ordered against replacing it"
    );

    let deeper = object(
        "deeper-file",
        location("root", &["root", "tree", "a", "b", "c", "d"]),
        "tree/a/b/c/d/e.txt",
        ResourceAccess::Write,
    );
    assert_eq!(conflict_between(&scope, &deeper), Some(ConflictReason::SubtreeContains));

    // A chain of the same depth under the *same* anchor but not through `tree` is unrelated:
    // identity decides, not depth and not a shared anchor.
    let elsewhere = object(
        "other-file",
        location("root", &["root", "sibling", "a", "b"]),
        "sibling/a/b/c.txt",
        ResourceAccess::Write,
    );
    assert_eq!(
        conflict_between(&scope, &elsewhere),
        None,
        "an unrelated chain of the same depth and the same anchor is not inside this one"
    );
}

/// **The false conflict the pre-fix `slot_is_subtree_root` produced.**
///
/// It compared the slot's *parent identity* against the subtree's **last** ancestor, so a scope
/// directory `child` whose parent is `P` made every slot under `P` look like `child`'s own
/// entry. The measured fact was `Some(SubtreeContains)` for a completely unrelated sibling.
#[test]
fn a_sibling_slot_under_a_shared_parent_is_not_the_subtree_root() {
    let scope =
        subtree("child", location("root", &["root", "P"]), "P/child", ResourceAccess::SubtreeWrite);

    let sibling = slot(
        "P",
        location("root", &["root", "P"]),
        "unrelated.txt",
        SlotCaseRule::InsensitivePreserving,
        "P/unrelated.txt",
        ResourceAccess::Write,
    );
    assert_eq!(
        conflict_between(&scope, &sibling),
        None,
        "a shared parent identity does not make a sibling entry the directory itself"
    );

    let root_entry = slot(
        "P",
        location("root", &["root", "P"]),
        "child",
        SlotCaseRule::InsensitivePreserving,
        "P/child",
        ResourceAccess::Write,
    );
    assert_eq!(
        conflict_between(&scope, &root_entry),
        Some(ConflictReason::SubtreeContains),
        "the entry that names the directory being replaced is related"
    );
    assert_ne!(root_entry.resource.identity_of(), sibling.resource.identity_of());
}

/// **The other false conflict the pre-fix shape produced: two directories sharing a parent.**
///
/// With the ancestry folded into `ResourceIdentity::Subtree`, two sibling directories under one
/// parent compared as one identity and conflicted — the same class of mistake as the sibling
/// slot above, from the other side. A sibling is not a descendant.
#[test]
fn two_sibling_directories_do_not_conflict_even_though_they_share_a_parent() {
    let first = subtree(
        "one",
        location("root", &["root", "shared"]),
        "shared/one",
        ResourceAccess::SubtreeWrite,
    );
    let second = subtree(
        "two",
        location("root", &["root", "shared"]),
        "shared/two",
        ResourceAccess::SubtreeWrite,
    );
    assert_eq!(
        conflict_between(&first, &second),
        None,
        "two directories under one parent are unrelated, not subtree-contained"
    );
    assert_ne!(first.resource.identity_of(), second.resource.identity_of());

    let nested = subtree(
        "inner",
        location("root", &["root", "one"]),
        "one/inner",
        ResourceAccess::SubtreeWrite,
    );
    assert_eq!(conflict_between(&first, &nested), Some(ConflictReason::SubtreeContains));
}

/// **A nested workspace's write IS inside the parent workspace's directory scope.**
///
/// The pre-fix test here asserted the opposite — `locations_from_different_anchors_never_claim_
/// containment` — and that assertion encoded a wrong assumption about the world, which a
/// probe then measured as a real defect: with workspace A rooted at a real parent directory and
/// workspace B rooted at its `nested` child, a directory scope held by A did **not** exclude B's
/// real `fs.write_file`, which returned `Applied` and changed `deep/file.txt` from `old` to
/// `new`.
///
/// Both anchors are verified directory identities, so one anchor can be located inside the
/// other walk. This test states that containment as a fact, and keeps the genuine
/// "not comparable" rule where it belongs: chain *equality*.
#[test]
fn a_nested_workspace_write_is_inside_the_parent_workspaces_directory_scope() {
    // A's scope is the directory `root/nested`; A's own anchor is `root`.
    let directory =
        subtree("nested", location("root", &["root"]), "nested", ResourceAccess::SubtreeWrite);
    // B is a second workspace rooted *at* `root/nested`, so its anchor is the directory A's
    // scope replaces and its chain records `deep` below it.
    let nested_write =
        object("file", location("nested", &["deep"]), "deep/file.txt", ResourceAccess::Write);
    assert_eq!(
        conflict_between(&directory, &nested_write),
        Some(ConflictReason::SubtreeContains),
        "a write reached through a workspace rooted inside the directory being replaced is \
         really inside it"
    );

    // The same from the other workspace's own scope, and an entry inside it.
    let nested_entry = slot(
        "deep",
        location("nested", &["deep"]),
        "file.txt",
        SlotCaseRule::InsensitivePreserving,
        "deep/file.txt",
        ResourceAccess::Write,
    );
    assert_eq!(
        conflict_between(&directory, &nested_entry),
        Some(ConflictReason::SubtreeContains),
        "an entry under a nested workspace's directory is inside the parent scope"
    );

    // The control: a chain that shares *no* identity with the scope stays parallel, so this is
    // containment and not a blanket "everything conflicts".
    let unrelated = object(
        "other-file",
        location("other-root", &["other-root", "deep"]),
        "deep/file.txt",
        ResourceAccess::Write,
    );
    assert_eq!(
        conflict_between(&directory, &unrelated),
        None,
        "a chain that shares no verified identity with the scope is a different tree"
    );
    let sibling =
        subtree("sibling", location("root", &["root"]), "sibling", ResourceAccess::SubtreeWrite);
    assert_eq!(conflict_between(&directory, &sibling), None);
}

/// Two chains are the same chain only when they really are one walk.
///
/// This is where the anchor rule belongs. A shared ancestry suffix from two different anchors is
/// never read as one place, because "same chain" is the question that needs it — containment is
/// decided by identity, not by equality.
#[test]
fn a_shared_suffix_from_different_anchors_is_not_the_same_chain() {
    let first = location("root-one", &["a", "b"]);
    let second = location("root-two", &["a", "b"]);
    assert!(!first.same_chain(&second));
    assert!(first.same_chain(&location("root-one", &["a", "b"])));
    // Containment, by contrast, is decidable whenever one identity appears in the other walk.
    let outer = location("root", &[]);
    assert!(location("root", &["root", "nested"]).is_at_or_under(&identity("vol", "root")));
    assert!(outer.is_at_or_under(&identity("vol", "root")), "the anchor is in its own chain");
    assert!(!outer.is_at_or_under(&identity("vol", "elsewhere")));
}

/// A directory scope does not conflict with a sibling directory or with a file outside it.
#[test]
fn a_directory_scope_leaves_unrelated_siblings_parallel() {
    // `tree` and `sibling` are both directly inside `root`, so their subtrees carry the *same*
    // parent location. Only the directory identity tells them apart.
    let directory =
        subtree("tree", location("root", &["root"]), "tree", ResourceAccess::SubtreeWrite);
    let sibling =
        subtree("sibling", location("root", &["root"]), "sibling", ResourceAccess::SubtreeWrite);
    assert_eq!(
        conflict_between(&directory, &sibling),
        None,
        "two directories under one verified parent are unrelated"
    );
    let outside = slot(
        "elsewhere",
        location("root", &["root", "elsewhere"]),
        "file.txt",
        SlotCaseRule::InsensitivePreserving,
        "elsewhere/file.txt",
        ResourceAccess::Write,
    );
    assert_eq!(conflict_between(&directory, &outside), None);
    let outside_read = slot(
        "tree",
        location("root", &["root", "tree"]),
        "read-only.txt",
        SlotCaseRule::InsensitivePreserving,
        "tree/read-only.txt",
        ResourceAccess::Read,
    );
    assert_eq!(
        conflict_between(&directory, &outside_read),
        None,
        "a read inside the directory is not a claim and must not be blocked by the rename"
    );
}

/// A slot write and a scope write on the *same* directory entry really conflict; the
/// delete-recreate case must not slip through because the two sides are different variants.
#[test]
fn a_slot_write_and_a_scope_write_of_the_same_entry_conflict() {
    let entry = slot(
        "parent",
        location("root", &["root"]),
        "child",
        SlotCaseRule::InsensitivePreserving,
        "parent/child",
        ResourceAccess::Write,
    );
    let directory = subtree(
        "child",
        location("root", &["root", "parent"]),
        "parent/child",
        ResourceAccess::SubtreeWrite,
    );
    assert_eq!(
        conflict_between(&entry, &directory),
        Some(ConflictReason::SubtreeContains),
        "creating or deleting the entry that names a directory being replaced is related"
    );
}

/// The location helpers describe a real chain: a scope's parent location ends at the directory
/// the scope's own directory sits in.
#[test]
fn a_subtree_location_ends_at_the_directory_above_the_one_it_names() {
    let directory =
        subtree("child", location("root", &["root", "P"]), "P/child", ResourceAccess::SubtreeWrite);
    let FileResource::Subtree { location, .. } = &directory.resource else {
        panic!("a subtree");
    };
    assert_eq!(location.parent(), &identity("vol", "P"));
    assert_eq!(location.chain().count(), 3, "anchor, P, and nothing else");
    assert!(location.child_at(identity("vol", "child")).ancestry.contains(&identity("vol", "P")));
}

/// **Two unrelated targets below two directories that do not exist yet are two resources.**
///
/// The pre-fix model had no shape for "below a directory that does not exist", so `slot` walked
/// up to the nearest existing ancestor and kept only the last component. A probe measured
/// the consequence on the real runtime: `missing-one/note.txt` and `missing-two/note.txt` both
/// produced `slot:<root-id>/note.txt:write` and were judged `SameSlot`, so two unrelated targets
/// serialised against each other and neither future directory was covered.
///
/// The whole pending tail is carried here, so the two are distinct — and each still conflicts
/// with a directory scope that covers the directories it will really be created inside.
#[test]
fn two_targets_below_different_uncreated_parents_are_not_one_entry() {
    let first = prospective(
        "root",
        location("root", &[]),
        &["missing-one", "note.txt"],
        "missing-one/note.txt",
    );
    let second = prospective(
        "root",
        location("root", &[]),
        &["missing-two", "note.txt"],
        "missing-two/note.txt",
    );
    assert_ne!(
        first.resource.identity_of(),
        second.resource.identity_of(),
        "the uncreated intermediate component is part of the location and must not be lost"
    );
    assert_eq!(
        conflict_between(&first, &second),
        None,
        "two unrelated uncreated parents are two unrelated targets"
    );
    // The same target reached twice is still one target.
    let same = prospective(
        "root",
        location("root", &[]),
        &["missing-one", "note.txt"],
        "missing-one/note.txt",
    );
    assert_eq!(conflict_between(&first, &same), Some(ConflictReason::SameSlot));
    // Once the parent really exists, a resolver derives the entry with that parent, so the two
    // *are* different derivations of one place. They are not identity-equal — one says "under
    // `root`, the path `missing-one/note.txt`", the other says "under `missing-one`, the name
    // `note.txt`" — and they do not conflict, because the second is strictly the stronger fact
    // and the first is what a caller states only while the parent is missing. What must hold is
    // that the future parent is still covered: a scope replacing `missing-one` orders against
    // the prospective entry.
    let resolved_slot = slot(
        "missing-one",
        location("root", &["root", "missing-one"]),
        "note.txt",
        SlotCaseRule::InsensitivePreserving,
        "missing-one/note.txt",
        ResourceAccess::Write,
    );
    assert_eq!(
        conflict_between(&first, &resolved_slot),
        None,
        "the two derivations name the entry in two different ways; neither is a claim on the \
         other's resources once they are compared as entry-versus-entry"
    );
    // But the *directory* the prospective entry will be created inside is what a caller who
    // replaces it must be ordered against, and that reaches the entry through the verified
    // parent it really has: the entry sits in `root`, on the path `missing-one/note.txt`.
    //
    // A verified scope over that same `root` therefore covers it — the entry really will be
    // created inside `root`, whatever further components are still missing.
    let real_parent_scope =
        subtree("root", location("root", &[]), "root", ResourceAccess::SubtreeWrite);
    assert_eq!(
        conflict_between(&real_parent_scope, &first),
        Some(ConflictReason::SubtreeContains),
        "an entry that will be created under the directory being replaced is inside it, even \
         though the entry itself does not exist yet"
    );
    // The control: a scope over a *different* verified directory of the same shape is unrelated.
    let elsewhere_scope =
        subtree("other-root", location("other-root", &[]), "other", ResourceAccess::SubtreeWrite);
    assert_eq!(
        conflict_between(&elsewhere_scope, &first),
        None,
        "a scope over a different verified directory is a different tree"
    );
    // And the future sub-directory is protected from the *other* side too: a prospective scope
    // over `missing-one` covers an entry below it.
    let future_parent_scope =
        prospective("root", location("root", &[]), &["missing-one"], "missing-one");
    let deeper = prospective(
        "root",
        location("root", &[]),
        &["missing-one", "deeper", "note.txt"],
        "missing-one/deeper/note.txt",
    );
    assert_eq!(
        conflict_between(
            &ResourceRequest::new(future_parent_scope.resource, ResourceAccess::SubtreeWrite),
            &deeper
        ),
        Some(ConflictReason::SubtreeContains),
        "a directory that will be created covers the entries that will be created inside it"
    );
}

/// A directory scope covers an entry that will be created inside a directory that does not exist
/// yet, so "future intersecting directories" are not lost.
#[test]
fn a_directory_scope_covers_a_future_intersecting_directory() {
    // A real, verified `tree` scope.
    let scope = subtree("tree", location("root", &["root"]), "tree", ResourceAccess::SubtreeWrite);
    // A write whose parent chain is `root/tree/new/deep` where nothing below `tree` exists yet.
    let future = prospective(
        "tree",
        location("root", &["root", "tree"]),
        &["new", "deep", "note.txt"],
        "tree/new/deep/note.txt",
    );
    assert_eq!(
        conflict_between(&scope, &future),
        Some(ConflictReason::SubtreeContains),
        "a file that will be created inside the directory being replaced is inside it"
    );
    // An unrelated future path of the same shape stays parallel.
    let elsewhere = prospective(
        "other",
        location("root", &["root", "other"]),
        &["new", "deep", "note.txt"],
        "other/new/deep/note.txt",
    );
    assert_eq!(conflict_between(&scope, &elsewhere), None);
}

/// A prospective entry is placed by the deepest directory that really exists, so it is covered
/// across a nested workspace too — the two repairs compose.
#[test]
fn a_future_entry_in_a_nested_workspace_is_still_inside_the_parent_scope() {
    let scope =
        subtree("nested", location("root", &["root"]), "nested", ResourceAccess::SubtreeWrite);
    // Reached through a workspace whose anchor IS the scope's directory.
    let future = prospective(
        "nested",
        location("nested", &[]),
        &["brand-new", "note.txt"],
        "brand-new/note.txt",
    );
    assert_eq!(conflict_between(&scope, &future), Some(ConflictReason::SubtreeContains));
}

/// An unknown case rule on a prospective entry is a missing fact, not a free resource.
#[test]
fn an_unknown_rule_makes_a_prospective_entry_indeterminate() {
    let request = ResourceRequest::new(
        FileResource::ProspectiveEntry {
            parent: SlotParent {
                identity: identity("vol", "root"),
                location: location("root", &[]),
                case_rule: SlotCaseRule::Unknown,
                name_comparison: None,
            },
            pending: vec!["missing".to_owned(), "note.txt".to_owned()],
            owner: owner("missing/note.txt"),
        },
        ResourceAccess::Write,
    );
    assert!(!request.has_determinate_identity());
    // An unknown rule applies no folding, so the key keeps the spelling as written — the
    // conservative direction, which can only make two slots *less* likely to compare equal.
    assert_eq!(request.lock_key(), "pending:vol:root/missing/note.txt:write");
}

/// The lock key is derived from identity, so two workspaces' same-named files never share one.
#[test]
fn the_lock_key_is_derived_from_identity_not_display_text() {
    let first = object("aa", location("root", &[]), "note.txt", ResourceAccess::Write);
    let second = object("bb", location("root", &[]), "note.txt", ResourceAccess::Write);
    assert_ne!(first.lock_key(), second.lock_key());
    let same_file_other_name =
        object("aa", location("root", &[]), "deep/other.txt", ResourceAccess::Write);
    assert_eq!(first.lock_key(), same_file_other_name.lock_key());
    assert_eq!(same_file_other_name.resource.display(), "deep/other.txt");
    assert_eq!(first.resource.display(), "note.txt");
}
