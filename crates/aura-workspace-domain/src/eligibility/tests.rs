use aura_domain_common::{FileIdentity, Revision, SessionId, WorkspaceId, WorkspacePath};

use super::*;
use crate::resource::{ResourceLocation, SlotCaseRule, SlotParent};

/// The seam must be off unless a test armed it, or every measurement below would be about the
/// seam rather than about the owner. Guarded here once so a leaked arming is a failure rather
/// than a silently different result.
struct SeamGuard;

impl SeamGuard {
    fn off() -> Self {
        assert!(!fault::is_armed(), "a seam was still armed from an earlier test");
        Self
    }
}

impl Drop for SeamGuard {
    fn drop(&mut self) {
        fault::disarm();
    }
}

fn identity(volume: &str, id: &str) -> FileIdentity {
    FileIdentity { volume_serial_number: volume.to_owned(), file_id: id.to_owned() }
}

fn owner(workspace: WorkspaceId, session: SessionId, path: &str) -> crate::resource::ResourceOwner {
    crate::resource::ResourceOwner {
        workspace_id: workspace,
        session_id: session,
        path: WorkspacePath::parse(path).expect("path"),
    }
}

fn location(anchor: &str, chain: &[&str]) -> ResourceLocation {
    ResourceLocation {
        anchor: identity("vol", anchor),
        ancestry: chain.iter().map(|id| identity("vol", id)).collect(),
    }
}

fn object(
    workspace: WorkspaceId,
    session: SessionId,
    id: &str,
    at: ResourceLocation,
    path: &str,
) -> FileResource {
    FileResource::Object {
        identity: identity("vol", id),
        location: at,
        owner: owner(workspace, session, path),
    }
}

fn slot(
    workspace: WorkspaceId,
    session: SessionId,
    parent: &str,
    parent_location: ResourceLocation,
    leaf: &str,
) -> FileResource {
    FileResource::Slot {
        parent: SlotParent {
            identity: identity("vol", parent),
            location: parent_location,
            case_rule: SlotCaseRule::InsensitivePreserving,
            name_comparison: Some(crate::resource::ascii_comparison_fixture()),
        },
        leaf_name: leaf.to_owned(),
        owner: owner(workspace, session, leaf),
    }
}

fn subtree(
    workspace: WorkspaceId,
    session: SessionId,
    directory: &str,
    at: ResourceLocation,
    path: &str,
) -> FileResource {
    FileResource::Subtree {
        directory_identity: identity("vol", directory),
        location: at,
        owner: owner(workspace, session, path),
    }
}

fn commit_one(
    store: &EligibilityOwner,
    resource: FileResource,
    session: SessionId,
    execution: ExecutionId,
) -> Result<ResourceCommit, AdmissionRefusal> {
    store.commit(&CommitRequest::one(resource, ResourceAccess::Write), session, execution, None)
}

/// The cross-workspace claim rule: one physical file has one live edit eligibility however many
/// workspaces name it.
#[test]
fn one_physical_file_has_one_live_edit_eligibility_across_workspaces() {
    let store = EligibilityOwner::new();
    let first_workspace = WorkspaceId::new();
    let second_workspace = WorkspaceId::new();
    let first_session = SessionId::new();
    let second_session = SessionId::new();
    store
        .claim_edit(
            object(first_workspace, first_session, "aa", location("root", &[]), "note.txt"),
            first_session,
            first_workspace,
            ExecutionId::next(),
        )
        .expect("first eligibility");
    let refusal = store
        .claim_edit(
            object(second_workspace, second_session, "aa", location("root", &[]), "other/note.txt"),
            second_session,
            second_workspace,
            ExecutionId::next(),
        )
        .expect_err("the second workspace must not get an independent eligibility");
    assert_eq!(refusal.reason, ConflictReason::SameObject);
    assert_eq!(refusal.held_by, Some(first_session));
}

/// **Probe defect: `same_session_second_commit_admitted` + `first_release_clears_second`.**
///
/// The pre-fix `conflicting_hold` skipped the caller's own session and the commit map was keyed
/// by resource string, so a second commit on one object was admitted (count stayed 1) and the
/// first release then cleared the second. Measured: `second_commit_admitted: true`,
/// `commits_before_first_release: 1`, `commits_after_first_release: 0`.
///
/// Both halves must now fail: the second execution is refused, and if it were admitted the
/// first release could not end it.
#[test]
fn a_second_execution_of_one_session_cannot_hold_the_same_object() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let first_execution = ExecutionId::next();
    let second_execution = ExecutionId::next();
    let resource = object(workspace, session, "object-X", location("root", &[]), "note.txt");

    let first = commit_one(&store, resource.clone(), session, first_execution).expect("first");
    let refusal = commit_one(&store, resource.clone(), session, second_execution)
        .expect_err("one session's second execution is not exempt from exclusivity");
    assert_eq!(refusal.held_by_execution, Some(first_execution));
    assert_eq!(refusal.held_by, Some(session));
    assert!(!refusal.is_self(second_execution));

    assert_eq!(store.snapshot().commits, 1, "the object holds exactly one live commit");
    assert!(store.release_commit(first), "the first release ends the first record");
    assert_eq!(store.snapshot().commits, 0);
    // And the resource is then genuinely free for the second execution.
    commit_one(&store, resource, session, second_execution).expect("free after the real release");
}

/// **Probe defect: `released_edit_claim_still_accepted_at_commit`.**
///
/// The pre-fix `commit` stored the passed `claim_id` without checking anything, so a released
/// eligibility still committed. Measured: `released_edit_claim_still_accepted_at_commit: true`.
#[test]
fn a_released_edit_eligibility_cannot_still_commit() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let execution = ExecutionId::next();
    let resource = object(workspace, session, "object-X", location("root", &[]), "note.txt");
    let claim =
        store.claim_edit(resource.clone(), session, workspace, execution).expect("eligibility");
    assert!(store.release_claim(&claim.claim_id, session));

    let refusal = store
        .commit(
            &CommitRequest::one(resource, ResourceAccess::Write),
            session,
            execution,
            Some(&claim),
        )
        .expect_err("a released eligibility is not a token");
    assert_eq!(refusal.reason, ConflictReason::SameObject);
    assert!(!refusal.retryable, "a stale token is not a wait-for-the-holder conflict");
}

/// **Probe defect: `claim_token_not_required`.**
///
/// A long eligibility was live on the object and a commit that presented no token was still
/// admitted, because `conflicting_hold` skipped the caller's own session. Measured:
/// `claimed_object_commit_without_claim_token: true`.
///
/// An execution that holds the eligibility may commit under it; an execution that does not
/// must not inherit it just because the session matches.
#[test]
fn a_live_eligibility_held_by_another_execution_is_not_free_to_commit() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let holder = ExecutionId::next();
    let other = ExecutionId::next();
    let resource = object(workspace, session, "object-X", location("root", &[]), "note.txt");
    let claim =
        store.claim_edit(resource.clone(), session, workspace, holder).expect("eligibility");

    let refusal = commit_one(&store, resource.clone(), session, other)
        .expect_err("a live long eligibility excludes another execution, token or not");
    assert_eq!(refusal.held_by_execution, Some(holder));
    assert!(refusal.retryable);

    // The holder itself commits under it, and its epoch is unchanged by that commit.
    let commit = store
        .commit(&CommitRequest::one(resource, ResourceAccess::Write), session, holder, Some(&claim))
        .expect("the holder commits under its own eligibility");
    assert_eq!(commit.claim_id(), Some(claim.claim_id.as_str()));
    assert_eq!(commit.execution_id(), holder);
}

/// An eligibility whose epoch moved is not the one the caller believes it holds.
#[test]
fn an_eligibility_whose_epoch_moved_is_refused_at_commit() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let execution = ExecutionId::next();
    let resource = object(workspace, session, "object-X", location("root", &[]), "note.txt");
    let stale =
        store.claim_edit(resource.clone(), session, workspace, execution).expect("eligibility");
    let migration = store
        .migrate_claim_identity(&stale.claim_id, identity("vol", "object-Y"))
        .expect("the replacement migrates rather than duplicating")
        .expect("a migration was recorded");
    assert_eq!(migration.from, identity("vol", "object-X"));

    // The caller still holds the pre-migration epoch, which no longer names the live record.
    let _ = Revision::INITIAL;
    let refusal = store
        .commit(
            &CommitRequest::one(resource, ResourceAccess::Write),
            session,
            execution,
            Some(&stale),
        )
        .expect_err("an old-epoch eligibility must not commit");
    assert!(!refusal.retryable);
}

/// **Probe defect: `session_end_clears_live_commit`.**
///
/// `release_session` removed the session's live commits, so a still-executing side effect lost
/// its protection and another session could start the same write. Measured:
/// `ending_session_releases_live_commit: true`.
///
/// Ending a session ends its long eligibilities, never a side effect that already started.
#[test]
fn ending_a_session_does_not_release_a_still_live_commit() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let ending = SessionId::new();
    let other_session = SessionId::new();
    let ending_execution = ExecutionId::next();
    let other_execution = ExecutionId::next();
    let resource = object(workspace, ending, "object-X", location("root", &[]), "note.txt");
    let held = commit_one(&store, resource.clone(), ending, ending_execution).expect("commit");

    assert_eq!(store.release_session(ending), 0, "ending a session releases no long claims here");
    assert!(store.holds_commit(&held), "the live side effect keeps its commit");
    let refusal = commit_one(
        &store,
        object(workspace, other_session, "object-X", location("root", &[]), "note.txt"),
        other_session,
        other_execution,
    )
    .expect_err("a future session end is not proof the side effect finished");
    assert_eq!(refusal.held_by_execution, Some(ending_execution));

    // Only the owning execution's explicit release ends it.
    assert!(store.release_commit(held));
    commit_one(
        &store,
        object(workspace, other_session, "object-X", location("root", &[]), "note.txt"),
        other_session,
        other_execution,
    )
    .expect("after the real release the object is free");
}

/// A release addresses exactly one record. A token cannot be replayed: once its own record is
/// gone, releasing again reports "already gone" instead of clearing a later commit that happens
/// to name the same execution and resource.
///
/// This is the structural half of the measured `first_release_clears_second_commit`: the
/// pre-fix release removed whatever sat at the resource key, so a stale token for the first
/// commit silently ended the second.
#[test]
fn a_stale_commit_token_cannot_end_a_later_commit() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let execution = ExecutionId::next();
    let resource = object(workspace, session, "object-X", location("root", &[]), "note.txt");

    let first = commit_one(&store, resource.clone(), session, execution).expect("first");
    assert!(store.release_commit(first));
    assert_eq!(store.snapshot().commits, 0);

    // A later commit by the same execution on the same resource is a *different* record.
    let second = commit_one(&store, resource, session, execution).expect("second");
    assert_eq!(store.snapshot().commits, 1);
    assert!(
        store.holds_commit(&second),
        "the second commit is the live record, not a resurrected first"
    );
    assert!(store.release_commit(second));
    assert_eq!(store.snapshot().commits, 0);
}

/// **Probe defect: `directory_child_conflict_lost_by_owner`.**
///
/// A commit holding a directory did not catch another execution's commit on a slot directly
/// inside it, because `CommitHolder` stored no resource and compared only a string key.
/// Measured: `child_commit_admitted_while_directory_commit_held: true` while the pure
/// `conflict_between` rule already found the conflict.
#[test]
fn a_directory_commit_excludes_a_child_slot_commit() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let holder = ExecutionId::next();
    let child_execution = ExecutionId::next();
    let directory =
        subtree(workspace, session, "directory-D", location("root", &["root", "parent-P"]), "dir");
    let child = slot(
        workspace,
        session,
        "directory-D",
        location("root", &["root", "parent-P"]),
        "inside.txt",
    );
    assert!(
        conflict_between(
            &ResourceRequest::new(directory.clone(), ResourceAccess::SubtreeWrite),
            &ResourceRequest::new(child.clone(), ResourceAccess::Write),
        )
        .is_some(),
        "the rule already knew; the owner's storage was what lost it"
    );

    let held = store
        .commit(&CommitRequest::one(directory, ResourceAccess::SubtreeWrite), session, holder, None)
        .expect("directory commit");
    let refusal = commit_one(&store, child, session, child_execution)
        .expect_err("a child slot commit must be refused while the directory commit is held");
    assert_eq!(refusal.reason, ConflictReason::SubtreeContains);
    assert_eq!(refusal.held_by_execution, Some(holder));
    assert!(store.release_commit(held));
    assert_eq!(store.snapshot().commits, 0);
}

/// **Probe defect: `migration_into_another_live_claim`.**
///
/// Migration moved an eligibility onto an object another session already held, leaving two live
/// eligibilities on one target. Measured: `migration_accepted: true`,
/// `claims_on_one_target_after_migration: 2`.
#[test]
fn a_migration_into_an_object_someone_else_holds_is_refused() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let first_session = SessionId::new();
    let second_session = SessionId::new();
    let first_execution = ExecutionId::next();
    let second_execution = ExecutionId::next();
    let first = store
        .claim_edit(
            object(workspace, first_session, "object-X", location("root", &[]), "note.txt"),
            first_session,
            workspace,
            first_execution,
        )
        .expect("first eligibility");
    store
        .claim_edit(
            object(workspace, second_session, "object-Y", location("root", &[]), "other.txt"),
            second_session,
            workspace,
            second_execution,
        )
        .expect("second eligibility");

    let refusal = store
        .migrate_claim_identity(&first.claim_id, identity("vol", "object-Y"))
        .expect_err("migrating into a held object would leave two live eligibilities");
    assert_eq!(refusal.held_by_execution, Some(second_execution));
    assert_eq!(
        store
            .snapshot()
            .claims
            .iter()
            .filter(|claim| claim.resource().identity_of()
                == crate::resource::ResourceIdentity::Object(identity("vol", "object-Y")))
            .count(),
        1,
        "one target, one eligibility"
    );
}

/// **Probe defect: `same_session_cross_workspace_claim_duplicates`.**
///
/// Claiming the same object through a second `WorkspaceId` produced two eligibilities, because
/// the re-entry test included the workspace. Measured:
/// `same_session_cross_workspace_duplicate_edit_claims: 2`.
#[test]
fn one_execution_reaching_one_object_through_two_workspaces_has_one_eligibility() {
    let store = EligibilityOwner::new();
    let first_workspace = WorkspaceId::new();
    let second_workspace = WorkspaceId::new();
    let session = SessionId::new();
    let execution = ExecutionId::next();
    let resource = object(first_workspace, session, "object-X", location("root", &[]), "note.txt");
    let first =
        store.claim_edit(resource.clone(), session, first_workspace, execution).expect("first");
    let second = store
        .claim_edit(resource, session, second_workspace, execution)
        .expect("re-entry through another workspace");
    assert_eq!(first.claim_id, second.claim_id);
    assert_eq!(store.snapshot().claims.len(), 1, "one object, one eligibility");
}

/// A different object in another workspace is still a separate eligibility.
#[test]
fn unrelated_objects_are_admitted_in_parallel_in_one_workspace() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let first_session = SessionId::new();
    let second_session = SessionId::new();
    store
        .claim_edit(
            object(workspace, first_session, "aa", location("root", &[]), "one.txt"),
            first_session,
            workspace,
            ExecutionId::next(),
        )
        .expect("first");
    store
        .claim_edit(
            object(workspace, second_session, "bb", location("root", &[]), "two.txt"),
            second_session,
            workspace,
            ExecutionId::next(),
        )
        .expect("a different file must not be blocked by another session's eligibility");
}

/** A commit does not wait for an unrelated commit, and the same applies to a claim. */
#[test]
fn a_commit_does_not_wait_for_an_unrelated_commit() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let first_session = SessionId::new();
    let second_session = SessionId::new();
    let first = commit_one(
        &store,
        object(workspace, first_session, "aa", location("root", &[]), "one.txt"),
        first_session,
        ExecutionId::next(),
    )
    .expect("first commit");
    commit_one(
        &store,
        object(workspace, second_session, "bb", location("root", &[]), "two.txt"),
        second_session,
        ExecutionId::next(),
    )
    .expect("a different file must commit in parallel");
    assert!(store.release_commit(first));
}

/// The complete set is admitted as a unit, and a conflict on any member refuses the whole set —
/// never holding part of it while waiting for the rest.
#[test]
fn a_multi_target_commit_is_admitted_or_refused_as_one_set() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let holder = ExecutionId::next();
    let other = ExecutionId::next();
    let held_resource = object(workspace, session, "bb", location("root", &[]), "two.txt");
    let held = commit_one(&store, held_resource.clone(), session, holder).expect("held");

    let free = object(workspace, session, "cc", location("root", &[]), "three.txt");
    let refusal = store
        .commit(
            &CommitRequest::many(vec![free.clone(), held_resource.clone()], ResourceAccess::Write),
            session,
            other,
            None,
        )
        .expect_err("the set is judged together");
    assert_eq!(refusal.held_by_execution, Some(holder));
    assert_eq!(
        store.snapshot().commits,
        1,
        "the refused set must not have taken the free member while waiting"
    );
    assert!(store.release_commit(held));

    // With the conflict gone the whole set is admitted.
    let set = store
        .commit(
            &CommitRequest::many(vec![free, held_resource], ResourceAccess::Write),
            session,
            other,
            None,
        )
        .expect("the whole set is free");
    assert_eq!(set.resources().len(), 2);
    assert!(store.release_commit(set));
}

/// The workspace lifecycle releases its eligibilities and **reports** the commits it left,
/// because closing a workspace is not evidence that a write inside it has finished.
#[test]
fn workspace_lifecycle_releases_eligibilities_and_reports_live_commits() {
    let store = EligibilityOwner::new();
    let closing = WorkspaceId::new();
    let other = WorkspaceId::new();
    let first = SessionId::new();
    let second = SessionId::new();
    store
        .claim_edit(
            object(closing, first, "aa", location("root", &[]), "one.txt"),
            first,
            closing,
            ExecutionId::next(),
        )
        .expect("first");
    store
        .claim_edit(
            object(other, second, "bb", location("root", &[]), "two.txt"),
            second,
            other,
            ExecutionId::next(),
        )
        .expect("second");
    let live = commit_one(
        &store,
        object(closing, first, "cc", location("root", &[]), "three.txt"),
        first,
        ExecutionId::next(),
    )
    .expect("a live commit in the closing workspace");

    let release = store.release_workspace(closing);
    assert_eq!(release.released_claims, 1);
    assert_eq!(release.surviving_commits.len(), 1, "the live write is not pretended away");
    assert!(store.holds_commit(&live));
    let snapshot = store.snapshot();
    assert_eq!(snapshot.claims.len(), 1);
    assert_eq!(snapshot.claims[0].workspace_id, other);
}

/// A released eligibility is never reported as a conflict, so the refusal fact is honest.
#[test]
fn a_released_holder_is_not_reported_as_a_conflict() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let first_session = SessionId::new();
    let second_session = SessionId::new();
    let first_execution = ExecutionId::next();
    let claim = store
        .claim_edit(
            object(workspace, first_session, "aa", location("root", &[]), "note.txt"),
            first_session,
            workspace,
            first_execution,
        )
        .expect("eligibility");
    assert!(store.release_claim(&claim.claim_id, first_session));
    store
        .claim_edit(
            object(workspace, second_session, "aa", location("root", &[]), "note.txt"),
            second_session,
            workspace,
            ExecutionId::next(),
        )
        .expect("a released holder must not block the next writer");
}

/// A release by another session is refused, so one session cannot end another's eligibility.
#[test]
fn an_eligibility_cannot_be_released_by_another_session() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let owner_session = SessionId::new();
    let other = SessionId::new();
    let claim = store
        .claim_edit(
            object(workspace, owner_session, "aa", location("root", &[]), "note.txt"),
            owner_session,
            workspace,
            ExecutionId::next(),
        )
        .expect("eligibility");
    assert!(!store.release_claim(&claim.claim_id, other));
    assert_eq!(store.snapshot().claims.len(), 1, "the owner's eligibility must survive");
    assert!(store.release_claim(&claim.claim_id, owner_session));
    assert!(store.snapshot().claims.is_empty());
}

/// Ending a session releases that session's eligibilities and nothing else.
#[test]
fn ending_a_session_releases_only_its_own_eligibilities() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let ending = SessionId::new();
    let surviving = SessionId::new();
    store
        .claim_edit(
            object(workspace, ending, "aa", location("root", &[]), "one.txt"),
            ending,
            workspace,
            ExecutionId::next(),
        )
        .expect("ending eligibility");
    store
        .claim_edit(
            object(workspace, surviving, "bb", location("root", &[]), "two.txt"),
            surviving,
            workspace,
            ExecutionId::next(),
        )
        .expect("surviving eligibility");
    assert_eq!(store.release_session(ending), 1);
    let snapshot = store.snapshot();
    assert_eq!(snapshot.claims.len(), 1);
    assert_eq!(snapshot.claims[0].session_id, surviving);
}

/// Migration moves the eligibility without releasing it or duplicating it, and the old identity
/// is genuinely free afterwards because nothing is there any more.
#[test]
fn an_identity_replacement_migrates_the_live_eligibility() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let execution = ExecutionId::next();
    let claim = store
        .claim_edit(
            object(workspace, session, "aa", location("root", &[]), "note.txt"),
            session,
            workspace,
            execution,
        )
        .expect("eligibility");

    let migration = store
        .migrate_claim_identity(&claim.claim_id, identity("vol", "bb"))
        .expect("the migration is allowed")
        .expect("a migration was recorded");
    assert_eq!(migration.from, identity("vol", "aa"));
    assert_eq!(migration.to, identity("vol", "bb"));
    assert_eq!(store.snapshot().migrations.len(), 1);

    let other_execution = ExecutionId::next();
    let refusal = store
        .claim_edit(
            object(workspace, session, "bb", location("root", &[]), "note.txt"),
            session,
            workspace,
            other_execution,
        )
        .expect_err("the eligibility must still be live on the replacement");
    assert_eq!(refusal.held_by_execution, Some(execution));
    assert_eq!(refusal.reason, ConflictReason::SameObject);

    store
        .claim_edit(
            object(workspace, session, "aa", location("root", &[]), "note.txt"),
            session,
            workspace,
            other_execution,
        )
        .expect("the replaced object no longer exists, so its identity is not claimed");
}

/// An eligibility on an **unknown** case rule is a missing fact, and the owner refuses rather
/// than admitting a change as "no conflict".
#[test]
fn an_indeterminate_slot_is_not_admitted_as_free() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let holder = ExecutionId::next();
    let unknown = FileResource::Slot {
        parent: SlotParent {
            identity: identity("vol", "parent"),
            location: location("root", &[]),
            case_rule: SlotCaseRule::Unknown,
            name_comparison: None,
        },
        leaf_name: "notes.txt".to_owned(),
        owner: owner(workspace, session, "notes.txt"),
    };
    let first = commit_one(&store, unknown.clone(), session, holder)
        .expect_err("an unproven target is refused even without an existing holder");
    assert!(!first.retryable);
    assert_eq!(first.held, "file.name_comparison_unavailable");
    assert_eq!(store.snapshot().commits, 0);
    let refusal = commit_one(
        &store,
        FileResource::Slot {
            parent: SlotParent {
                identity: identity("vol", "parent"),
                location: location("root", &[]),
                case_rule: SlotCaseRule::Unknown,
                name_comparison: None,
            },
            leaf_name: "Notes.txt".to_owned(),
            owner: owner(workspace, session, "Notes.txt"),
        },
        session,
        ExecutionId::next(),
    )
    .expect_err("an undecidable entry is not proven free");
    assert_eq!(refusal.reason, ConflictReason::SameSlot);
    assert_eq!(store.snapshot().commits, 0);
}

/// **The fault seam is real, default-off, and reaches the owner's own decision.**
///
/// A directed test needs the caller's "the owner refused the complete set" branch to be
/// reachable without a second thread and without a copy of the owner. The seam is checked in
/// the same place the real conflict is, so the refusal the caller sees is the ordinary typed
/// [`AdmissionRefusal`] — nothing here fabricates a state the product cannot produce.
#[test]
fn the_admit_seam_is_default_off_and_refuses_only_what_it_is_aimed_at() {
    let guard = SeamGuard::off();
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let aimed = ExecutionId::next();
    let other = ExecutionId::next();
    let resource = object(workspace, session, "object-X", location("root", &[]), "note.txt");

    // Off by default: the commit really succeeds.
    let first = commit_one(&store, resource.clone(), session, aimed).expect("no seam is armed");
    assert!(store.release_commit(first));

    // Armed for one execution and one use only.
    fault::arm(fault::Stage::BeforeAdmit, fault::Target::Execution(aimed), 1);
    let injected = commit_one(&store, resource.clone(), session, aimed)
        .expect_err("the armed seam refuses this execution");
    assert!(!injected.retryable, "an injected refusal is not a wait-for-the-holder conflict");
    assert!(injected.held.contains("fault seam"), "{injected:?}");
    commit_one(&store, resource.clone(), session, aimed).expect("the seam had exactly one use");

    // Aimed at another execution, the seam does not touch this one.
    fault::arm(fault::Stage::BeforeAdmit, fault::Target::Execution(other), 1);
    commit_one(&store, resource.clone(), session, aimed).expect("not this execution's seam");

    // The second stage is reachable too, and it is a distinct point in the decision.
    fault::arm(fault::Stage::ConflictAtAdmit, fault::Target::Any, 1);
    commit_one(&store, resource, session, aimed).expect_err("the second seam refuses");
    assert!(fault::is_armed(), "one use was consumed but the seam record remains");
    drop(guard);
    assert!(!fault::is_armed(), "disarming really disarms");
}

/// **A copy's source is a read, not a write claim.**
///
/// `CommitRequest::many(resources, Write)` turned every resource in a set into a write claim,
/// including a copy's source — which the same module documents as a shared read. The measured
/// consequence is that two copies of one source serialised against each other and a copy looked
/// like a writer of its own source. The access now travels with the resource.
#[test]
fn a_shared_read_source_does_not_conflict_while_two_writers_still_do() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let source = object(workspace, session, "source-object", location("root", &[]), "in.txt");
    let destination_one = slot(workspace, session, "root", location("root", &[]), "out-one.txt");
    let destination_two = slot(workspace, session, "root", location("root", &[]), "out-two.txt");

    // Copy A: reads the source and writes its own destination.
    let copy_a = store
        .commit(
            &CommitRequest::per_resource(vec![
                ResourceRequest::new(destination_one, ResourceAccess::Write),
                ResourceRequest::new(source.clone(), ResourceAccess::Read),
            ]),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the first copy is admitted");

    // Copy B reads the same source and writes a *different* destination: both readers share the
    // source, so this is admitted while A is still in flight.
    let copy_b = store
        .commit(
            &CommitRequest::per_resource(vec![
                ResourceRequest::new(destination_two, ResourceAccess::Write),
                ResourceRequest::new(source.clone(), ResourceAccess::Read),
            ]),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("two readers of one source do not conflict");

    // A writer of the source, by contrast, is refused while a *reader* holds it, because a
    // write against a read really is a conflict.
    let writer = store
        .commit(
            &CommitRequest::one(source.clone(), ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect_err("a writer of a source that a copy is reading must be refused");
    assert_eq!(writer.reason, ConflictReason::SameObject);
    assert!(writer.retryable);

    assert!(store.release_commit(copy_a));
    assert!(store.release_commit(copy_b));
    // With both readers gone the writer is admitted, so the refusal above was the read and not
    // a blanket one.
    let writer = store
        .commit(
            &CommitRequest::one(source, ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the source is free once no reader holds it");
    assert!(store.release_commit(writer));
}

/// **A commit that is never released still ends, and says so.**
///
/// The brief requires that `ResourceCommit`'s real Drop match its documentation. The pre-fix
/// shape had no `Drop` at all, so a caller that returned early — an error path, a panic, a
/// future dropped mid-write — left its resource set excluded for the life of the process. Here
/// the token is dropped without release and the resource is provably free afterwards, while a
/// *live* token still excludes.
#[test]
fn dropping_an_unreleased_commit_frees_exactly_its_own_resources() {
    let store = std::sync::Arc::new(EligibilityOwner::new());
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let resource = object(workspace, session, "object-Z", location("root", &[]), "note.txt");

    let abandoned = store
        .commit(
            &CommitRequest::one(resource.clone(), ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the resource is free");
    assert_eq!(store.snapshot().commits, 1);
    assert!(
        store
            .commit(
                &CommitRequest::one(resource.clone(), ResourceAccess::Write),
                session,
                ExecutionId::next(),
                None,
            )
            .is_err(),
        "a live commit really excludes"
    );
    drop(abandoned);
    assert_eq!(
        store.snapshot().commits,
        0,
        "dropping the token ends the record rather than leaking the resource set"
    );
    let after = store
        .commit(
            &CommitRequest::one(resource, ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the resource is free again");
    // An explicit release is not an abandoned write, so it stands as the ordinary path.
    assert!(store.release_commit(after));
    assert_eq!(store.snapshot().commits, 0);
}

/// **The bounded wait is a real wait: it takes the set once the holder releases it.**
///
/// A synchronous `commit` that merely refuses is not a wait implementation — that is the defect
/// this replaces. Here a holder is released from another thread while the waiter is inside
/// `commit_waiting`, and the waiter really obtains the complete set afterwards, within the
/// deadline and without ever holding a partial set.
#[test]
fn a_bounded_wait_takes_the_complete_set_once_the_holder_releases_it() {
    let store = std::sync::Arc::new(EligibilityOwner::new());
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let first = object(workspace, session, "wait-one", location("root", &[]), "one.txt");
    let second = object(workspace, session, "wait-two", location("root", &[]), "two.txt");

    // The holder takes **only** the second resource, so a waiter asking for both must wait for
    // the whole set rather than being admitted with the first.
    let holder = store
        .commit(
            &CommitRequest::one(second.clone(), ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the holder takes the second resource");

    let release = std::thread::spawn({
        let store = std::sync::Arc::clone(&store);
        move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            assert!(store.release_commit(holder), "the holder releases its own record");
        }
    });

    let request = CommitRequest::per_resource(vec![
        ResourceRequest::new(first, ResourceAccess::Write),
        ResourceRequest::new(second, ResourceAccess::Write),
    ]);
    let wait =
        AdmissionWait::bounded_only(std::time::Instant::now() + std::time::Duration::from_secs(5));
    let started = std::time::Instant::now();
    let admitted = store
        .commit_waiting(&request, session, ExecutionId::next(), None, &wait)
        .expect("the whole set is admitted once the holder releases it");
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(50),
        "the wait really waited for the holder rather than being admitted immediately"
    );
    assert_eq!(admitted.resources().len(), 2, "the complete set, not part of it");
    assert!(store.release_commit(admitted));
    release.join().expect("the releasing thread did not panic");
}

/// The wait is **cancellable** and **bounded**: a cancellation stops it early, and the deadline
/// stops it even when nothing cancels.
#[test]
fn the_wait_stops_on_cancellation_and_on_its_own_deadline() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let resource = object(workspace, session, "busy", location("root", &[]), "note.txt");
    let holder = store
        .commit(
            &CommitRequest::one(resource.clone(), ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the holder keeps the resource");

    // Cancelled: the wait returns as soon as it is asked, well inside its deadline.
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let cancel_check = || cancelled.load(std::sync::atomic::Ordering::Acquire);
    let wait = AdmissionWait::new(
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        &cancel_check,
    );
    let started = std::time::Instant::now();
    let refusal = store
        .commit_waiting(
            &CommitRequest::one(resource.clone(), ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
            &wait,
        )
        .expect_err("a cancelled wait does not take the resource");
    assert!(refusal.retryable);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "a cancelled wait must not run to its deadline: {:?}",
        started.elapsed()
    );

    // Not cancelled, but bounded: the deadline ends it and the last refusal still names the
    // real holder rather than reporting a bare timeout.
    let never = std::sync::atomic::AtomicBool::new(false);
    let never_check = || never.load(std::sync::atomic::Ordering::Acquire);
    let wait = AdmissionWait::new(
        std::time::Instant::now() + std::time::Duration::from_millis(120),
        &never_check,
    );
    let started = std::time::Instant::now();
    let refusal = store
        .commit_waiting(
            &CommitRequest::one(resource, ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
            &wait,
        )
        .expect_err("the deadline ends the wait");
    assert!(refusal.retryable);
    assert_eq!(refusal.reason, ConflictReason::SameObject);
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(100)
            && started.elapsed() < std::time::Duration::from_secs(5),
        "the wait ran to approximately its deadline: {:?}",
        started.elapsed()
    );
    assert!(store.release_commit(holder));
}

/// The set is admitted **whole or not at all**: no resource of a refused set stays held.
#[test]
fn a_refused_set_leaves_no_part_of_itself_held() {
    let store = EligibilityOwner::new();
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let blocked = object(workspace, session, "blocked", location("root", &[]), "b.txt");
    let free = object(workspace, session, "free", location("root", &[]), "f.txt");
    let holder = store
        .commit(
            &CommitRequest::one(blocked.clone(), ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the holder takes one member of the set");

    let request = CommitRequest::per_resource(vec![
        ResourceRequest::new(free.clone(), ResourceAccess::Write),
        ResourceRequest::new(blocked, ResourceAccess::Write),
    ]);
    store
        .commit(&request, session, ExecutionId::next(), None)
        .expect_err("the complete set cannot be taken while one member is held");
    assert_eq!(
        store.snapshot().commits,
        1,
        "only the holder's own record exists: the refused set took nothing"
    );
    // The other member is genuinely untouched, so the refusal was not a partial acquisition.
    let alone = store
        .commit(
            &CommitRequest::one(free, ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("an unrelated member of the refused set was never held");
    assert!(store.release_commit(alone));
    assert!(store.release_commit(holder));
}
