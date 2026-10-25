//! The old directory grant's object scope never grants a child's contents.
use super::consent_fixture::approve_file;
use super::*;
use aura_permission_domain::{ExternalGrantCapability, ExternalGrantTerm};

#[tokio::test]
async fn s06_r1_list_find_and_search_ask_for_child_contents_before_any_attempt() {
    for name in ["fs.list", "fs.find", "fs.search"] {
        let f = Fixture::with_exposure(PermissionMode::ReadOnly, &[name]).await;
        let directory = f._host.root.path().join("legacy-grant");
        let child = directory.join("sub");
        std::fs::create_dir_all(&child).expect("create the isolated legacy grant tree");
        std::fs::write(child.join("inner.txt"), "s06-r1-deep-marker")
            .expect("seed content that needs another approval");
        let native = aura_windows_platform::inspect_existing_path(&directory, true)
            .expect("verify the legacy directory identity");
        let display = native.final_path.display().to_string();
        let legacy = f
            .service()
            .agent_execution_required()
            .external_grants
            .issue_approved_directory(
                crate::external_grants::ApprovedDirectoryGrant {
                    session_id: f.turn.session_id,
                    workspace_id: f.turn.workspace_id,
                    canonical_root: CanonicalPath {
                        utf16: display.encode_utf16().collect(),
                        display,
                    },
                    root_identity: aura_workspace::domain_identity(&native.identity),
                    recursive_scope: false,
                    capabilities: vec![ExternalGrantCapability::Read],
                    term: ExternalGrantTerm::Session,
                    turn_task_id: None,
                    expires_at: None,
                },
                ApprovalRequestId::new(),
                "old non-recursive directory approval",
                CorrelationId::new(),
            )
            .expect("persist an unchanged legacy non-recursive grant");
        let args = match name {
            "fs.list" => json!({"path":child,"limit":100,"cursor":null}),
            "fs.find" => json!({"path":child,"file_names":["inner.txt"],"recursive":true,
                "match_mode":"exact","case_sensitive":false,"limit":100,"cursor":null}),
            _ => json!({"root":child,"pattern":{"kind":"literal","text":"s06-r1-deep-marker",
                "case_sensitive":true},"path_glob":null,"include_ignored":false,
                "include_generated":false,"limit":100}),
        };
        let batch = f.admit(&[(name, args)]);
        let id = batch.batch.calls[0].tool_call_id;
        let call = batch.calls[&id].clone();
        let target = call
            .operation
            .content
            .resources
            .iter()
            .find_map(aura_security_contract::file_consent_target_of)
            .expect("the child has its own verified approval target");
        assert!(target.recursive, "new directory approvals include their contents");
        assert!(
            matches!(f.execute_rejecting(batch).await, Some(BatchOutcome::AwaitingApproval { .. })),
            "{name} must ask before it can read a child's contents"
        );
        let row = aura_persistence_v2::tool::execution::call(
            f.service().broker.lock().expect("inspect the real journal").database.connection(),
            id,
        )
        .expect("the admitted call is durable");
        assert!(row.attempt_id.is_none(), "{name} must have no Attempt while approval is missing");
        assert!(
            !f.service()
                .agent_execution_required()
                .external_grants
                .record(legacy.id)
                .expect("read the old grant after the ask")
                .recursive_scope,
            "the old grant remains non-recursive"
        );
        approve_file(&f, &call);
        let resumed = f
            .service()
            .agent_execution_required()
            .resume_batch(call.batch.tool_batch_id)
            .expect("recover the same frozen call with the new explicit approval");
        f.execute(resumed).await;
        assert!(
            f.output(id).to_string().contains("inner.txt"),
            "{name}'s actual scope agrees with the approved child's preview"
        );
    }
}
