//! Release compatibility: an existing store that already holds the previous
//! built-in release must keep those records and accept the new release beside
//! them.
//!
//! This is the regression for the failure mode where a Tool's definition
//! content changes while its definition revision and instance identity stay the
//! same: the Store correctly refuses same-key content changes, so every tool
//! batch admission — which registers the whole current release manifest — would
//! fail against a database written by the earlier release, even for a Tool whose
//! own definition did not change.
//!
//! The baseline manifest is the exact artifact published at R10, kept as a
//! fixture so the comparison is against real shipped facts rather than a
//! hand-written approximation.
use aura_persistence_v2::{V2Store, tool::definitions};
use aura_tool_contract::ToolSettingsSnapshotV1;
use aura_tool_registry::agent_release::{AgentBuiltinManifest, release};

/// The built-in release exactly as published before this batch.
const BASELINE_RELEASE: &[u8] =
    include_bytes!("../../../fixtures/tools/builtin-agent-release.v1.r10-baseline.json");

fn baseline() -> AgentBuiltinManifest {
    serde_json::from_slice(BASELINE_RELEASE).expect("baseline release manifest")
}

fn store() -> (tempfile::TempDir, V2Store) {
    let directory = tempfile::tempdir().expect("directory");
    let store = V2Store::create(&directory.path().join("store")).expect("store");
    (directory, store)
}

#[test]
fn the_previous_release_is_registered_then_the_new_release_is_accepted_beside_it() {
    let baseline = baseline();
    let current = release().expect("current release");
    let (_directory, mut store) = store();

    // A store written by the previous release: every one of its definitions and
    // instances is already durable.
    store
        .database
        .unit_of_work(|tx| -> Result<(), aura_persistence_core::PersistenceCoreError> {
            for entry in &baseline.entries {
                definitions::put(tx, &entry.definition, &entry.instance)?;
            }
            Ok(())
        })
        .expect("the previous release registers cleanly");

    // Registering the whole current manifest is what a Tool batch admission does.
    // It must not conflict with what the previous release left behind.
    store
        .database
        .unit_of_work(|tx| -> Result<(), aura_persistence_core::PersistenceCoreError> {
            for entry in &current.manifest().entries {
                definitions::put(tx, &entry.definition, &entry.instance)?;
            }
            Ok(())
        })
        .expect("the new release must be accepted alongside the previous one");

    // The old records are preserved verbatim: an upgrade never rewrites or
    // removes shipped history.
    for entry in &baseline.entries {
        let stored = definitions::get(
            store.database.connection(),
            entry.definition.tool_id,
            &entry.definition.tool_version,
        )
        .expect("read previous definition")
        .expect("the previous definition is still present");
        assert_eq!(stored, entry.definition, "{}", entry.definition.canonical_name);
    }

    // The new records are present and usable.
    for entry in &current.manifest().entries {
        let stored = definitions::get(
            store.database.connection(),
            entry.definition.tool_id,
            &entry.definition.tool_version,
        )
        .expect("read current definition")
        .expect("the current definition is present");
        assert_eq!(stored, entry.definition, "{}", entry.definition.canonical_name);
    }

    // Re-registering the same release is idempotent.
    store
        .database
        .unit_of_work(|tx| -> Result<(), aura_persistence_core::PersistenceCoreError> {
            for entry in &current.manifest().entries {
                definitions::put(tx, &entry.definition, &entry.instance)?;
            }
            Ok(())
        })
        .expect("re-registering the same release is idempotent");
}

#[test]
fn every_changed_definition_took_a_new_revision_and_a_new_instance_identity() {
    let baseline = baseline();
    let current = release().expect("current release");
    let baseline_by_name = baseline
        .entries
        .iter()
        .map(|entry| (entry.definition.canonical_name.clone(), entry))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut changed = Vec::new();
    for entry in &current.manifest().entries {
        let Some(previous) = baseline_by_name.get(&entry.definition.canonical_name) else {
            continue;
        };
        let definition_changed = previous.definition != entry.definition
            || previous.instance.source_contract_fingerprint
                != entry.instance.source_contract_fingerprint;
        if !definition_changed {
            // An unchanged Tool keeps its exact published identity.
            assert_eq!(
                previous.definition.tool_version, entry.definition.tool_version,
                "{} changed nothing and must not move revision",
                entry.definition.canonical_name
            );
            assert_eq!(
                previous.instance.tool_instance_id, entry.instance.tool_instance_id,
                "{} changed nothing and must not move identity",
                entry.definition.canonical_name
            );
            continue;
        }
        assert!(
            entry.definition.tool_version.definition_revision.value()
                > previous.definition.tool_version.definition_revision.value(),
            "{} changed content and must take a new definition revision",
            entry.definition.canonical_name
        );
        assert_ne!(
            previous.instance.tool_instance_id, entry.instance.tool_instance_id,
            "{} changed content and must take a new instance identity",
            entry.definition.canonical_name
        );
        changed.push(entry.definition.canonical_name.clone());
    }
    // Revisions since: file schemas rewritten; web revised; the four Shell interfaces revised;
    // context, workflow and file inputs given documented defaults and shorter descriptions;
    // a question takes the user's own words, a refusal or no response; fs.write and
    // fs.patch name the file's hash after the change; tools.load and tools.unload apply
    // from the next turn, tools.search name the unified entry, tools.call, and the completion tools
    // take the evidence a model can cite.
    // Every other existing Tool must retain its published identity.
    let mut changed_names = changed.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
    changed_names.sort();
    assert_eq!(
        changed_names,
        vec![
            "context.read_events".to_owned(),
            "context.read_tool_result".to_owned(),
            "fs.list".to_owned(),
            "fs.patch".to_owned(),
            "fs.read".to_owned(),
            "fs.search".to_owned(),
            "fs.write".to_owned(),
            "goal.check".to_owned(),
            "goal.complete".to_owned(),
            "normal.complete".to_owned(),
            "plan.complete".to_owned(),
            "query.user_question".to_owned(),
            "shell.cmd".to_owned(),
            "shell.powershell".to_owned(),
            "shell.query".to_owned(),
            "shell.stop".to_owned(),
            "task_lineage.review_completion".to_owned(),
            "todo.replace".to_owned(),
            "tools.load".to_owned(),
            "tools.search".to_owned(),
            "tools.unload".to_owned(),
            "web.fetch".to_owned(),
            "web.read".to_owned(),
            "web.search".to_owned(),
        ],
        "only the declared file, web, Shell, context, workflow and registry revisions changed existing definitions"
    );
}

#[test]
fn the_current_release_registers_its_own_definitions_into_an_empty_store() {
    let current = release().expect("current release");
    let (_directory, mut store) = store();
    store
        .database
        .unit_of_work(|tx| -> Result<(), aura_persistence_core::PersistenceCoreError> {
            for entry in &current.manifest().entries {
                definitions::put(tx, &entry.definition, &entry.instance)?;
            }
            Ok(())
        })
        .expect("a fresh store accepts the current release");
    // The global settings snapshot is unaffected by a definition revision move,
    // so a restarted runtime still enforces the committed capacity.
    assert_eq!(
        ToolSettingsSnapshotV1::default().max_concurrent_tool_calls,
        aura_tool_contract::DEFAULT_MAX_CONCURRENT_TOOL_CALLS
    );
}

#[test]
fn a_definition_that_changes_again_still_conflicts_at_the_same_revision() {
    // The guard that produced the original failure is intact: a same-version
    // content change is still refused, never silently overwritten.
    let current = release().expect("current release");
    let (_directory, mut store) = store();
    store
        .database
        .unit_of_work(|tx| -> Result<(), aura_persistence_core::PersistenceCoreError> {
            definitions::put(
                tx,
                &current.manifest().entries[0].definition,
                &current.manifest().entries[0].instance,
            )?;
            Ok(())
        })
        .expect("first registration");

    let mut tampered = current.manifest().entries[0].definition.clone();
    tampered.model_description = "a different definition at the same revision".to_owned();
    let result = store.database.unit_of_work(
        |tx| -> Result<(), aura_persistence_core::PersistenceCoreError> {
            definitions::put(tx, &tampered, &current.manifest().entries[0].instance)
        },
    );
    assert!(
        result.is_err(),
        "a same-key content change must stay a conflict rather than overwrite history"
    );
}
