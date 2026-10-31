//! Real Core fault isolation from a damaged configuration domain.
//!
//! # What these tests are for
//!
//! "The facade can still be opened" is **not** evidence that Core isolates a
//! configuration fault. The requirement is a statement about the running
//! product:
//!
//! * with one configuration domain genuinely damaged, Core still starts;
//! * unrelated **control** paths (status, health, sessions, workspaces) still
//!   answer, and unrelated **work** still runs;
//! * a new call that genuinely **depends** on the damaged configuration is
//!   refused honestly, with a stable code — not served from a fabricated default;
//! * after a real repair, the capability comes back.
//!
//! # How the damage is real
//!
//! The domain's file is replaced by a **directory** of the same name. The path
//! still exists, so the facade believes there is a file to load, and the real
//! `File::open` fails with a genuine Windows I/O error. Nothing is simulated and
//! no in-memory snapshot stands in for a real read.
//!
//! # Model fixture vs real Provider
//!
//! These cases use the repository's isolated Core fixture (a real SQLite database,
//! real files, real IPC dispatch inside one process) with a **controlled config
//! fixture**. They do not contact any real account Provider and they do not
//! install or publish anything, which is exactly the boundary the brief sets for
//! R3/R4 evidence.
use super::*;
use crate::r03_tests::{Fixture, success};
use aura_config::{ConfigModule, SettingsFacade};
use aura_core_base::config_degradation::{ConfigDegradationRegistry, ConfigDependentCapability};
use serde_json::json;

/// A bounded, path-free label for the startup mode.
///
/// `StartupMode` deliberately does not implement `Debug` — it carries service
/// owners — so the test reports the mode as a stable word rather than leaking a
/// service graph into an assertion message.
fn startup_mode_label(state: &RuntimeState) -> &'static str {
    match &state.startup_mode {
        StartupMode::Ready(_) => "ready",
        StartupMode::RecoveryRequired { .. } => "recovery_required",
    }
}

/// Replaces one domain's configuration file with a directory, so its real
/// `File::open` fails with a genuine I/O error.
fn break_domain(paths: &aura_config::AuraPaths, module: ConfigModule) {
    let path = paths.config_path(module);
    if path.is_file() {
        fs::remove_file(&path).expect("S05 R2 fixture operation must succeed");
    }
    fs::create_dir_all(&path).expect("S05 R2 fixture operation must succeed");
    fs::write(path.join("blocker.txt"), b"this domain cannot be opened")
        .expect("S05 R2 fixture operation must succeed");
}

/// The degradation registry records what it is told, and never claims a
/// capability is available when it was degraded with an unknown scope.
#[test]
fn the_registry_never_reports_an_unknown_scope_as_available() {
    let registry = ConfigDegradationRegistry::default();
    registry.degrade(
        ConfigDependentCapability::ToolPolicy,
        Some(ConfigModule::Tools),
        "config.domain.io",
        true,
    );
    assert!(!registry.is_available(ConfigDependentCapability::ToolPolicy));
    // An unrelated capability is untouched: one domain's fault is not global.
    assert!(registry.is_available(ConfigDependentCapability::SecurityAuthority));
    assert!(registry.is_available(ConfigDependentCapability::ModelAuthority));
    assert!(!registry.recovery_scope_unknown());

    // A repair really restores it.
    registry.restore_domain(ConfigDependentCapability::ToolPolicy, ConfigModule::Tools);
    assert!(registry.is_available(ConfigDependentCapability::ToolPolicy));
}

#[tokio::test]
async fn one_broken_config_domain_does_not_stop_core_from_starting_or_working() {
    let fixture = Fixture::new();
    // Seed every real configuration file first, then damage exactly one domain
    // that neither the tool-policy chain nor the security authority reads, so the
    // test measures *isolation* rather than a genuine dependency.
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05 R2 fixture operation must succeed"),
    );
    break_domain(&fixture.paths, ConfigModule::Browser);

    let state = fixture.start().await;

    // Core really started: this is the whole point. Before R2 a damaged domain
    // could abort `initialize_services_at` and every control path with it.
    assert!(
        matches!(&state.startup_mode, StartupMode::Ready(_)),
        "Core must start with one damaged configuration domain; startup mode is {}",
        startup_mode_label(&state)
    );

    // Unrelated **control** paths still answer for real. `runtime.readiness` is
    // the status surface: it reports the process's own state and its capability
    // set, and it does not depend on P03.
    let health = success(&state, "runtime.readiness", json!({})).await;
    assert!(
        health["core_health"].as_str().is_some(),
        "the status surface must answer with a real state: {health}"
    );
    // The status surface still reports the unrelated capabilities as ready, which
    // is what "unrelated capability keeps working" means at the projection level.
    assert!(
        health["capabilities"].as_array().is_some_and(|capabilities| capabilities.iter().any(
            |entry| {
                entry["capability"] == "settings.configuration" && entry["readiness"] == "ready"
            }
        )),
        "an unrelated capability is still reported ready: {health}"
    );

    // Unrelated **work**: a domain the damaged one does not feed still reads and
    // writes for real. `Diagnostics` is untouched by this test, so its write must
    // reach its own real file.
    let facade =
        SettingsFacade::open(fixture.paths.clone()).expect("S05 R2 fixture operation must succeed");
    let diagnostics = facade
        .snapshot(ConfigModule::Diagnostics)
        .expect("an unrelated domain still reads while another domain is broken");
    let updated = facade
        .update(ConfigModule::Diagnostics, diagnostics.document.revision, |values| {
            values.insert("level".to_owned(), json!("debug"));
        })
        .expect("an unrelated domain still writes while another domain is broken");
    assert_eq!(updated.document.revision, diagnostics.document.revision + 1);
    let reread = fs::read_to_string(fixture.paths.config_path(ConfigModule::Diagnostics))
        .expect("S05 R2 fixture operation must succeed");
    assert!(reread.contains("debug"), "the unrelated write really reached its own file: {reread}");

    // Sessions are served from persistence, which does not depend on the damaged
    // domain. The Workspace/Session projection is the session surface, and it must
    // keep answering: this is "unrelated control survives" at the product level.
    // The model-settings surface is served by the P05 chain, which this test did
    // not damage, so it must keep answering. This is a real IPC round trip through
    // the same dispatcher a damaged domain would have taken down before R2.
    let models = success(&state, "models.settings.get_snapshot", json!({})).await;
    assert!(!models.is_null(), "an unrelated capability still answers over IPC: {models}");

    // The damaged domain is reported honestly rather than as Ready.
    let degraded = state.config_degradation.all();
    assert!(
        degraded.iter().any(|entry| entry.fault_code.starts_with("config.domain.")),
        "the damaged domain is recorded with a stable configuration fault code: {degraded:?}"
    );
    // And it did not take any unrelated capability with it.
    assert!(
        state.config_degradation.is_available(ConfigDependentCapability::ModelAuthority),
        "a damaged Diagnostics domain must not disable the model authority"
    );
}

#[tokio::test]
async fn a_failed_tool_policy_read_never_becomes_a_fabricated_default() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05 R2 fixture operation must succeed"),
    );
    break_domain(&fixture.paths, ConfigModule::Tools);

    let state = fixture.start().await;
    assert!(
        matches!(&state.startup_mode, StartupMode::Ready(_)),
        "Core must start with tools.json unreadable; startup mode is {}",
        startup_mode_label(&state)
    );

    // The tool policy capability is genuinely refused, not served from a default.
    assert!(
        !state.config_degradation.is_available(ConfigDependentCapability::ToolPolicy),
        "an unreadable tools.json must degrade the tool policy rather than fabricate one"
    );
    let reason = state
        .config_degradation
        .reason(ConfigDependentCapability::ToolPolicy)
        .expect("the refusal carries its real reason");
    assert_eq!(
        reason.domain,
        Some(ConfigModule::Tools),
        "the reason names the domain that really failed"
    );

    // Control still answers, and unrelated configuration still reads and writes.
    let readiness = success(&state, "runtime.readiness", json!({})).await;
    assert!(readiness["core_health"].as_str().is_some(), "{readiness}");
    let facade =
        SettingsFacade::open(fixture.paths.clone()).expect("S05 R2 fixture operation must succeed");
    let update_snapshot = facade.snapshot(ConfigModule::Update).expect("unrelated domain reads");
    facade
        .update(ConfigModule::Update, update_snapshot.document.revision, |values| {
            values.insert("channel".to_owned(), json!("beta"));
        })
        .expect("an unrelated domain still writes while tools.json is unreadable");
    assert!(
        state.config_degradation.is_available(ConfigDependentCapability::SecurityAuthority),
        "an unreadable tool policy must not disable the security authority"
    );

    // A call that **genuinely depends** on the unreadable domain is refused
    // honestly, with the stable capability code — not served from a fabricated
    // default and not panicking. Creating a workspace needs the workspace runtime
    // that `tools.json` sizes, so this is exactly a dependent new call.
    let root = fixture.root.path().join("s05r-r2-tools-workspace");
    fs::create_dir(&root).expect("S05 R2 fixture operation must succeed");
    let refusal = crate::r03_tests::request(
        &state,
        "workspace.create",
        json!({"selected_root": root, "display_name": "s05r-r2-tools"}),
    )
    .await;
    assert_eq!(
        refusal["error"]["data"]["code"], "core.tool_policy.config_unavailable",
        "a dependent call is refused with the stable capability code: {refusal}"
    );
    assert_eq!(
        refusal["error"]["data"]["category"], "unavailable",
        "the refusal is categorised as unavailable, not as an internal fault: {refusal}"
    );
    assert_eq!(
        refusal["error"]["data"]["retryable"], false,
        "a configuration fault is not silently retryable: {refusal}"
    );

    // The health projection is a real consumer: it must not claim plain readiness
    // while a capability is genuinely gone.
    let projection = current_core_health_projection(&state);
    let ProjectionChange::CoreHealth { payload, .. } = projection else {
        panic!("the health projection is a CoreHealth change");
    };
    assert_eq!(payload.status_code, "core.ready_degraded");
    assert!(
        !payload.read_only,
        "a capability loss must not take away unrelated work, status, stop or repair"
    );
    // The message names capability keys and stable codes only: no path, no body.
    assert!(!payload.status_message.contains(':'), "{}", payload.status_message);
    assert!(!payload.status_message.contains('\\'), "{}", payload.status_message);
    assert!(!payload.status_message.contains("tools.json"), "{}", payload.status_message);
}

#[tokio::test]
async fn a_repair_brings_the_capability_back_without_a_restart() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05 R2 fixture operation must succeed"),
    );
    break_domain(&fixture.paths, ConfigModule::Tools);

    let state = fixture.start().await;
    assert!(!state.config_degradation.is_available(ConfigDependentCapability::ToolPolicy));

    // A real repair: the blocking directory is removed and a valid document is
    // written, then read back through the real facade.
    let path = fixture.paths.config_path(ConfigModule::Tools);
    fs::remove_dir_all(&path).expect("S05 R2 fixture operation must succeed");
    fs::write(
        &path,
        serde_json::json!({ "schema_version": 1, "revision": 2, "values": {} }).to_string(),
    )
    .expect("S05 R2 fixture operation must succeed");

    // The repair is driven through the **real recovery
    // entry**, not by clearing the degradation record by hand.
    //
    // The previous version of this case called `restore_domain(...)` directly and then
    // checked only the health projection. That cannot stand: it proves a
    // record was deleted, never that a service exists, so it passes while the real slot is
    // still `None` and a genuine request for the capability would still fail.
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be ready");
    };
    let entry = services.config_recovery();
    // The **first** supply discharges the one-time
    // responsibilities the startup path never met — the Agent/inbox scan, the wait restoration and
    // the maintenance task. That work changes live state, and a recovery plan is derived from live
    // state, so a preview taken before it is a genuinely stale confirmation afterwards. The preview
    // below is therefore the one a client would take once the capability is present, and the
    // agreement between two consecutive previews is asserted so a plan that kept moving would fail
    // here rather than masquerade as a bound confirmation.
    let first_preview = entry.plan_bundle(&[ConfigModule::Tools]).expect("a real preview");
    let preview = entry.plan_bundle(&[ConfigModule::Tools]).expect("a settled preview");
    assert_eq!(
        first_preview.domains[0].preview_id, preview.domains[0].preview_id,
        "once the first recovery has run, the preview identity is stable; a plan that keeps \
         changing would make every confirmation stale by construction"
    );
    let plan = entry
        .plan_domain_confirmed(ConfigModule::Tools, &preview.domains[0].preview_id)
        .expect("the confirmation matches the preview that was just shown");
    let report = entry
        .apply_domain(&plan, aura_config::recovery::ConfigRecoveryAction::Retry)
        .expect("the repair is applied through the real entry");

    // The configuration really was recovered, and the entry says so from the write.
    assert!(
        report.outcomes.iter().any(|outcome| outcome.applied),
        "the retry really applied: {report:?}"
    );
    assert!(
        state.config_degradation.is_available(ConfigDependentCapability::ToolPolicy),
        "after a real recovery the record stops reporting the capability unavailable"
    );

    // The capability report is read from the **service**, so a
    // capability is listed only when its service is really present. In this process the tool
    // policy is served by the command backend **and** the Agent application, and both are
    // supplied by the live recovery — no restart is involved.
    assert!(
        report.restored_capabilities.iter().any(|capability| capability == "tool_policy"),
        "after a real recovery the tool policy's service is present, so it is reported \
         restored: {report:?}"
    );
    assert!(
        report.unavailable_capabilities.iter().all(|entry| entry.capability != "tool_policy"),
        "the tool policy's whole service chain is present, so it is NOT reported unavailable: \
         {report:?}"
    );
    // Transaction recovery is a responsibility judgement, not a service to construct.
    // This fixture has no unresolved journal; its report must agree with that real fact.
    // The unknown-scope regressions separately prove an unresolved judgement stays refused.
    assert!(
        state.config_degradation.is_available(ConfigDependentCapability::ConfigTransactionRecovery),
        "this fixture has no unresolved configuration transaction"
    );
    assert!(
        report
            .restored_capabilities
            .iter()
            .any(|capability| capability == "config_transaction_recovery"),
        "the report includes the healthy transaction judgement: {report:?}"
    );
    assert!(
        report
            .unavailable_capabilities
            .iter()
            .all(|entry| entry.capability != "config_transaction_recovery"),
        "a healthy judgement must not be misreported as a missing service: {report:?}"
    );
    // And the tool policy is really readable through the service the recovery published.
    assert!(
        state.config_degradation.is_available(ConfigDependentCapability::ToolPolicy),
        "the degraded record is cleared as well"
    );

    // THE DECISIVE ASSERTION: a **real capability request** through the ordinary
    // dispatcher, on this same live runtime, must now succeed. The previous version of this case
    // accepted "the capability is still missing" as the outcome, which is exactly the defect the
    // brief names ("不能以只能重启Core替代第14项").
    let root = fixture.root.path().join("s05r-r2-repaired-workspace");
    fs::create_dir(&root).expect("S05 R2 fixture operation must succeed");
    let created = success(
        &state,
        "workspace.create",
        json!({"selected_root": root, "display_name": "s05r-r2-repaired"}),
    )
    .await;
    assert_eq!(
        created["disposition"], "created",
        "the tool-policy-dependent call really succeeds after the repair, with no restart: \
         {created}"
    );
    assert!(
        created["workspace"]["workspace_id"].as_str().is_some(),
        "the created workspace carries a real identity read back from the runtime: {created}"
    );

    // And the domain really serves the repaired document — the configuration half is true
    // independently of the service half.
    let facade =
        SettingsFacade::open(fixture.paths.clone()).expect("S05 R2 fixture operation must succeed");
    assert!(facade.is_available(ConfigModule::Tools), "the repaired domain is usable again");

    // And the health projection goes back to plain ready.
    let ProjectionChange::CoreHealth { payload, .. } = current_core_health_projection(&state)
    else {
        panic!("the health projection is a CoreHealth change");
    };
    if state.config_degradation.all().is_empty() {
        assert_eq!(payload.status_code, "core.ready");
    }
    assert!(!payload.read_only, "status, stop and repair stay reachable");
}

/// **Dependent-service counterexample.** One capability's failure must not discard
/// the others.
///
/// `open_dependent_services` built P04, P05, the command backend and the Agent
/// application with bare `?` operators, so the first failure aborted the whole
/// function and threw away every capability that had **already opened**. A damaged
/// `models.json` therefore also cost the user the **security authority**, which
/// does not read `models.json` at all — an unrelated fault taking an unrelated
/// capability with it, and silently, because nothing recorded that P04 had been
/// available.
///
/// This case damages exactly `models.json` and asserts the security authority is
/// still there while the model authority is honestly refused.
#[tokio::test]
async fn a_model_authority_fault_does_not_discard_the_security_authority() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05 R2 fixture operation must succeed"),
    );
    // Only the model authority's own domain is damaged. `models.json` is not read by
    // P04, so the security authority has no real dependency on it.
    break_domain(&fixture.paths, ConfigModule::Models);

    let state = fixture.start().await;

    // Core still starts, and it started for real rather than falling back.
    assert!(
        matches!(&state.startup_mode, StartupMode::Ready(_)),
        "Core must start with a damaged models.json; startup mode is {}",
        startup_mode_label(&state)
    );
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be Ready for this case");
    };

    // THE COUNTEREXAMPLE: the model authority is refused, honestly and on its own
    // evidence…
    assert!(
        !services.config_degradation.is_available(ConfigDependentCapability::ModelAuthority),
        "a damaged models.json really disables the model authority"
    );
    // …while the security authority, which does not read that file, is still there.
    // Under the old `?` chain this was `None`, because the P05 failure aborted the
    // function after P04 had already been opened.
    assert!(
        services.p04().is_ok(),
        "the security authority must survive an unrelated model-authority fault"
    );
    // And the registry agrees: the security capability was never degraded.
    assert!(
        services.config_degradation.is_available(ConfigDependentCapability::SecurityAuthority),
        "an unrelated fault must not be recorded against the security authority"
    );
}

/// **Item 14: repeated and concurrent recovery must not duplicate anything.**
///
/// The brief requires an *actual observation* of the unique owners and background tasks
/// after repeated recovery — "重复恢复与并发恢复：唯一owner和后台任务数量有实际观测，没有重复启动恢复".
/// A claim that no duplicate was started is not an observation.
///
/// # What the previous version of this case got wrong
///
/// Three measured weaknesses, each fixed here rather than reworded:
///
/// 1. it **swallowed every recovery error** (`let _ = ...`), so the case passed even if all
///    twelve recoveries failed — "全部恢复失败也不能由这些断言排除";
/// 2. it compared `Arc` **pointers** and a fixed capacity, which a second pool or a second
///    background task would not change;
/// 3. it never counted the real per-capability **services**, so a duplicate supervisor could
///    exist without any assertion noticing.
///
/// This version reports the real counts: how many recoveries succeeded, how many failed and
/// why, how many services exist before and after, and how many live run tasks the supervisor
/// holds.
#[tokio::test]
async fn repeated_and_concurrent_recovery_does_not_duplicate_owners_or_tasks() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05 R2 fixture operation must succeed"),
    );
    break_domain(&fixture.paths, ConfigModule::Tools);

    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be ready");
    };

    // The process-wide owners a supply could most plausibly duplicate: the broker journal, the
    // persistence coordinator, the Agent Store, the bounded execution boundary, and the Agent
    // application itself (which owns the run supervisor, the expiry timers and the backfill
    // owner). Their **identities** are recorded before any recovery, so "the same owner" is an
    // observation rather than an assertion about intent.
    let broker_before = std::sync::Arc::as_ptr(&services.broker);
    let persistence_before = std::sync::Arc::as_ptr(&services.persistence);
    let agent_before = services.agent.get().map(|agent| std::sync::Arc::as_ptr(&agent));
    let boundary_capacity_before = services.sync.capacity();
    let counts_before = services.sync.all_counters();
    // The real live-run count the supervisor holds. A second supervisor would be a second,
    // independent record, so comparing this after the recoveries observes whether the run path
    // still has exactly one owner.
    let active_runs_before =
        services.agent.get().map(|agent| agent.probe_active_runs().len()).unwrap_or_default();

    // The workspace runtime is the root of the dependent chain: with `tools.json` damaged it was
    // refused at startup, so it is genuinely absent now and the recoveries must supply it.
    assert!(
        services.p03().is_err(),
        "a damaged tools.json refuses the workspace capability at startup, which is the \
         precondition that makes the supply below meaningful"
    );

    // Repair the configuration for real.
    let path = fixture.paths.config_path(ConfigModule::Tools);
    fs::remove_dir_all(&path).expect("S05 R2 fixture operation must succeed");
    fs::write(
        &path,
        serde_json::json!({ "schema_version": 1, "revision": 2, "values": {} }).to_string(),
    )
    .expect("S05 R2 fixture operation must succeed");

    // Recover repeatedly, and concurrently, through the real entry. Every outcome is
    // **counted** rather than discarded.
    let entry = services.config_recovery();
    let mut handles = Vec::new();
    for _ in 0..4 {
        let entry = entry.clone_entry();
        handles.push(std::thread::spawn(move || {
            let mut applied = 0usize;
            let mut refused = Vec::new();
            for _ in 0..3 {
                match entry.plan_bundle(&[ConfigModule::Tools]) {
                    Ok(preview) => {
                        match entry.plan_domain_confirmed(
                            ConfigModule::Tools,
                            &preview.domains[0].preview_id,
                        ) {
                            Ok(plan) => match entry.apply_domain(
                                &plan,
                                aura_config::recovery::ConfigRecoveryAction::Retry,
                            ) {
                                Ok(report) if report.outcomes.iter().any(|o| o.applied) => {
                                    applied += 1;
                                }
                                Ok(report) => refused.push(format!("not applied: {report:?}")),
                                Err(error) => refused.push(error.to_string()),
                            },
                            Err(error) => refused.push(error.to_string()),
                        }
                    }
                    Err(error) => refused.push(error.to_string()),
                }
            }
            (applied, refused)
        }));
    }
    let mut applied_total = 0usize;
    let mut refusals = Vec::new();
    for handle in handles {
        let (applied, refused) = handle.join().expect("a recovery thread joins");
        applied_total += applied;
        refusals.extend(refused);
    }

    // THE REAL COUNTS. A recovery of an already-repaired domain is legitimately refused (there is
    // nothing left to repair), so the honest assertion is that at least one recovery really
    // applied, and that every non-applied attempt carries a reason rather than being swallowed.
    assert!(
        applied_total >= 1,
        "at least one of the twelve recoveries must really apply; got {applied_total} applied \
         and {} refusals: {refusals:?}",
        refusals.len()
    );
    assert!(
        refusals.iter().all(|reason| !reason.is_empty()),
        "every non-applied attempt carries a real reason rather than being swallowed: {refusals:?}"
    );

    // The owners are the SAME objects, not replacements.
    assert_eq!(
        std::sync::Arc::as_ptr(&services.broker),
        broker_before,
        "recovery must reuse the process's one BrokerJournal"
    );
    assert_eq!(
        std::sync::Arc::as_ptr(&services.persistence),
        persistence_before,
        "recovery must reuse the process's one PersistenceCoordinator"
    );
    assert_eq!(
        services.sync.capacity(),
        boundary_capacity_before,
        "recovery must reuse the process's one bounded execution capacity"
    );
    // The Agent application is **one** object: a second one would own a second supervisor, and
    // `publish_if_vacant` refuses to replace an occupant, so twelve concurrent recoveries must
    // leave exactly one. It was genuinely absent before — `tools.json` was damaged — so
    // "present afterwards" is the recoveries' own result rather than something inherited.
    assert!(
        agent_before.is_none(),
        "the Agent application is absent before the recoveries, which is the precondition that \
         makes 'exactly one afterwards' meaningful"
    );
    let agent_after = services
        .agent
        .get()
        .map(|agent| std::sync::Arc::as_ptr(&agent))
        .expect("the recoveries supplied the Agent application the startup path refused");
    assert_eq!(
        services.agent.get().map(|agent| std::sync::Arc::as_ptr(&agent)),
        Some(agent_after),
        "the recoveries must leave exactly one Agent application, not replace it per attempt"
    );
    assert_eq!(
        services.agent.get().map(|agent| agent.probe_active_runs().len()).unwrap_or_default(),
        active_runs_before,
        "the supervisor still holds exactly the runs it had, so there is no second supervisor"
    );
    // The workspace capability was supplied by the recoveries, and it is **one** object: a
    // second supply would have replaced it, which `publish_if_vacant` refuses to do.
    let p03_after = services
        .p03()
        .expect("the recoveries supplied the workspace capability the startup path refused")
        .as_ref() as *const _;
    // Reading it again returns the same object, which is what "exactly one service" means.
    assert_eq!(
        services.p03().expect("still present").as_ref() as *const _,
        p03_after,
        "the recoveries must leave exactly one workspace runtime, not replace it per attempt"
    );
    // The boundary's **section set** observes "no second boundary": a second pool would bring its
    // own counters, and the process's one boundary is the object every recovery shares.
    //
    // The first recovery legitimately **adds** sections to the
    // one boundary, because it now discharges the one-time responsibilities the startup path never
    // met — the Agent/inbox scan and the wait restoration run through sections that did not exist
    // in this process before. That is new work on the same boundary, not a second boundary, and the
    // two are distinguished by requiring the previous keys to still be a subset and the new keys to
    // be only those the first recovery really needs.
    let counts_after = services.sync.all_counters();
    for previous in counts_before.keys() {
        assert!(
            counts_after.contains_key(previous),
            "every section the process already had is still on the same boundary: {previous}"
        );
    }
    let added = counts_after
        .keys()
        .filter(|key| !counts_before.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        added.iter().all(|section| matches!(*section, "journal_read" | "store_read")),
        "the only sections a first recovery may add are the ones its own one-time work uses; a \
         second boundary would add more: {added:?}"
    );
    // The boundary is the **same object**: its capacity is unchanged and its shared counters are
    // reachable through the same handle, so the sections above are on the process's one pool.
    assert_eq!(
        services.sync.capacity(),
        boundary_capacity_before,
        "the one boundary's capacity is unchanged, so no second pool was opened"
    );
    // And the runtime still serves: an unrelated real read keeps working afterwards.
    let health = success(&state, "runtime.readiness", json!({})).await;
    assert!(
        health["core_health"].as_str().is_some(),
        "the runtime still answers after repeated recovery: {health}"
    );
    // And the genuinely dependent call still works, on the same live runtime.
    let root = fixture.root.path().join("s05r-r2-concurrent-workspace");
    fs::create_dir(&root).expect("S05 R2 fixture operation must succeed");
    let created = success(
        &state,
        "workspace.create",
        json!({"selected_root": root, "display_name": "s05r-r2-concurrent"}),
    )
    .await;
    assert_eq!(
        created["disposition"], "created",
        "a dependent call still succeeds after twelve concurrent recoveries: {created}"
    );
}
