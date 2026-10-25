//! The tool switch through the **real enablement authority**.
//!
//! # Why this file exists separately from `s05_tool_switch.rs`
//!
//! `s05_tool_switch.rs` carried two pieces of evidence that were **positioned
//! wrongly**, and correcting that is the point of this file:
//!
//! * `close.fixed_tool_ids.retain(...)` is a **default-load (preload) change**,
//!   not a disable. Per the user's confirmed rule, cancelling a built-in's default
//!   load must not be twisted into "disabled" — the tool still loads on demand. A
//!   backend-entry count after *that* operation therefore proves something about
//!   preloading and nothing about a genuine disable.
//! * A `ToolEntityNotFound` branch `return`ed before the decisive
//!   `closed_entries == 0` assertion, so the required property was reported `ok`
//!   without ever running.
//!
//! # What a genuine disable is in this product (measured, not assumed)
//!
//! Two switch facts exist and they are not interchangeable:
//!
//! | fact | where it lives | entry |
//! | --- | --- | --- |
//! | preload selection | `ConfigModule::Tools`'s `fixed_tool_ids` | `agent.tools.configure` |
//! | tool enabled | a committed `tool_instances.enabled` row | `tools.tool.set_enabled` |
//!
//! Only the second is a disable. The authority that reads it is the coordinator's
//! **existing** enablement fact — the same `tool_authority_snapshot` /
//! `instance.enabled` pair that `aura_tool_registry::builder::ineligibility_reason`
//! turns into `RegistryExclusionReasonV1::Disabled`. These tests consume that fact
//! through the single port `tools_not_enabled_under_scope`, so there is no second
//! source of truth and no comparison of snapshot ids that two databases minted
//! independently.
//!
//! # Why the disable is driven on an *installed package* tool
//!
//! `commit_tool_definition_enabled_change` is **package-only** by construction: it
//! resolves the definition's `tool_package_id` and requires
//! `ToolPackageLifecycleV1::Installed`. A Built-In has no package, so a Built-In
//! genuinely **cannot** be actively disabled through this entry. That is the
//! product rule ("built-in tools are not opened to user/model/external active
//! disable") expressed in code, and it is why the genuine-disable evidence here
//! installs a real package rather than trying to disable a Built-In and treating
//! the refusal as a bypass.
use super::fixture::Fixture;
use super::s05_tool_switch::{fs_write_expected_id, update_from};
use super::*;

/// Agent handlers come from the verified Core release, not from the separate
/// persisted registry-management release or a journal-only mutable flag.
#[tokio::test]
async fn the_verified_agent_release_is_visible_to_live_enablement() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = fixture.workspace_scope();
    let release = aura_tool_registry::agent_release::release().expect("verified Agent release");
    for name in ["fs.write", "fs.read", "shell.query", "normal.complete"] {
        let entry = release.entry(name).expect("real Agent handler");
        assert!(
            live_enablement(&fixture, &scope, entry.definition.tool_id)
                .expect("real Session ownership and live enablement are readable"),
            "a verified current Agent handler must not be reported disabled: {name}",
        );
    }
    assert!(
        !live_enablement(&fixture, &scope, aura_domain_common::ToolId::new())
            .expect("an unknown tool yields a negative fact"),
        "including a verified release must not permit arbitrary tools",
    );
}

/// A frozen journal record remains enabled after a real package disable. It
/// cannot override the coordinator's current switch: the two tables own
/// different facts despite sharing the same name.
#[tokio::test]
async fn a_frozen_journal_instance_does_not_override_a_real_disable() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = fixture.workspace_scope();
    let (tool_id, revision) = install_disableable_tool(&fixture);
    let authority = fixture
        .service()
        .persistence
        .tool_authority_snapshot(None, None, None)
        .expect("current authority");
    let definition = authority
        .definitions
        .into_iter()
        .find(|definition| definition.tool_id == tool_id)
        .expect("installed definition");
    let instance = authority
        .instances
        .into_iter()
        .find(|instance| instance.tool_id == tool_id)
        .expect("installed instance");
    assert!(instance.enabled);
    fixture
        .service()
        .broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| aura_persistence_v2::tool::definitions::put(tx, &definition, &instance))
        .expect("freeze the actual installed definition and instance");
    assert!(live_enablement(&fixture, &scope, tool_id).expect("enabled positive control"));
    set_enabled(&fixture, tool_id, revision, false).expect("ordinary disable commits");
    let frozen_payload: String = fixture
        .service()
        .broker
        .lock()
        .expect("journal")
        .database
        .connection()
        .query_row(
            "SELECT payload FROM tool_instances WHERE id=?1",
            [instance.tool_instance_id.to_string()],
            |row| row.get(0),
        )
        .expect("read the real frozen journal record");
    let frozen: aura_tool_domain::ToolInstanceRecordV1 =
        aura_persistence_core::record::decode(&frozen_payload).expect("frozen instance");
    assert!(frozen.enabled, "the immutable execution record was not rewritten");
    assert!(
        !live_enablement(&fixture, &scope, tool_id).expect("live switch remains readable"),
        "the old execution record cannot undo a committed disable",
    );
}

/// The real Session owner is checked on the Agent Store's connection before
/// asking the coordinator's live enablement port. Journal records are not
/// substituted for the live switch, and scope refusals remain distinct from
/// configuration disablement. This follows the production dispatch ordering.
fn live_enablement(
    fixture: &Fixture,
    scope: &aura_tool_contract::ToolRegistryScopeV1,
    tool_id: aura_domain_common::ToolId,
) -> anyhow::Result<bool> {
    // The **same** production wiring the execution path uses: the Agent Store answers
    // ownership from its own rows, and only then is the coordinator's enablement state
    // read. Driving this through `Execution`'s own seam rather than a test-only
    // shortcut is what makes the evidence about the product rather than about the test.
    let service = fixture.service();
    let connection = service
        .broker
        .lock()
        .map_err(|_| anyhow::anyhow!("live enablement could not be read: journal lock"))?;
    let ownership = service
        .agent_execution_required()
        .store
        .verify_scope_ownership_on(connection.database.connection(), scope);
    drop(connection);
    if !ownership.permits_new_work() {
        // A refused scope is reported as its own outcome, never as "the tool is
        // disabled" — that would be a false statement about configuration.
        anyhow::bail!("live enablement refused by scope: {}", ownership.code());
    }
    fixture
        .service()
        .persistence
        .tool_enabled_under_scope(scope, tool_id)
        .map_err(|error| anyhow::anyhow!("live enablement could not be read: {error}"))
}

/// Commit a genuine disable (or re-enable) through the product's own entry.
fn set_enabled(
    fixture: &Fixture,
    tool_id: aura_domain_common::ToolId,
    expected_entity_revision: aura_domain_common::Revision,
    enabled: bool,
) -> anyhow::Result<()> {
    fixture
        .service()
        .persistence
        .commit_tool_definition_enabled_change(
            aura_persistence::ToolDefinitionEnabledChangeRequestV1 {
                tool_id,
                expected_entity_revision,
                enabled,
                changed_at: aura_domain_common::UtcTimestamp::now(),
                correlation_id: CorrelationId::new(),
            },
        )
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("the enablement entry refused: {error}"))
}

/// One tool's instance revision in the live authority.
fn instance_revision(
    fixture: &Fixture,
    tool_id: aura_domain_common::ToolId,
) -> anyhow::Result<aura_domain_common::Revision> {
    Ok(fixture
        .service()
        .persistence
        .tool_authority_snapshot(None, None, None)
        .map_err(|error| anyhow::anyhow!("tool authority: {error}"))?
        .instances
        .into_iter()
        .find(|instance| instance.tool_id == tool_id)
        .ok_or_else(|| anyhow::anyhow!("the tool has no instance in the authority"))?
        .entity_revision)
}

/// Commits controlled package/definition/instance records through the production
/// installer commit. They satisfy the Installed precondition of the real switch
/// writer and measure its durable enablement changes under a real Agent scope.
///
/// The ExternalSource and evidence hashes below are fixture data: this helper
/// does not run package validation, conformance, transport or an executable.
/// These tests therefore prove the live authority read, not backend-entry counts.
/// A Built-In has no package and cannot be actively disabled through this entry.
fn install_disableable_tool(
    fixture: &Fixture,
) -> (aura_domain_common::ToolId, aura_domain_common::Revision) {
    use aura_domain_common::{Revision, ToolId, ToolInstanceId, ToolPackageId};
    use aura_tool_contract::{
        PackageKey, PackageRelativePath, ToolAudienceV1, ToolCanonicalName, ToolInputSchemaV1,
        ToolKey, ToolResultContractV1, ToolRuntimeDescriptorV1, ToolSourceKindV1, ToolVersion,
    };
    use aura_tool_domain::{
        ToolDefinitionRecordV1, ToolExecutionSemanticsV1, ToolInstanceOverridesV1,
        ToolInstanceRecordV1, ToolPackageLifecycleV1, ToolRuntimeHealthV1,
    };

    let seed = 0x5A_u8;
    let package_id = ToolPackageId::from_bytes([seed; 16]);
    let tool_id = ToolId::from_bytes([seed.wrapping_add(1); 16]);
    let instance_id = ToolInstanceId::from_bytes([seed.wrapping_add(2); 16]);
    let package_version =
        aura_domain_common::CanonicalSemVer::new("1.0.0").expect("package version");
    let tool_version =
        ToolVersion::new(Some(package_version.clone()), Revision::INITIAL).expect("tool version");
    let package_key = PackageKey::new(format!("s05r.package{seed}")).expect("package key");
    let definition = ToolDefinitionRecordV1::new(
        tool_id,
        tool_version.clone(),
        Some(package_id),
        Some(package_key.clone()),
        Some(ToolKey::new(format!("tool{seed}")).expect("tool key")),
        ToolCanonicalName::new(format!("s05r.tool{seed}")).expect("canonical name"),
        format!("S05R Tool {seed}"),
        format!("Execute S05R tool {seed}"),
        format!("S05R tool {seed} summary"),
        ToolInputSchemaV1::new(serde_json::json!({
            "type": "object",
            "properties": {"value": {"type": "string"}},
            "required": ["value"],
            "additionalProperties": false
        }))
        .expect("input schema"),
        false,
        ToolResultContractV1::Unit,
        ToolRuntimeDescriptorV1::ExternalSource {
            source_contract_key: format!("s05r.package.{seed}.v1"),
        },
        Vec::new(),
        Vec::new(),
        ToolAudienceV1::SubagentConfigurable,
        ToolSourceKindV1::Imported,
        ToolExecutionSemanticsV1 {
            automatic_retry_allowed: false,
            repeat_is_idempotent: true,
            may_create_persistent_effects: false,
        },
        aura_domain_common::jcs::CanonicalHashV1::sha256(&[seed, 4]),
        Revision::INITIAL,
        aura_domain_common::UtcTimestamp::now(),
    )
    .expect("definition");
    let instance = ToolInstanceRecordV1 {
        tool_instance_id: instance_id,
        tool_id,
        tool_version: tool_version.clone(),
        source_kind: ToolSourceKindV1::Imported,
        source_contract_fingerprint: aura_tool_contract::ToolSourceContractFingerprintV1::package(
            ToolSourceKindV1::Imported,
            aura_tool_contract::ToolContractFingerprintV1 {
                package_runtime_closure_hash: aura_tool_contract::PackageRuntimeClosureHashV1(
                    aura_domain_common::jcs::CanonicalHashV1::sha256(&[seed, 4]),
                ),
                fingerprint: aura_domain_common::jcs::CanonicalHashV1::sha256(&[seed, 5]),
            },
        )
        .expect("package source fingerprint"),
        enabled: true,
        runtime_health: ToolRuntimeHealthV1::Healthy,
        overrides: ToolInstanceOverridesV1 {
            timeout_ms: None,
            max_retries: None,
            max_concurrent_attempts: None,
            subagent_enabled: None,
        },
        entity_revision: Revision::INITIAL,
        created_at: aura_domain_common::UtcTimestamp::now(),
        updated_at: aura_domain_common::UtcTimestamp::now(),
    };
    let package_version_record = aura_tool_domain::ToolPackageVersionRecordV1 {
        tool_package_id: package_id,
        package_version: package_version.clone(),
        package_fingerprint: aura_tool_contract::PackageFingerprintV1(
            aura_domain_common::jcs::CanonicalHashV1::sha256(&[seed, 1]),
        ),
        package_security_fingerprint: aura_tool_contract::PackageSecurityFingerprintV1(
            aura_domain_common::jcs::CanonicalBlake3HashV1::blake3(&[seed, 2]),
        ),
        manifest_hash: aura_domain_common::jcs::CanonicalHashV1::sha256(&[seed, 3]),
        package_store_relative_path: PackageRelativePath::new(format!(
            "packages/s05r-package{seed}/1.0.0"
        ))
        .expect("store path"),
        conformance_run_ids: vec![aura_domain_common::ToolConformanceRunId::from_bytes(
            [seed.wrapping_add(3); 16],
        )],
        installed_at: aura_domain_common::UtcTimestamp::now(),
        payload_strong_reference_count: 1,
        payload_gc_eligible_at: None,
    };
    let package = aura_tool_domain::ToolPackageRecordV1 {
        tool_package_id: package_id,
        package_key,
        active_version: Some(package_version.clone()),
        lifecycle: ToolPackageLifecycleV1::Installed,
        deletion_pending: false,
        entity_revision: Revision::INITIAL,
        created_at: aura_domain_common::UtcTimestamp::now(),
        updated_at: aura_domain_common::UtcTimestamp::now(),
    };

    fixture
        .service()
        .persistence
        .seed_verified_tool_package_for_test(aura_persistence::VerifiedToolPackageInstallRequest {
            package,
            package_version: package_version_record,
            definitions: vec![definition],
            instances: vec![instance],
            correlation_id: CorrelationId::new(),
        })
        .expect("the verified install entry accepts the package");

    (tool_id, Revision::INITIAL)
}

/// A genuine disable through the ordinary entry removes the tool from the live
/// enablement authority, and re-enabling restores it under a new revision.
#[tokio::test]
async fn the_ordinary_entry_really_changes_the_live_enablement_authority() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = fixture.workspace_scope();
    let (tool_id, before_revision) = install_disableable_tool(&fixture);

    // While enabled, the live authority says so. This is the positive control:
    // without it, "not enabled after the disable" would prove nothing.
    assert!(
        live_enablement(&fixture, &scope, tool_id).expect("live enablement is readable"),
        "the tool starts enabled in the live authority"
    );

    set_enabled(&fixture, tool_id, before_revision, false).expect("a genuine disable commits");

    // The **live** authority now reports the tool as not enabled: the fact the
    // dispatch path consumes, with no snapshot-id comparison involved.
    assert!(
        !live_enablement(&fixture, &scope, tool_id).expect("live enablement is readable"),
        "after the committed disable the live authority reports the tool as not enabled"
    );

    let after_revision = instance_revision(&fixture, tool_id).expect("instance revision");
    assert!(
        after_revision.value() > before_revision.value(),
        "the disable advanced the instance revision: {} -> {}",
        before_revision.value(),
        after_revision.value()
    );

    set_enabled(&fixture, tool_id, after_revision, true).expect("a re-enable commits");
    assert!(
        live_enablement(&fixture, &scope, tool_id).expect("live enablement is readable"),
        "availability returned after the re-enable, under a new revision"
    );
}

/// The product rule, measured: a **Built-In cannot be actively disabled** through
/// the enablement entry.
///
/// This is the positive control for "built-in tools are not opened to
/// user/model/external active disable". The entry refuses because the definition
/// has no package, which is the mechanism the rule rests on. If a Built-In could
/// be disabled here, the rule would be unimplemented and this test would fail.
#[tokio::test]
async fn a_builtin_cannot_be_actively_disabled_through_the_enablement_entry() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;

    let authority = fixture
        .service()
        .persistence
        .tool_authority_snapshot(None, None, None)
        .expect("tool authority");
    let builtin = authority
        .instances
        .iter()
        .find(|instance| instance.source_kind == aura_tool_contract::ToolSourceKindV1::BuiltIn)
        .map(|instance| (instance.tool_id, instance.entity_revision))
        .expect("the release has at least one Built-In tool instance");

    let refused = set_enabled(&fixture, builtin.0, builtin.1, false);
    assert!(
        refused.is_err(),
        "a Built-In must NOT be actively disableable through this entry; the rule would be \
         unimplemented if this succeeded: {refused:?}"
    );

    // And it is still enabled, which is what the rule requires.
    let scope = fixture.workspace_scope();
    assert!(
        live_enablement(&fixture, &scope, builtin.0).expect("live enablement is readable"),
        "the Built-In is still enabled after the refused disable"
    );
}

/// Cancelling a built-in's **preload** leaves it enabled, so it is not wrongly
/// rejected.
///
/// The other half of the confirmed rule: the existing default-load meaning is
/// unchanged, unchecking it must not be twisted into "disabled", and a built-in
/// whose default load was cancelled still loads on demand.
#[tokio::test]
async fn cancelling_preload_does_not_disable_a_builtin() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = fixture.workspace_scope();
    let application = fixture.service().agent_required();

    let before = application.tool_settings().expect("tool settings");
    // The entry's own preload selection: a real committed settings fact.
    let preloaded = before
        .fixed_tool_ids
        .first()
        .copied()
        .expect("the release exposes at least one preloaded tool id");
    let mut update = update_from(&before);
    update.fixed_tool_ids.retain(|id| *id != preloaded);
    application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), update)
        .await
        .expect("the preload change is accepted through the ordinary settings entry");
    let after = application.tool_settings().expect("tool settings after the change");
    assert!(
        !after.fixed_tool_ids.contains(&preloaded),
        "the preload selection really changed, so the enablement assertion below is not vacuous"
    );

    // THE POSITIVE CONTROL. The tool whose preload was cancelled must still be
    // **enabled**, because "cancel preload" is not "disable". The question is asked
    // about a built-in that really has an instance row, because that is the tool
    // the enablement authority is about: an id with no instance row is not a
    // disable, it is simply a tool the authority does not know.
    //
    // The release's built-in is the right subject: it is the tool the product
    // exposes, it is enabled, and cancelling preload must leave that untouched.
    let builtin = fixture
        .service()
        .persistence
        .tool_authority_snapshot(None, None, None)
        .expect("tool authority")
        .instances
        .iter()
        .find(|instance| instance.source_kind == aura_tool_contract::ToolSourceKindV1::BuiltIn)
        .map(|instance| instance.tool_id)
        .expect("the release has at least one Built-In tool instance");

    // Cancel that built-in's preload too, then prove its enablement is unchanged.
    let before_reselection = application.tool_settings().expect("tool settings");
    let mut second_change = update_from(&before_reselection);
    let builtin_was_preloaded = second_change.fixed_tool_ids.contains(&builtin);
    second_change.fixed_tool_ids.retain(|id| *id != builtin);
    application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), second_change)
        .await
        .expect("the second preload change is accepted");

    assert!(
        live_enablement(&fixture, &scope, builtin).expect("live enablement is readable"),
        "a built-in is still ENABLED after its preload was cancelled (was preloaded: \
         {builtin_was_preloaded}); it must still be able to load on demand"
    );

    // And the authority still knows the tool at all: preload cancellation did not
    // remove or disable its instance.
    let instance = fixture
        .service()
        .persistence
        .tool_authority_snapshot(None, None, None)
        .expect("tool authority")
        .instances
        .into_iter()
        .find(|instance| instance.tool_id == builtin)
        .expect("the built-in still has an instance after a preload change");
    assert!(
        instance.enabled,
        "the built-in's instance is still enabled, so 'cancel preload' did not become 'disable'"
    );
}

/// One tool's disable does not make an **unrelated** tool unenabled.
///
/// This is "a single unrelated tool's update must not invalidate an entire frozen
/// batch", measured on the live authority the dispatch path reads.
#[tokio::test]
async fn disabling_one_tool_does_not_disable_another() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = fixture.workspace_scope();

    // The genuinely disableable tool comes from a real installed package, because
    // a Built-In cannot be actively disabled at all (see `install_disableable_tool`).
    let (first_id, first_revision) = install_disableable_tool(&fixture);
    // The unrelated tool is a different, still-enabled instance.
    let second = fixture
        .service()
        .persistence
        .tool_authority_snapshot(None, None, None)
        .expect("tool authority")
        .instances
        .iter()
        .find(|instance| instance.enabled && instance.tool_id != first_id)
        .map(|instance| instance.tool_id)
        .expect("there is at least one other enabled tool");

    set_enabled(&fixture, first_id, first_revision, false).expect("disable the first tool");

    assert!(
        !live_enablement(&fixture, &scope, first_id).expect("live enablement is readable"),
        "the disabled tool is reported not enabled"
    );
    assert!(
        live_enablement(&fixture, &scope, second).expect("live enablement is readable"),
        "an unrelated tool is still enabled: one tool's disable is not a batch-wide invalidation"
    );

    // And the port agrees when asked about both at once: exactly one is reported.
    let both = std::collections::BTreeSet::from([first_id, second]);
    let disabled = fixture
        .service()
        .persistence
        .tools_not_enabled_under_scope_with(&scope, &both, |persistence, _scope| {
            persistence.tool_authority_snapshot(None, None, None)
        })
        .expect("the port answers");
    assert_eq!(
        disabled,
        vec![first_id],
        "exactly the disabled tool is reported, and the unrelated one is not"
    );
}

/// The live-enablement read is a **typed** outcome: a caller can always tell
/// "not enabled" from "could not ask".
///
/// A bad or missing fact must be `fail` / `not_exercised`. Treating "could not
/// ask" as "nothing is disabled" would let a broken authority pass as a
/// permissive one; treating it as "everything is disabled" would refuse all work.
#[tokio::test]
async fn the_enablement_read_distinguishes_an_unreadable_fact_from_a_negative() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = fixture.workspace_scope();
    let (tool_id, _) = install_disableable_tool(&fixture);

    // A Session scope whose session does not exist cannot be resolved into an
    // authority read: `tool_authority_snapshot` resolves the session row and
    // refuses. That refusal must reach the caller rather than being flattened
    // into "no tools are disabled".
    let unknown_session = aura_tool_contract::ToolRegistryScopeV1::Session {
        workspace_id: aura_domain_common::WorkspaceId::new(),
        session_id: aura_domain_common::SessionId::new(),
        turn_task_id: aura_domain_common::TurnTaskId::new(),
        agent_run_id: aura_domain_common::AgentRunId::new(),
    };
    let requested = std::collections::BTreeSet::from([tool_id]);
    let unreadable =
        fixture.service().persistence.tools_not_enabled_under_scope(&unknown_session, &requested);
    assert!(
        unreadable.is_err(),
        "an unresolvable scope must be an error, not an empty disabled set: {unreadable:?}"
    );

    // The real scope resolves, so a genuine fact is obtainable and a negative
    // answer from it is meaningful.
    let readable = live_enablement(&fixture, &scope, tool_id);
    assert!(readable.is_ok(), "the real scope yields a real fact: {readable:?}");
}

/// The settings entry's **preload** change advances the committed revision, and a
/// stale selection is refused.
///
/// Cited only for what it is: a revisioned compare-and-swap on the preload
/// selection. It is **not** disable evidence.
#[tokio::test]
async fn a_preload_change_is_a_revisioned_compare_and_swap() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let application = fixture.service().agent_required();

    let original = application.tool_settings().expect("tool settings");
    let removed = fs_write_expected_id(&original);

    let mut change = update_from(&original);
    change.fixed_tool_ids.retain(|id| id.to_string() != removed);
    let changed = application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), change)
        .await
        .expect("the preload change is accepted");
    assert!(!changed.fixed_tool_ids.iter().any(|id| id.to_string() == removed));
    assert!(
        changed.revision.value() > original.revision.value(),
        "the change advanced the committed revision"
    );

    // A stale replay of the *original* selection must be refused rather than
    // silently re-applied over the newer fact.
    let stale = update_from(&original);
    let error = application
        .update_tool_settings(aura_domain_common::CommandInvocationId::new(), stale)
        .await
        .expect_err("a stale selection must not overwrite the newer fact");
    assert!(
        error.to_string().contains("revision")
            || error.to_string().contains("tools.configuration_revision_conflict"),
        "the refusal names the revision: {error}"
    );

    // The newer fact survived.
    let current = application.tool_settings().expect("tool settings after the stale attempt");
    assert!(!current.fixed_tool_ids.iter().any(|id| id.to_string() == removed));
    assert_eq!(current.revision.value(), changed.revision.value());
}

/// Two independently built registries mint their **own** snapshot ids, and that
/// difference is not by itself a judgement about legitimacy.
///
/// This is the R3 defect stated as a property. The OLD rule required the frozen
/// snapshot's `tool_registry_snapshot_id` to equal the scope head's — ids two
/// databases mint independently — so two healthy snapshots describing the same
/// enabled tool set could disagree and a legitimate call was refused.
///
/// # Why the two snapshots are built at the parent-registry layer
///
/// `build_and_commit_tool_registry_snapshot` resolves a Session scope through the
/// coordinator's `sessions` / `turn_tasks` rows, which this Agent fixture does not
/// create (it writes its turn into the broker journal). That is a harness
/// limitation, and it does not touch the property under test: the property is
/// about **what the ids are and what the rule does with them**, and the ids are
/// minted by the same builder either way. So the snapshots are built with the
/// parent-registry builder the fixture itself uses, which needs no session row.
#[tokio::test]
async fn two_built_registries_mint_distinct_ids_and_ids_are_not_the_judgement() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = fixture.workspace_scope();
    let first = fixture.registry();
    assert_eq!(first.scope, scope, "the fixture's head is the scope under test");

    // A second, independent build of the same scope through the same production
    // builder. It mints its own snapshot id, exactly like the coordinator does.
    let release = aura_tool_registry::agent_release::release().expect("the release");
    let second = aura_tool_registry::build_parent_registry_snapshot(
        aura_tool_registry::ParentRegistryBuildInputV1 {
            registry_revision: aura_domain_common::Revision::new(2),
            scope: scope.clone(),
            settings: fixture.settings_facade().tool_settings_snapshot().expect("tool settings"),
            mode_permission_context_hash: first.eligibility_context_hash.clone(),
            core_exposed_tool_ids: first.catalog_entries.iter().map(|e| e.tool_id).collect(),
            candidates: release.candidates(),
            workspace_pins: vec![],
            session_pins: vec![],
            ephemeral_loads: vec![],
            created_at: aura_domain_common::UtcTimestamp::now(),
        },
    )
    .expect("a second registry really builds")
    .snapshot;

    // The two ids differ. Under the OLD rule this alone refused a legitimate call.
    assert_ne!(
        second.snapshot_ref.tool_registry_snapshot_id, first.snapshot_ref.tool_registry_snapshot_id,
        "two independent builds mint two different snapshot ids, which is exactly why ids must not \
         be the judgement of legitimacy"
    );

    // The corrected rule's judgement is the live enablement fact, independent of
    // which snapshot id happens to be current.
    let (tool_id, revision) = install_disableable_tool(&fixture);
    assert!(
        live_enablement(&fixture, &scope, tool_id).expect("live enablement is readable"),
        "a rebuilt head does not by itself make an enabled tool not enabled"
    );

    // A genuine disable still refuses, regardless of which head is current.
    set_enabled(&fixture, tool_id, revision, false).expect("disable");
    assert!(
        !live_enablement(&fixture, &scope, tool_id).expect("live enablement is readable"),
        "a genuine disable is still refused by the corrected rule"
    );
}

// ---------------------------------------------------------------------------
// The ownership verdicts, each driven as its own case
// ---------------------------------------------------------------------------

/// The real scope this fixture's committed registry names.
fn real_scope(fixture: &Fixture) -> aura_tool_contract::ToolRegistryScopeV1 {
    fixture.workspace_scope()
}

/// The ownership verdict for a scope, read through the Agent Store's own authority.
fn ownership_verdict(
    fixture: &Fixture,
    scope: &aura_tool_contract::ToolRegistryScopeV1,
) -> aura_persistence::tool_entities::ScopeOwnershipV1 {
    let service = fixture.service();
    let connection = service.broker.lock().expect("journal lock");
    let verdict = service
        .agent_execution_required()
        .store
        .verify_scope_ownership_on(connection.database.connection(), scope);
    drop(connection);
    verdict
}

/// **A real scope is owned.** The positive control: without this, every refusal below
/// could be passing because the check refuses everything.
#[tokio::test]
async fn a_real_live_scope_is_owned() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = real_scope(&fixture);
    let verdict = ownership_verdict(&fixture, &scope);
    assert_eq!(
        verdict,
        aura_persistence::tool_entities::ScopeOwnershipV1::Owned,
        "the fixture's own committed scope must verify as owned"
    );
    assert!(verdict.permits_new_work(), "owned permits new work");
}

/// **A forged scope is refused as mismatched, not as owned.**
///
/// This is the case the old existence check could not express at all: it asked whether
/// ids existed, never whether they belonged together, so two real ids from two different
/// tasks passed it.
#[tokio::test]
async fn a_forged_scope_is_refused_as_mismatched() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = real_scope(&fixture);
    let aura_tool_contract::ToolRegistryScopeV1::Session {
        workspace_id,
        session_id,
        turn_task_id,
        agent_run_id,
    } = scope
    else {
        panic!("the fixture's registry is session-scoped");
    };

    // A session that does not belong to the named workspace. Every id is individually
    // well-formed, and the workspace-membership answer is the only thing that can
    // refuse it.
    let forged = aura_tool_contract::ToolRegistryScopeV1::Session {
        workspace_id: aura_domain_common::WorkspaceId::new(),
        session_id,
        turn_task_id,
        agent_run_id,
    };
    assert_eq!(
        ownership_verdict(&fixture, &forged),
        aura_persistence::tool_entities::ScopeOwnershipV1::Mismatched,
        "a session under a different workspace is a mismatch, not a pass"
    );

    // A run that belongs to another turn task: again individually well-formed.
    let forged_run = aura_tool_contract::ToolRegistryScopeV1::Session {
        workspace_id,
        session_id,
        turn_task_id,
        agent_run_id: aura_domain_common::AgentRunId::new(),
    };
    assert_eq!(
        ownership_verdict(&fixture, &forged_run),
        aura_persistence::tool_entities::ScopeOwnershipV1::Unknown,
        "an unknown run is refused rather than assumed to belong"
    );
    assert!(
        !ownership_verdict(&fixture, &forged_run).permits_new_work(),
        "no refusal may permit new work"
    );
}

/// **A finished task is reported as ended, not as forged.**
///
/// The distinction is the user-facing one: a stale request is not an attack, and
/// reporting it as a scope mismatch would send them looking for a security problem that
/// does not exist.
#[tokio::test]
async fn a_finished_task_is_reported_as_ended_rather_than_forged() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = real_scope(&fixture);
    let aura_tool_contract::ToolRegistryScopeV1::Session {
        workspace_id,
        session_id,
        turn_task_id,
        agent_run_id,
    } = scope
    else {
        panic!("the fixture's registry is session-scoped");
    };

    // Terminate the real turn task in the journal the owner reads.
    {
        let service = fixture.service();
        let connection = service.broker.lock().expect("journal lock");
        connection
            .database
            .connection()
            .execute(
                "UPDATE turn_tasks SET state='completed' WHERE id=?1",
                [turn_task_id.to_string()],
            )
            .expect("the fixture's own turn task is updated");
    }

    let verdict = ownership_verdict(&fixture, &scope);
    assert_eq!(
        verdict,
        aura_persistence::tool_entities::ScopeOwnershipV1::TaskEnded,
        "a real, finished task is ended rather than mismatched"
    );
    assert!(!verdict.permits_new_work(), "an ended task permits no new work");
    assert_eq!(verdict.code(), "tool.scope.task_ended");
    // And the ids still belong together, which is why this is not a mismatch.
    let _ = (workspace_id, session_id, agent_run_id);
}

/// **An unreadable owner is `Unavailable`, never a permission.**
#[tokio::test]
async fn an_unreadable_owner_is_not_permission() {
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let scope = real_scope(&fixture);

    // Rename the table the owner's run lookup reads, so the read genuinely fails at the
    // SQLite boundary. A rename rather than a drop: the fixture's foreign keys reference
    // this table, so dropping it is refused by SQLite itself, and the failure must come
    // from the owner's own read rather than from the fixture's setup.
    {
        let service = fixture.service();
        let connection = service.broker.lock().expect("journal lock");
        connection
            .database
            .connection()
            .execute_batch("ALTER TABLE agent_runs RENAME TO agent_runs_unavailable_for_this_case")
            .expect("the fixture's own schema is altered");
    }

    let verdict = ownership_verdict(&fixture, &scope);
    assert_eq!(
        verdict,
        aura_persistence::tool_entities::ScopeOwnershipV1::Unavailable,
        "a read that could not run is reported as unavailable"
    );
    assert!(!verdict.permits_new_work(), "failing to ask is never evidence of ownership");
    assert_eq!(verdict.code(), "tool.scope.unavailable");
}
