//! Aura's real editor save route follows existing consent; outside replacement does not.
use super::consent_fixture::{approve_file, set_mode};
use super::*;
use serde_json::Value;
#[path = "../s07/editor_save_notice.rs"]
mod s07_notice;

struct DirtyEditorFixture {
    fixture: Fixture,
    approval: aura_security_contract::AuthorizationGrant,
    status: Value,
    completed_batch: ToolBatchId,
    group: aura_context_domain::HistoryAtomicGroupPreimageV1,
}

async fn dirty_editor_fixture() -> DirtyEditorFixture {
    let f = Fixture::with_exposure_setup(PermissionMode::ReadOnly, &["fs.write"], |root| {
        std::fs::write(root.join("editor.txt"), "before").expect("seed the editor file");
    })
    .await;
    let initial = f.admit(&[(
        "fs.write",
        json!({"path":"editor.txt","text":"approval only",
        "expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"before")}}),
    )]);
    let approved =
        approve_file(&f, initial.calls.values().next().expect("the file call is admitted"));
    let batch_id = initial.batch.tool_batch_id;
    let group = f.execute(initial).await;
    set_mode(&f, PermissionMode::WorkspaceReadWrite).await;
    let opened = crate::r03_tests::success(
        &f.runtime,
        crate::method::BUFFER_OPEN,
        json!({
            "workspace_id":f.turn.workspace_id,"session_id":f.turn.session_id,
            "path":"editor.txt","writable":true,
        }),
    )
    .await;
    let revision: Revision = serde_json::from_value(opened["buffer_revision"].clone())
        .expect("the open response contains a buffer revision");
    let edited = crate::r03_tests::success(&f.runtime, crate::method::BUFFER_EDIT, json!({
        "workspace_id":f.turn.workspace_id,"session_id":f.turn.session_id,"path":"editor.txt",
        "reservation_id":opened["reservation_id"],"reservation_epoch":opened["reservation_epoch"],
        "buffer_revision":Revision::new(revision.value()+1),
        "base_core_revision":opened["core_acknowledged_revision"],"content":"editor saved",
        "active_editing":false,"ime_composition_active":false,
    })).await;
    DirtyEditorFixture {
        fixture: f,
        approval: approved,
        status: edited["status"].clone(),
        completed_batch: batch_id,
        group,
    }
}

#[tokio::test]
async fn s06_r5_editor_save_keeps_a_file_approval_live_for_a_real_agent_write() {
    let DirtyEditorFixture { fixture: mut f, approval: approved, status, completed_batch, group } =
        dirty_editor_fixture().await;
    crate::r03_tests::success(&f.runtime, crate::method::BUFFER_SAVE, json!({
        "workspace_id":f.turn.workspace_id,"session_id":f.turn.session_id,"path":"editor.txt",
        "reservation_id":status["reservation_id"],"reservation_epoch":status["reservation_epoch"],
        "expected_buffer_revision":status["buffer_revision"],
    })).await;
    assert_eq!(
        std::fs::read(f.root.join("editor.txt")).expect("read the actual saved bytes"),
        b"editor saved"
    );
    let views = crate::r03_tests::success(
        &f.runtime,
        crate::method::FILE_SESSION_CONSENT_LIST,
        json!({"session_id":f.turn.session_id,"include_inactive":true}),
    )
    .await;
    let view = views
        .as_array()
        .expect("the management route returns a list")
        .iter()
        .find(|view| view["grant_id"] == json!(approved.grant_id))
        .expect("the original approval is still listed");
    assert_eq!(
        view["availability"], "in_force",
        "Aura's editor publication follows the existing grant: {view}"
    );
    // Stop editing through the real buffer owner before checking another execution's write.
    f.service()
        .p03_required()
        .workspace
        .buffers()
        .invalidate_workspace(f.turn.workspace_id)
        .expect("close this fixture's saved buffer and release its editor eligibility");
    set_mode(&f, PermissionMode::ReadOnly).await;
    f.continue_after_tools(completed_batch, &group);
    let next = f.admit(&[(
        "fs.write",
        json!({"path":"editor.txt","text":"agent after editor",
        "expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"editor saved")}}),
    )]);
    let id = next.batch.calls[0].tool_call_id;
    assert!(
        matches!(f.execute_rejecting(next).await, Some(BatchOutcome::Completed(_))),
        "the original file consent authorizes the new call"
    );
    f.output(id);
    assert_eq!(
        std::fs::read(f.root.join("editor.txt")).expect("read the Agent effect"),
        b"agent after editor"
    );
}

#[tokio::test]
async fn s06_r5_lifecycle_save_follows_the_same_original_file_approval() {
    let DirtyEditorFixture { fixture: f, approval: approved, .. } = dirty_editor_fixture().await;
    // Capture the lifecycle revision after the independent catalogue updater settles.
    // The real lifecycle route retains its ordinary workspace CAS.
    f.service()
        .p03_required()
        .workspace
        .stop_watchers()
        .expect("settle the catalogue before this consent-focused lifecycle transition");
    let renderer = aura_domain_common::RendererInstanceId::new();
    let workspace = f
        .service()
        .persistence
        .workspace(f.turn.workspace_id)
        .expect("read this fixture's current workspace revision");
    let began = crate::r03_tests::success(
        &f.runtime,
        crate::method::WORKSPACE_LIFECYCLE_BEGIN,
        json!({
            "workspace_id":f.turn.workspace_id,"expected_workspace_revision":workspace.revision,
            "renderer_instance_id":renderer,"operation":{"operation_kind":"archive"},
        }),
    )
    .await;
    crate::r03_tests::success(&f.runtime, crate::method::WORKSPACE_LIFECYCLE_PREPARE_DELIVERED, json!({
        "operation_id":began["operation_id"],"expected_lifecycle_revision":began["lifecycle_revision"],
        "renderer_instance_id":renderer,
    })).await;
    let buffers = f
        .service()
        .p03_required()
        .workspace
        .buffers()
        .current_lifecycle_states(f.turn.workspace_id)
        .expect("read the actual editor state for the renderer acknowledgement");
    let blocked = crate::r03_tests::success(
        &f.runtime,
        crate::method::WORKSPACE_LIFECYCLE_ACK,
        json!({
            "operation_id":began["operation_id"],"lifecycle_revision":began["lifecycle_revision"],
            "renderer_instance_id":renderer,"workspace_id":f.turn.workspace_id,"buffers":buffers,
        }),
    )
    .await;
    assert_eq!(blocked["state"], "blocked", "the real lifecycle waits for its dirty buffer");
    let completed = crate::r03_tests::success(&f.runtime, crate::method::WORKSPACE_LIFECYCLE_RESOLVE, json!({
        "operation_id":blocked["operation_id"],"expected_lifecycle_revision":blocked["lifecycle_revision"],
        "renderer_instance_id":renderer,"action":"save_all_and_continue","discard_confirmation_nonce":null,
    })).await;
    assert_eq!(completed["state"], "completed", "the real lifecycle result is {completed}");
    assert_eq!(
        std::fs::read(f.root.join("editor.txt")).expect("read the lifecycle save effect"),
        b"editor saved"
    );
    let object = aura_windows_platform::inspect_existing_path(&f.root.join("editor.txt"), true)
        .expect("inspect the object actually published by the lifecycle save");
    let journal = f.service().broker.lock().expect("read the existing approval authority store");
    let followed =
        aura_persistence_v2::security::grant(journal.database.connection(), approved.grant_id)
            .expect("read the original grant after lifecycle publication")
            .expect("publication retains the original grant");
    let mut expected =
        approved.scope.file_consent.expect("the original file approval contains its scope");
    expected.targets[0].identity_key =
        aura_security_contract::file_object_key(&aura_workspace::domain_identity(&object.identity));
    expected.grant_revision = followed.revision;
    assert_eq!(
        followed.scope.file_consent,
        Some(expected),
        "lifecycle save changes only the published identity and grant revision"
    );
}
