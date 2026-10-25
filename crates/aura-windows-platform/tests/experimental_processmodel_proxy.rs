#![cfg(feature = "experimental-processmodel-proxy")]

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use aura_windows_platform::{
    EXPERIMENTAL_PROCESSMODEL_DOCUMENTATION_URL, ExperimentalProcessModelProxyStatus,
    probe_experimental_processmodel_proxy,
};
use serde_json::{Value, json};

const REPORT_PATH_ENV: &str = "AURA_EXPERIMENT_PROCESSMODEL_PROXY_REPORT";
const TEST_PLAN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/experimental/processmodel-proxy/test-plan.json"
));

#[test]
#[ignore = "explicit non-release experiment; run with cargo xtask experimental-processmodel-proxy"]
fn records_experimental_processmodel_proxy_probe() {
    let output_path = env::var_os(REPORT_PATH_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/experimental/processmodel-proxy/report.json"));
    let test_plan: Value = serde_json::from_str(TEST_PLAN)
        .unwrap_or_else(|error| panic!("experimental test plan must be valid JSON: {error}"));
    let probe = probe_experimental_processmodel_proxy();
    let generated_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| panic!("system clock is earlier than the Unix epoch: {error}"))
        .as_millis();

    let report = json!({
        "schema_version": 1,
        "experiment_id": "experimental-create-process-in-sandbox-proxy",
        "generated_at_unix_ms": generated_at_unix_ms,
        "status": probe.status.as_str(),
        "reason_code": probe.reason_code,
        "summary": probe.summary,
        "release_guarantee": probe.release_security_guarantee,
        "security_boundary_claim": false,
        "experiment_execution_in_check_all": false,
        "experiment_result_in_p04_report": false,
        "official_documentation": EXPERIMENTAL_PROCESSMODEL_DOCUMENTATION_URL,
        "host_probe": {
            "processmodel_dll_loaded": probe.processmodel_dll_loaded,
            "experimental_create_process_export_available": probe.create_process_export_available,
            "experimental_create_process_as_user_export_available": probe.create_process_as_user_export_available,
            "loader_error_code": probe.loader_error_code,
            "aura_audited_flatbuffer_binding_available": probe.aura_audited_flatbuffer_binding_available,
            "api_invoked": probe.api_invoked,
            "bypass_tests_executed": probe.bypass_tests_executed,
            "cleanup_required": probe.cleanup_required,
            "cleanup_verified": probe.cleanup_verified
        },
        "test_plan": test_plan
    });

    write_report(&output_path, &report);
    println!(
        "experimental ProcessModel proxy report: {} ({})",
        output_path.display(),
        probe.status.as_str()
    );

    if probe.status == ExperimentalProcessModelProxyStatus::Failed {
        panic!("experimental ProcessModel proxy probe reported a real cleanup failure");
    }
}

fn write_report(output_path: &Path, report: &Value) {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|error| {
            panic!("failed to create experimental report directory {}: {error}", parent.display())
        });
    }
    let mut bytes = serde_json::to_vec_pretty(report)
        .unwrap_or_else(|error| panic!("failed to serialize experimental report: {error}"));
    bytes.push(b'\n');
    fs::write(output_path, bytes).unwrap_or_else(|error| {
        panic!("failed to write experimental report {}: {error}", output_path.display())
    });
}
