//! Probes that write evidence reports: the experimental ProcessModel proxy and the AppContainer
//! lowbox/loopback check. Both run one ignored test from the shared Rust test build.
use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::{display_relative, required_string, rust_tests::Session};

pub(crate) fn experimental_processmodel_proxy(root: &Path) -> Result<()> {
    let output_path = root.join("target/experimental/processmodel-proxy/report.json");
    if output_path.exists() {
        fs::remove_file(&output_path)
            .with_context(|| format!("failed to remove stale {}", output_path.display()))?;
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut session = Session::start(root)?;
    let passed = session.run_with_env(
        &[
            "-p",
            "aura-windows-platform",
            "--tests",
            "--",
            "experimental_processmodel_proxy::",
            "--ignored",
            "--nocapture",
        ],
        &[("AURA_EXPERIMENT_PROCESSMODEL_PROXY_REPORT", output_path.as_os_str())],
    )?;
    drop(session);

    ensure!(output_path.is_file(), "experimental probe did not produce {}", output_path.display());
    let report: Value = serde_json::from_slice(
        &fs::read(&output_path)
            .with_context(|| format!("failed to read {}", output_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", output_path.display()))?;
    let report_status = required_string(&report, "status")?;
    ensure!(
        matches!(report_status, "passed" | "failed" | "unsupported" | "inconclusive"),
        "experimental report contains unknown status: {report_status}"
    );
    ensure!(
        report_status != "failed",
        "experimental ProcessModel proxy validation found a real bypass or cleanup failure; see {}",
        output_path.display()
    );
    ensure!(passed, "experimental probe harness failed; see {}", output_path.display());

    println!(
        "Experimental ProcessModel proxy probe recorded {report_status}: {}",
        output_path.display()
    );
    Ok(())
}

const APPCONTAINER_EVIDENCE_FILE: &str = "target/p04-report/appcontainer-network-evidence.json";
const APPCONTAINER_EVIDENCE_TEST: &str =
    "tests::compatibility_appcontainer_has_lowbox_token_and_denies_loopback";

pub(crate) fn appcontainer_network_evidence(root: &Path) -> Result<()> {
    let evidence = collect_appcontainer_network_evidence(root)?;
    let evidence_path = root.join(APPCONTAINER_EVIDENCE_FILE);
    let status = required_string(&evidence, "status")?;
    println!(
        "AppContainer lowbox/loopback evidence recorded {status}: {}",
        display_relative(root, &evidence_path)
    );
    ensure!(
        status == "passed",
        "required AppContainer evidence is {status}, not passed; see {}",
        evidence_path.display()
    );
    Ok(())
}

pub(crate) fn collect_appcontainer_network_evidence(root: &Path) -> Result<Value> {
    let evidence_path = root.join(APPCONTAINER_EVIDENCE_FILE);
    if evidence_path.exists() {
        fs::remove_file(&evidence_path)
            .with_context(|| format!("failed to remove stale {}", evidence_path.display()))?;
    }
    if let Some(parent) = evidence_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let arguments = [
        "-p",
        "aura-sandbox",
        "--",
        APPCONTAINER_EVIDENCE_TEST,
        "--ignored",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ];
    let command = format!("cargo xtask test {}", arguments.join(" "));
    let mut session = Session::start(root)?;
    let probe_passed = session.run_with_env(
        &arguments,
        &[("AURA_APPCONTAINER_NETWORK_EVIDENCE_REPORT", evidence_path.as_os_str())],
    )?;
    drop(session);

    if !evidence_path.is_file() {
        let evidence = json!({
            "schema_version": 1,
            "evidence_id": "appcontainer_lowbox_token_and_loopback_denial",
            "status": "failed",
            "gate_status": "failed",
            "reason": "probe_did_not_write_evidence",
            "host_preparation_available": null,
            "required_assertions": {
                "child_token_is_appcontainer_lowbox": null,
                "loopback_connection_denied": null
            },
            "probe_command": command,
            "probe_passed": probe_passed
        });
        fs::write(&evidence_path, serde_json::to_vec_pretty(&evidence)?)
            .with_context(|| format!("failed to write {}", evidence_path.display()))?;
        return Ok(evidence);
    }

    let mut evidence: Value = serde_json::from_slice(
        &fs::read(&evidence_path)
            .with_context(|| format!("failed to read {}", evidence_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", evidence_path.display()))?;
    let reported_status = evidence.get("status").and_then(Value::as_str).unwrap_or("failed");
    let proves_lowbox = evidence
        .pointer("/required_assertions/child_token_is_appcontainer_lowbox")
        .and_then(Value::as_bool)
        == Some(true);
    let proves_loopback_denial = evidence
        .pointer("/required_assertions/loopback_connection_denied")
        .and_then(Value::as_bool)
        == Some(true);
    let host_preparation_available =
        evidence.get("host_preparation_available").and_then(Value::as_bool) == Some(true);
    let valid_pass = probe_passed
        && reported_status == "passed"
        && host_preparation_available
        && proves_lowbox
        && proves_loopback_denial;
    let reported_gate_status =
        evidence.get("gate_status").and_then(Value::as_str).unwrap_or("failed");
    let valid_pending_host_preparation = !probe_passed
        && reported_status == "PendingHostPreparation"
        && reported_gate_status == "Inconclusive"
        && !host_preparation_available
        && !proves_lowbox
        && !proves_loopback_denial;

    let object =
        evidence.as_object_mut().context("AppContainer evidence report must be a JSON object")?;
    object.insert("probe_command".to_owned(), Value::String(command));
    object.insert("probe_passed".to_owned(), json!(probe_passed));
    if valid_pass {
        object.insert("gate_status".to_owned(), Value::String("passed".to_owned()));
    } else if valid_pending_host_preparation {
        object.insert("gate_status".to_owned(), Value::String("Inconclusive".to_owned()));
    } else {
        object.insert("status".to_owned(), Value::String("failed".to_owned()));
        object.insert("gate_status".to_owned(), Value::String("failed".to_owned()));
        object.insert(
            "reason".to_owned(),
            Value::String("probe_result_did_not_prove_both_required_assertions".to_owned()),
        );
    }
    fs::write(&evidence_path, serde_json::to_vec_pretty(&evidence)?)
        .with_context(|| format!("failed to write {}", evidence_path.display()))?;
    Ok(evidence)
}
