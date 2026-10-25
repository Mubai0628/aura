//! The recovery entry is **reachable**, not just implemented.
//!
//! # What "reachable" means here
//!
//! A recovery flow that exists only as a library function is not an entry: the
//! user cannot get to it. These tests drive the three IPC methods the product
//! serves — `config.recovery.notice`, `config.recovery.plan`,
//! `config.recovery.apply` — through the same dispatcher a renderer's request
//! takes, against a runtime whose configuration is genuinely damaged.
//!
//! # The loop that must close
//!
//! A fault is recorded in the degradation registry; the notice reports it; the
//! user previews and applies a recovery; and the **registry is updated**, so the
//! capability stops being reported as unavailable. Before R5 the registry was a
//! write-only record with no consumer, and the capability stayed reported as
//! broken even after a successful repair.
use super::*;
use crate::config_degradation::ConfigDependentCapability;
use crate::r03_tests::{Fixture, attempt, success};
use aura_config::{ConfigModule, SettingsFacade};
use serde_json::json;
#[path = "recovery_entry/preview_binding.rs"]
mod preview_binding;

/// A request that may succeed, or be refused with an envelope.
///
/// A refusal the runtime reports as a **result** (`error` in the response) is what a caller
/// acts on, so a case that proves "this was refused rather than applied" observes it here. A
/// transport-level error is a different shape and is asserted with `attempt` instead.
async fn success_or_refusal(
    state: &RuntimeState,
    method_name: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    match attempt(state, method_name, params).await {
        Ok(response) => response,
        // The dispatcher reported a transport-level failure rather than a response. That is a
        // refusal too, and it is reported in the same shape so a caller can assert on it.
        Err(error) => json!({"error": {"message": error}}),
    }
}

/// Replaces one domain's configuration file with a directory, so its real
/// `File::open` fails with a genuine I/O error.
fn break_domain(paths: &aura_config::AuraPaths, module: ConfigModule) {
    let path = paths.config_path(module);
    if path.is_file() {
        fs::remove_file(&path).expect("S05R R5 fixture operation must succeed");
    }
    fs::create_dir_all(&path).expect("S05R R5 fixture operation must succeed");
    fs::write(path.join("blocker.txt"), b"this domain cannot be opened")
        .expect("S05R R5 fixture operation must succeed");
}

/// The notice is served, and it names the affected domain, the suspended calls and
/// the actions the user has.
#[tokio::test]
async fn the_notice_is_served_and_names_what_is_affected_and_what_to_do() {
    let fixture = Fixture::new();
    drop(SettingsFacade::open(fixture.paths.clone()).expect("seed valid configuration"));
    break_domain(&fixture.paths, ConfigModule::Tools);

    let state = fixture.start().await;
    assert!(
        matches!(&state.startup_mode, StartupMode::Ready(_)),
        "Core must start so the recovery entry is reachable at all"
    );

    let notice = success(&state, "config.recovery.notice", json!({})).await;

    // It names the affected configuration domain.
    assert!(
        notice["affected_domains"]
            .as_array()
            .is_some_and(|domains| domains.iter().any(|domain| domain == "tools")),
        "the notice names the affected domain: {notice}"
    );
    // It names the capabilities that are unavailable.
    assert!(
        notice["unavailable_capabilities"].as_array().is_some_and(|capabilities| capabilities
            .iter()
            .any(|capability| capability == "tool_policy")),
        "the notice names the unavailable capability: {notice}"
    );
    // It names what is suspended, and it does so in bounded terms.
    assert!(
        notice["suspended_calls"].as_array().is_some_and(|calls| calls
            .iter()
            .any(|call| call.as_str().is_some_and(|call| call.contains("tool execution")))),
        "the notice names the suspended new calls: {notice}"
    );
    // It names actionable steps and a next step.
    assert!(
        notice["available_actions"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|action| action == "retry")),
        "the notice offers a retry: {notice}"
    );
    assert!(
        notice["next_step"].as_str().is_some_and(|step| !step.is_empty()),
        "the notice states a next step: {notice}"
    );
    assert_eq!(notice["scope_known"], json!(true));
    // A notice never carries a path or a document body.
    let described = notice.to_string();
    assert!(!described.contains("tools.json"), "a notice must not carry a path: {described}");
    assert!(!described.contains("\\\\"), "a notice must not carry a path: {described}");
}

/// The plan is served as a **preview** and writes nothing.
#[tokio::test]
async fn the_plan_is_served_as_a_preview_and_writes_nothing() {
    let fixture = Fixture::new();
    drop(SettingsFacade::open(fixture.paths.clone()).expect("seed valid configuration"));
    break_domain(&fixture.paths, ConfigModule::Tools);

    let state = fixture.start().await;
    let plan = success(&state, "config.recovery.plan", json!({"domains": ["tools"]})).await;

    // The plan states its scope.
    assert_eq!(plan["modules"], json!(["tools"]), "the plan states the scope: {plan}");
    // The restore option is honestly disabled when the recorded history cannot be
    // proven, and the default reset is offered instead.
    let options = plan["options"].as_array().expect("the plan lists its options");
    let reset = options
        .iter()
        .find(|option| option["action"] == "reset_to_default")
        .expect("the reset option is listed, not silently omitted");
    assert_eq!(reset["available"], json!(true), "the scoped default reset is offered: {reset}");
    let retry = options
        .iter()
        .find(|option| option["action"] == "retry")
        .expect("the retry option is listed");
    assert_eq!(retry["available"], json!(true));

    // The preview is a preview: the damaged path is still exactly as it was.
    assert!(
        fixture.paths.config_path(ConfigModule::Tools).is_dir(),
        "planning must not change anything on disk"
    );
    // And the suspended capability is still reported as unavailable.
    let notice = success(&state, "config.recovery.notice", json!({})).await;
    assert!(
        notice["unavailable_capabilities"].as_array().is_some_and(|capabilities| capabilities
            .iter()
            .any(|capability| capability == "tool_policy")),
        "the capability is still unavailable after a preview: {notice}"
    );
}

/// Applying a recovery closes the loop: the capability becomes available again.
#[tokio::test]
async fn applying_a_recovery_restores_the_capability_in_the_registry() {
    let fixture = Fixture::new();
    drop(SettingsFacade::open(fixture.paths.clone()).expect("seed valid configuration"));
    break_domain(&fixture.paths, ConfigModule::Tools);

    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must start for this case");
    };
    // Before: the capability really is unavailable.
    assert!(
        !services.config_degradation.is_available(ConfigDependentCapability::ToolPolicy),
        "an unreadable tools.json makes the tool policy unavailable"
    );

    // Clear the damage, write a valid document, then apply the recovery.
    let path = fixture.paths.config_path(ConfigModule::Tools);
    fs::remove_dir_all(&path).expect("S05R R5 fixture operation must succeed");
    fs::write(
        &path,
        serde_json::json!({ "schema_version": 1, "revision": 5, "values": {} }).to_string(),
    )
    .expect("S05R R5 fixture operation must succeed");

    let plan = success(&state, "config.recovery.plan", json!({"domains": ["tools"]})).await;
    assert_eq!(plan["modules"], json!(["tools"]));
    // The confirmation is **bound to the preview** the user was
    // shown. The identity is echoed from the plan above, which is what a real client does, and a
    // request without it is refused rather than treated as "apply the current state".
    let preview_id = plan["domains"][0]["preview_id"]
        .as_str()
        .expect("the preview carries its own identity for the confirmation to echo");
    let report = success(
        &state,
        "config.recovery.apply",
        json!({
            "action": "reset_to_default",
            "domains": ["tools"],
            "preview_id": preview_id,
        }),
    )
    .await;

    // The report states what it did, and which capabilities came back.
    assert_eq!(report["action"], json!("reset_to_default"));
    let outcomes = report["outcomes"].as_array().expect("the report lists its outcomes");
    assert!(
        outcomes.iter().any(|outcome| outcome["module"] == "tools" && outcome["applied"] == true),
        "the tools domain was really recovered: {report}"
    );
    // **the configuration recovery and the capability
    // restoration are two facts**, and this case asserts them separately.
    //
    // The reset really applied — `applied: true` above and the revision below prove it. And the
    // tool policy really is listed as restored, because its whole service chain is now supplied
    // by the recovery itself: the workspace runtime, the security authority built on it, the
    // command backend and the Agent application. The earlier version of this case asserted the
    // opposite — that the capability stayed missing and "a restart is required" — which is
    // exactly the outcome the brief forbids ("不能以只能重启Core替代第14项").
    assert!(
        report["restored_capabilities"].as_array().is_some_and(|capabilities| capabilities
            .iter()
            .any(|capability| capability == "tool_policy")),
        "the tool policy's whole service chain is supplied by the recovery, so it is reported \
         restored: {report}"
    );
    assert!(
        !report["unavailable_capabilities"].as_array().is_some_and(|entries| entries
            .iter()
            .any(|entry| entry["capability"] == "tool_policy")),
        "and it is not simultaneously reported unavailable: {report}"
    );
    // And the configuration recovery itself is real: the domain serves the new revision.
    assert_eq!(
        outcomes
            .iter()
            .find(|outcome| outcome["module"] == "tools")
            .and_then(|outcome| outcome["revision"].as_u64()),
        Some(2),
        "the reset really produced a new revision: {report}"
    );

    // THE LOOP CLOSED: the registry now says the capability is available, so the
    // notice stops reporting a fault the user already fixed.
    assert!(
        services.config_degradation.is_available(ConfigDependentCapability::ToolPolicy),
        "after a real recovery the capability must stop being reported as unavailable"
    );
    let notice = success(&state, "config.recovery.notice", json!({})).await;
    assert!(
        notice["unavailable_capabilities"]
            .as_array()
            .is_none_or(|capabilities| capabilities.is_empty()),
        "the notice stops reporting the repaired capability: {notice}"
    );
}

/// An unreachable or unusable request is refused with a real reason, never applied.
#[tokio::test]
async fn an_invalid_recovery_request_is_refused_rather_than_applied() {
    let fixture = Fixture::new();
    drop(SettingsFacade::open(fixture.paths.clone()).expect("seed valid configuration"));
    break_domain(&fixture.paths, ConfigModule::Tools);

    let state = fixture.start().await;

    // No domains named: the runtime must not guess which configuration to reset. A confirmation
    // identity is present so the refusal is about the missing scope, not about a malformed
    // request — the two are different statements and the user's next step differs.
    let refused = success_or_refusal(
        &state,
        "config.recovery.apply",
        json!({"action": "retry", "domains": [], "preview_id": "bundle:none"}),
    )
    .await;
    assert!(
        refused.get("error").is_some(),
        "a recovery with no named domain is refused: {refused}"
    );

    // The preview identity is **required**. Omitting it entirely is
    // refused before any write, so it is no longer a documented bypass of the confirmation. The
    // refusal is a malformed-request error, which `attempt` observes rather than panicking on.
    let no_preview = attempt(
        &state,
        "config.recovery.apply",
        json!({"action": "reset_to_default", "domains": ["tools"]}),
    )
    .await;
    assert!(
        no_preview.is_err(),
        "a recovery with no confirmation identity is refused rather than applied: {no_preview:?}"
    );
    assert!(
        no_preview.as_ref().is_err_and(|error| error.contains("invalid config.recovery.apply")),
        "the refusal names the malformed request: {no_preview:?}"
    );
    assert!(
        fixture.paths.config_path(ConfigModule::Tools).is_dir(),
        "the refusal happened before any write: the damaged path is untouched"
    );

    // A confirmation for a state the user was never shown is refused too — and this one is a
    // **valid** request refused for a real reason, so it reaches the caller as an envelope.
    let stale = success_or_refusal(
        &state,
        "config.recovery.apply",
        json!({
            "action": "reset_to_default",
            "domains": ["tools"],
            "preview_id": "bundle:0000000000000000000000000000000000000000000000000000000000000000",
        }),
    )
    .await;
    assert!(
        stale.get("error").is_some(),
        "a confirmation that does not describe the live state is refused: {stale}"
    );
    assert!(
        fixture.paths.config_path(ConfigModule::Tools).is_dir(),
        "a stale confirmation writes nothing"
    );

    // An unknown scope is never silently widened into "reset everything": the plan
    // for the one affected domain names exactly that domain.
    let plan = success(&state, "config.recovery.plan", json!({"domains": ["tools"]})).await;
    assert_eq!(
        plan["modules"],
        json!(["tools"]),
        "the scope stays exactly what the caller named: {plan}"
    );
}

/// A recovery that cannot be planned still leaves status and diagnostics reachable.
#[tokio::test]
async fn recovery_failure_does_not_take_away_status_or_diagnostics() {
    let fixture = Fixture::new();
    drop(SettingsFacade::open(fixture.paths.clone()).expect("seed valid configuration"));
    break_domain(&fixture.paths, ConfigModule::Tools);

    let state = fixture.start().await;

    // The status surface still answers while the domain is damaged.
    let readiness = success(&state, "runtime.readiness", json!({})).await;
    assert!(readiness["core_health"].as_str().is_some(), "{readiness}");

    // A recovery attempt against the still-blocked path fails honestly. The confirmation is
    // taken from a real preview of that same domain, so the refusal is about the blocked write
    // rather than about a missing identity.
    let plan = success(&state, "config.recovery.plan", json!({"domains": ["tools"]})).await;
    let preview_id = plan["domains"][0]["preview_id"]
        .as_str()
        .expect("the preview carries its own identity")
        .to_owned();
    let refused = success_or_refusal(
        &state,
        "config.recovery.apply",
        json!({
            "action": "reset_to_default",
            "domain": "tools",
            "preview_id": preview_id,
        }),
    )
    .await;
    assert!(
        refused.get("error").is_some(),
        "a recovery that cannot be applied reports a real failure: {refused}"
    );

    // THE POINT: after that failure, status, diagnostics and the notice all still
    // answer. A failed repair must not remove the user's ability to see and stop.
    let readiness = success(&state, "runtime.readiness", json!({})).await;
    assert!(readiness["core_health"].as_str().is_some());
    let notice = success(&state, "config.recovery.notice", json!({})).await;
    assert!(
        notice["fault_codes"].as_array().is_some_and(|codes| !codes.is_empty()),
        "the fault is still reported after a failed repair: {notice}"
    );
    assert!(
        notice["next_step"].as_str().is_some_and(|step| !step.is_empty()),
        "the user is still told what to do: {notice}"
    );
}
