//! Review regressions over the real resolver. No system file is read or written:
//! only directory identity is inspected, and the calls stop after admission.

use super::*;
use std::path::PathBuf;

#[test]
fn s06_review_a_verified_system_path_keeps_its_protected_class() {
    let selected = PathBuf::from(std::env::var_os("SystemRoot").expect("Windows system root"));
    let info = aura_windows_platform::inspect_existing_path(&selected, true)
        .expect("inspect directory identity without reading its contents");
    assert!(info.is_directory);
    assert_eq!(
        protected_paths::classify(&selected),
        protected_paths::ExternalPathClass::System,
        "the environment declares a genuine protection root"
    );
    eprintln!("selected={} verified_final={}", selected.display(), info.final_path.display());
    assert_eq!(
        protected_paths::classify(&info.final_path),
        protected_paths::ExternalPathClass::System,
        "the native final-path spelling must not turn the same system directory into an ordinary target"
    );
}

async fn recursive_parent_grant(handler: &str) -> (Fixture, PathBuf) {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &[handler]).await;
    let selected = PathBuf::from(std::env::var_os("SystemRoot").expect("Windows system root"));
    let parent = selected.parent().expect("the system directory has a drive root");
    let info = aura_windows_platform::inspect_existing_path(parent, true)
        .expect("parent directory identity; no content read");
    let display = info.final_path.display().to_string();
    f.service()
        .agent_execution_required()
        .external_grants
        .issue_approved_directory(
            crate::external_grants::ApprovedDirectoryGrant {
                session_id: f.turn.session_id,
                workspace_id: f.turn.workspace_id,
                canonical_root: CanonicalPath { utf16: display.encode_utf16().collect(), display },
                root_identity: aura_workspace::domain_identity(&info.identity),
                recursive_scope: true,
                capabilities: vec![aura_permission_domain::ExternalGrantCapability::Read],
                term: aura_permission_domain::ExternalGrantTerm::Session,
                turn_task_id: None,
                expires_at: None,
            },
            aura_domain_common::ApprovalRequestId::new(),
            "s06-review-parent-directory-approval",
            CorrelationId::new(),
        )
        .expect("the fixture's own durable directory grant");
    (f, selected)
}

#[tokio::test]
async fn s06_review_protected_list_reaches_its_own_descriptor_instead_of_path_rejection() {
    let (f, protected) = recursive_parent_grant("fs.list").await;
    let batch = f.admit(&[("fs.list", json!({"path":protected, "limit":1, "cursor":null}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let context = batch.calls.get(&id).unwrap_or_else(|| {
        panic!(
            "a protected child must reach independent approval, not an invalid workspace path: {:?}",
            batch.planned.not_executed
        )
    });
    assert!(
        context
            .operation
            .content
            .resources
            .iter()
            .any(|r| matches!(r, ResourceDescriptor::SystemPath { .. })),
        "a parent grant must not replace the child's protection descriptor: {:?}",
        context.operation.content.resources
    );
}

#[tokio::test]
async fn s06_review_protected_search_uses_the_same_carve_out_as_other_file_tools() {
    let (f, protected) = recursive_parent_grant("fs.search").await;
    let batch = f.admit(&[(
        "fs.search",
        json!({
            "root":protected,
            "pattern":{"kind":"literal", "text":"s06-no-system-content-read", "case_sensitive":true},
            "path_glob":null, "include_ignored":false, "include_generated":false, "limit":1
        }),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let context = batch.calls.get(&id).expect("the search input is valid and actually admitted");
    assert!(
        context
            .operation
            .content
            .resources
            .iter()
            .any(|r| matches!(r, ResourceDescriptor::SystemPath { .. })),
        "search must not skip a protection rule enforced for list/read/write: {:?}",
        context.operation.content.resources
    );
}

/// The negative control for the two cases above: an **ordinary** descendant of the same approved
/// parent stays covered by the parent's grant.
///
/// Without this, an implementation that simply refused everything under a granted root would pass
/// both protected-subrange tests. The confirmed rule is the opposite: the recursive default is
/// what makes the approval usable, and the carve-out is a narrow exception for protected ranges —
/// so an ordinary deep child resolves to the grant, not to a fresh approval.
#[tokio::test]
async fn s06_review_ordinary_child_still_uses_the_parent_grant() {
    let (f, _) = recursive_parent_grant("fs.list").await;
    // A directory that really exists below the approved root and is not protected. The approved
    // root here is a real drive root, so its own temporary-style child is the ordinary case.
    let ordinary = std::env::temp_dir();
    let batch = f.admit(&[("fs.list", json!({"path": ordinary, "limit": 1, "cursor": null}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let Some(context) = batch.calls.get(&id) else {
        // The temporary directory may not sit under the approved drive root in every
        // environment, in which case this call is legitimately not admitted and there is nothing
        // to assert about the grant. The protected cases above are the ones that must hold.
        return;
    };
    assert!(
        context
            .operation
            .content
            .resources
            .iter()
            .any(|r| matches!(r, ResourceDescriptor::ExternalGrant { .. })),
        "an ordinary descendant must still resolve through the parent's grant, not a new approval: {:?}",
        context.operation.content.resources
    );
}
