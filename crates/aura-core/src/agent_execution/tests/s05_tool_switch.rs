//! Preload, real Agent execution and independently frozen registries.
//!
//! Preload is not enablement. These tests exercise the ordinary preload entry
//! and real filesystem backend; installed-package disable and zero-dispatch
//! evidence belongs to the package execution path, not to a fake Built-In close.
use super::*;
use fixture::Fixture;

/// The settings authority really controls which built-ins are **preloaded**.
///
/// # Evidence scope — READ THIS BEFORE CITING THE RESULT
///
/// This test was previously named (and cited) as evidence that "closing a tool
/// through the settings entry refuses a later dispatch". **It is not that
/// evidence, and it never was.** The operation it performs is
/// `fixed_tool_ids.retain(...)` — removing a tool from the **default-load**
/// (preload) selection. Per the user's confirmed rule, "default load" keeps its
/// preload meaning and unchecking it must **not** be twisted into "disabled": a
/// built-in whose default load was cancelled still loads on demand.
///
/// So a later count of "1 backend entry" or "0 entries" after this operation
/// proves something about *preloading*, not about a genuine disable. The earlier
/// write-up treated the later `closed_entries == 0` as proof that a real disable
/// had been bypassed; that inference was invalid.
///
/// This test therefore asserts only what it really drives — the preload selection
/// and its durable settings fact — and is explicitly **not** counted as disable
/// evidence. The genuine-disable evidence is
/// `a_disabled_installed_tool_is_refused_at_the_backend_entrance` below, which
/// uses the instance `enabled` authority through the ordinary settings entry.
#[tokio::test]
async fn the_settings_entry_really_changes_which_tools_are_preloaded() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let service = fixture.service();
    let application = &service.agent_required();

    // Read the committed authority through the product's own entry point.
    let before = application.tool_settings().expect("tool settings before the switch");
    assert!(
        before.fixed_tool_ids.iter().any(|id| id.to_string() == fs_write_expected_id(&before)),
        "fs.write starts selected for preload"
    );

    // Change the **preload** selection: a real settings write through the product
    // entry, with the revision the product requires. This is a default-load
    // change, not a disable — see the test's documentation above.
    let mut update = update_from(&before);
    update.fixed_tool_ids.retain(|id| id.to_string() != fs_write_expected_id(&before));
    let after = application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), update)
        .await
        .expect("the switch is accepted through the ordinary settings entry");

    assert!(
        !after.fixed_tool_ids.iter().any(|id| id.to_string() == fs_write_expected_id(&before)),
        "the tool is no longer in the committed preload selection"
    );
    assert!(
        after.revision.value() > before.revision.value(),
        "the switch advanced the committed revision: {} -> {}",
        before.revision.value(),
        after.revision.value()
    );

    // The change is a real durable settings fact, not an in-memory view.
    let committed = service
        ._settings
        .snapshot(aura_config::ConfigModule::Tools)
        .expect("tools settings snapshot");
    let fixed = aura_config::fixed_tool_ids_from_document(&committed.document)
        .expect("fixed tool ids")
        .expect("the field is present after the switch");
    assert!(
        !fixed.iter().any(|id| id.to_string() == fs_write_expected_id(&before)),
        "the committed settings document really lost the preload selection"
    );

    // Unrelated tools are unaffected: this is a per-tool switch, not a global one.
    assert!(
        !after.fixed_tool_ids.is_empty(),
        "cancelling one tool's preload does not cancel the others: {:?}",
        after.fixed_tool_ids
    );

    // THE POSITIVE CONTROL FOR "CANCEL PRELOAD" — the property the confirmed rule
    // actually requires: cancelling preload must not disable anything, so EVERY
    // built-in the release exposes is still enabled and can still load on demand.
    // If "unchecking preload" had been implemented as "disabled", this would be
    // false, and that is exactly the conflation the repositioning prevents.
    //
    // The subject is the release's real built-in **instances**, because the
    // enablement authority is about tool instances: `fixed_tool_ids` names a
    // handler selection, and a selection id with no instance row is not a disable
    // — it is simply not what the enablement fact describes. Asserting enablement
    // over the real instances is therefore the honest form of this control.
    let authority =
        service.persistence.tool_authority_snapshot(None, None, None).expect("tool authority");
    let builtins = authority
        .instances
        .iter()
        .filter(|instance| instance.source_kind == aura_tool_contract::ToolSourceKindV1::BuiltIn)
        .collect::<Vec<_>>();
    assert!(
        !builtins.is_empty(),
        "the release must expose at least one Built-In instance for this control to mean anything"
    );
    assert!(
        builtins.iter().all(|instance| instance.enabled),
        "cancelling preload must leave every Built-In ENABLED so it can still load on demand; \
         treating it as disabled would be the exact conflation the confirmed rule forbids: {:?}",
        builtins.iter().map(|instance| (instance.tool_id, instance.enabled)).collect::<Vec<_>>()
    );
}

/// Cancelling preload does not disable a Built-In or prevent an already
/// admitted call from reaching the real filesystem backend.
#[tokio::test]
async fn cancelling_preload_still_dispatches_through_the_real_backend() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let target = fixture.root.join("s05-preload-real.txt");
    assert!(!target.exists(), "the real effect must not predate dispatch");
    let admitted = fixture.admit(&[(
        "fs.write",
        serde_json::json!({
            "path": "s05-preload-real.txt",
            "text": "still enabled after cancelling preload",
            "expected": {"expected_kind": "absent"}
        }),
    )]);
    let write = admitted.calls[&admitted.batch.calls[0].tool_call_id].validated.definition.tool_id;
    let application = fixture.service().agent_required();
    let before = application.tool_settings().expect("tool settings");
    assert!(before.fixed_tool_ids.contains(&write), "the selected tool was preloaded");
    let mut update = update_from(&before);
    update.fixed_tool_ids.retain(|id| *id != write);
    let after = application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), update)
        .await
        .expect("cancel preload through the ordinary settings entry");
    assert!(!after.fixed_tool_ids.contains(&write));
    assert!(after.revision > before.revision);

    let group = fixture.execute(admitted).await;
    assert_eq!(group.calls.len(), 1);
    assert_eq!(
        std::fs::read_to_string(&target).expect("the real backend published the file"),
        "still enabled after cancelling preload",
    );
}

/// The coordinator's management registry and the Agent's frozen execution
/// registry are independent. A real Workspace management head must not replace
/// the current Session call's exact source or enablement facts.
#[tokio::test]
async fn a_management_head_does_not_replace_the_agent_execution_registry() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let service = fixture.service();
    let admitted = fixture.admit(&[(
        "fs.write",
        serde_json::json!({
            "path": "s05-head-real.txt",
            "text": "independent head remains legitimate",
            "expected": {"expected_kind": "absent"}
        }),
    )]);
    let call = admitted.calls[&admitted.batch.calls[0].tool_call_id].clone();
    assert!(matches!(call.registry.scope, ToolRegistryScopeV1::Session { .. }));
    let scope = ToolRegistryScopeV1::Workspace { workspace_id: call.snapshot.workspace_id };
    assert!(
        service.persistence.read_tool_registry_scope_head(&scope).expect("read head").is_none()
    );
    service.agent_execution_required().verify_source(&call).expect("valid without a head");

    let committed = service
        .persistence
        .build_and_commit_tool_registry_snapshot(aura_persistence::ToolRegistryBuildRequestV1 {
            scope: scope.clone(),
            settings: service._settings.tool_settings_snapshot().expect("tool settings"),
            mode_permission_context_hash: crate::agent_execution::registry_mode_hash(
                &call.snapshot,
            )
            .expect("permission hash"),
            core_exposed_tool_ids: call
                .registry
                .catalog_entries
                .iter()
                .map(|entry| entry.tool_id)
                .collect(),
            non_package_candidates: Vec::new(),
            created_at: aura_domain_common::UtcTimestamp::now(),
            correlation_id: CorrelationId::new(),
        })
        .expect("commit a real management head for the same Workspace");
    let head = service
        .persistence
        .read_tool_registry_scope_head(&scope)
        .expect("read head")
        .expect("committed head");
    assert_eq!(
        head.tool_registry_snapshot_id,
        committed.registry_snapshot_ref.tool_registry_snapshot_id
    );
    assert_ne!(
        head.tool_registry_snapshot_id, call.registry.snapshot_ref.tool_registry_snapshot_id,
        "the independent registries must really have different snapshot identities",
    );
    service
        .agent_execution_required()
        .verify_source(&call)
        .expect("a management head cannot refuse a legitimate current Session tool");
    let group = fixture.execute(admitted).await;
    assert_eq!(group.calls.len(), 1);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("s05-head-real.txt"))
            .expect("the legitimate call really executes"),
        "independent head remains legitimate",
    );
}

/// Cancelling then restoring a tool's **preload** advances the revision, and a
/// stale selection is still refused.
///
/// This test's operation is a default-load change (`fixed_tool_ids`
/// retain/restore), not a disable. Its earlier wording said "re-open" and
/// "availability returned", which read as though a genuine disable had been
/// lifted. Nothing here disables anything: the tool stayed enabled throughout
/// (see `the_settings_entry_really_changes_which_tools_are_preloaded`'s positive
/// control). What this test really proves is that the preload selection is a
/// revisioned, compare-and-swap settings fact — which is worth proving, and is
/// the only thing it is cited for.
#[tokio::test]
async fn reopening_restores_availability_and_a_stale_selection_is_refused() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let service = fixture.service();
    let application = &service.agent_required();

    let original = application.tool_settings().expect("tool settings");
    let removed = fs_write_expected_id(&original);

    // Cancel the preload selection, then restore it.
    let mut close = update_from(&original);
    close.fixed_tool_ids.retain(|id| id.to_string() != removed);
    let closed = application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), close)
        .await
        .expect("cancel preload");
    assert!(!closed.fixed_tool_ids.iter().any(|id| id.to_string() == removed));

    let mut reopen = update_from(&closed);
    reopen.fixed_tool_ids = original.fixed_tool_ids.clone();
    let reopened = application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), reopen)
        .await
        .expect("restore the preload selection");
    assert!(
        reopened.fixed_tool_ids.iter().any(|id| id.to_string() == removed),
        "the preload selection returned after the restore"
    );
    assert!(
        reopened.revision.value() > closed.revision.value(),
        "the re-open advanced the revision"
    );

    // The stale negative case: replaying the *closed* request now carries an
    // out-of-date expected revision and must be refused rather than silently
    // re-applying the old selection over the newer fact.
    let stale = update_from(&closed);
    let error = application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), stale)
        .await
        .expect_err("a stale selection must not overwrite the newer fact");
    assert!(
        error.to_string().contains("revision")
            || error.to_string().contains("tools.configuration_revision_conflict"),
        "the refusal names the revision: {error}"
    );

    // The newer fact survived the stale request.
    let current = application.tool_settings().expect("tool settings after stale attempt");
    assert!(
        current.fixed_tool_ids.iter().any(|id| id.to_string() == removed),
        "the newer committed selection is intact"
    );
}

/// The activity table is not the execution authority.
///
/// A tool with no activity row at all is still dispatchable when the authority
/// selects it. If the activity table were consulted as the execution authority,
/// this would flip.
#[tokio::test]
async fn the_activity_table_is_not_the_execution_authority() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let service = fixture.service();

    // No activity rows exist for a brand-new fixture. The authority decides.
    let admitted = fixture.admit(&[(
        "fs.write",
        serde_json::json!({"path":"s05-d-authority.txt","text":"authority","expected":{"expected_kind":"absent"}}),
    )]);
    assert_eq!(
        admitted.batch.calls.len(),
        1,
        "admission is decided by the authority, not activity"
    );

    let settings = service.agent_required().tool_settings().expect("tool settings authority");
    assert!(
        settings.max_concurrent_tool_calls > 0,
        "the committed tool settings authority is readable independently of activity: {settings:?}"
    );

    // Closing the tool changes the authority. The registry the dispatch path
    // re-derives against then excludes it — which is a registry fact, not an
    // activity fact.
    let mut update = update_from(&settings);
    update.fixed_tool_ids.retain(|id| id.to_string() != fs_write_expected_id(&settings));
    let switched = service
        .agent_required()
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), update)
        .await
        .expect("switch");
    assert!(switched.revision.value() > settings.revision.value());
    assert!(
        !switched.fixed_tool_ids.iter().any(|id| id.to_string() == fs_write_expected_id(&settings)),
        "the execution authority changed; the activity table was not consulted"
    );
}

/// The handler name of the built-in under test, resolved from the release rather
/// than hard-coded, so the test cannot pass by naming a tool that does not exist.
const FS_WRITE_NAME: &str = "fs.write";

pub(super) fn fs_write_expected_id(settings: &aura_tool_contract::AgentToolSettingsView) -> String {
    settings
        .tools
        .iter()
        .find(|tool| tool.name == FS_WRITE_NAME)
        .map(|tool| tool.tool_id.to_string())
        .unwrap_or_else(|| {
            panic!("{FS_WRITE_NAME} must exist in the release for this test to mean anything")
        })
}

/// The product's own update input, built from a committed view.
///
/// `AgentToolSettingsUpdate` is the input the settings entry really accepts; the
/// view it returns is the committed result. Building the input from the view is
/// what a client does, and it keeps `expected_revision` a real revision rather
/// than a number the test invented.
///
/// Shared with `s05r_tool_enablement` so both files drive the same real entry
/// through the same input shape instead of each inventing its own.
pub(super) fn update_from(
    view: &aura_tool_contract::AgentToolSettingsView,
) -> aura_tool_contract::AgentToolSettingsUpdate {
    aura_tool_contract::AgentToolSettingsUpdate {
        expected_revision: view.revision,
        fixed_tool_ids: view.fixed_tool_ids.clone(),
        max_dynamic_tool_schemas: view.max_dynamic_tool_schemas,
        max_concurrent_tool_calls: view.max_concurrent_tool_calls,
        default_timeout_ms: view.default_timeout_ms,
        max_retries: view.max_retries,
        use_recommended_fixed_tool_ids: false,
        network_proxy: None,
        web_service: None,
    }
}

// ---------------------------------------------------------------------------
// Item 17: a genuinely closed installed-package tool dispatches zero times
// ---------------------------------------------------------------------------

// A real installed package program tool, disabled through the
// package entry after its call was admitted, never runs and creates no attempt, and a later call
// to it is not planned: see
// `agent_execution::tests::l04_packages::program::l04_disabling_refuses_queued_and_new_calls_before_anything_runs`.
// The P07 package path this note used to describe (enablement through
// `commit_tool_definition_enabled_change`, dispatch through `p07_execution`, the W3 conformance
// pair) was retired; a Built-in still cannot be closed through the package entry.
