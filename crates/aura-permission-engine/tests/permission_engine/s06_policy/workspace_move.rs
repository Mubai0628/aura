//! A session file approval belongs to the folder its Workspace was at when the user answered.
//! These calls are new operations on the approved target, so only that approval can cover them.
use super::*;

fn next_call(c: &mut Case) {
    c.op.operation_id = aura_domain_common::SecurityOperationId::new();
    c.op.content.action["text"] = serde_json::json!("second");
    c.refresh();
}

fn covered(c: &Case) -> bool {
    matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. })
}

fn moved() -> Option<ApprovalWorkspace> {
    Some(ApprovalWorkspace {
        canonical_root: r"D:\Moved\Workspace".into(),
        authorization_epoch: Revision::new(2),
    })
}

#[test]
fn a_session_file_approval_covers_nothing_once_its_workspace_points_at_another_folder() {
    let mut c = session_file_case();
    next_call(&mut c);
    let authority = c.allow();
    let request = c.request(&authority);
    // The move has committed. The approval is still in force here, as it is when the withdrawal
    // after the move never ran.
    c.workspace = moved();
    assert!(!covered(&c), "an approval given at the old folder covers a call at the new one");
    assert!(
        dispatch_guard::revalidate(request, &authority, &c.op, &c.input(), &c.backend, &cancel())
            .is_err(),
        "a call decided before the move is reserved after it"
    );
}

#[test]
fn an_approval_answered_with_a_network_question_covers_nothing_after_a_move() {
    let mut c = session_file_case();
    // One dialog answering a network and a file question leaves the file Grant limited to the
    // capabilities it was asked for, and such a Grant is read capability by capability.
    c.grants[0].scope.capability_limit = Some(c.op.content.capabilities.clone());
    next_call(&mut c);
    assert!(covered(&c), "the answer covers the file while its Workspace stays where it is");
    c.workspace = moved();
    assert!(!covered(&c), "an answer given at the old folder covers a call at the new one");
}

#[test]
fn archiving_and_restoring_a_workspace_keep_its_session_file_approvals() {
    let mut c = session_file_case();
    next_call(&mut c);
    // Each raises the authorization epoch; neither changes the folder.
    c.workspace.as_mut().expect("the case names its Workspace").authorization_epoch =
        Revision::new(3);
    assert!(covered(&c));
}

#[test]
fn an_approval_without_its_folder_covers_only_a_workspace_at_its_first_epoch() {
    let mut c = session_file_case();
    c.grants[0].scope.file_consent.as_mut().expect("a file approval").workspace_root = None;
    next_call(&mut c);
    assert!(covered(&c), "every move raises the epoch, so none can have committed since");
    c.workspace.as_mut().expect("the case names its Workspace").authorization_epoch =
        Revision::new(2);
    assert!(!covered(&c), "a move, an archive or a restore has happened since the answer");
}

#[test]
fn a_caller_that_cannot_name_the_workspace_reuses_no_file_approval() {
    let mut c = session_file_case();
    next_call(&mut c);
    c.workspace = None;
    assert!(!covered(&c));
}
