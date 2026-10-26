#![forbid(unsafe_code)]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use aura_contract_schema::{BUILTIN_TOOL_DEFINITION_SET_PATH, RUST_GENERATED_PATHS, SCHEMA_PATH};
use serde_json::{Value, json};

mod architecture;
mod contracts;
mod git_test_profile;
mod manifests;
mod process;
mod public;
mod r06;

use process::{run, run_with_env};

/// Cargo builds with
/// six jobs and tests run on six threads. Every cargo command xtask runs carries both.
pub(crate) const BUILD_JOBS: &str = "6";
pub(crate) const TEST_THREADS: &str = "6";

/// The stage gates run only under those limits, so their timings and load stay comparable.
pub(crate) fn ensure_build_limits(gate: &str) -> Result<()> {
    ensure!(
        std::env::var("CARGO_BUILD_JOBS").as_deref() == Ok(BUILD_JOBS)
            && std::env::var("RUST_TEST_THREADS").as_deref() == Ok(TEST_THREADS),
        "{gate} requires CARGO_BUILD_JOBS={BUILD_JOBS} and RUST_TEST_THREADS={TEST_THREADS}"
    );
    Ok(())
}

const TYPESCRIPT_CONTRACT_PATH: &str = "packages/contracts/src/generated/contracts.ts";
const TYPESCRIPT_READINESS_VALIDATOR_PATH: &str =
    "apps/desktop/shared/generated/readiness-validator.ts";
const TYPESCRIPT_ROUTES_PATH: &str = "apps/desktop/shared/generated/routes.ts";

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("check-architecture") => {
            ensure!(args.next().is_none(), "check-architecture received unexpected arguments");
            architecture::check(&workspace_root())
        }
        Some("check-fast") => {
            ensure!(args.next().is_none(), "check-fast received unexpected arguments");
            architecture::check_fast(&workspace_root())
        }
        Some("check-r06-spike") => {
            ensure!(args.next().is_none(), "check-r06-spike received unexpected arguments");
            r06::check(&workspace_root())
        }
        Some("r06-mechanism") => {
            ensure!(args.next().is_none(), "r06-mechanism received unexpected arguments");
            r06::mechanism(&workspace_root())
        }
        Some("check-all") => {
            let option = args.next();
            ensure!(
                option.is_none() || option.as_deref() == Some("--skip-e2e"),
                "check-all accepts only the optional --skip-e2e flag"
            );
            ensure!(args.next().is_none(), "check-all received unexpected arguments");
            check_all(option.is_some())
        }
        Some("test-isolated") => {
            let arguments = args.collect::<Vec<_>>();
            let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
            git_test_profile::run_tests(&workspace_root(), &arguments)
        }
        Some("generate-contracts") => generate_contracts(&workspace_root()),
        Some("contracts") if args.next().as_deref() == Some("--check") => contracts::check(),
        Some("test-permission") => test_permission(&workspace_root()),
        Some("test-sandbox") => test_sandbox(&workspace_root()),
        Some("test-network") => test_network(&workspace_root()),
        Some("test-tool") => {
            let target = args.next().context("test-tool requires a namespace or test filter")?;
            ensure!(args.next().is_none(), "test-tool received unexpected arguments");
            test_tool(&workspace_root(), &target)
        }
        Some("appcontainer-network-evidence") => appcontainer_network_evidence(&workspace_root()),
        Some("experimental-processmodel-proxy") => {
            experimental_processmodel_proxy(&workspace_root())
        }
        Some("p04-report") => p04_report(&workspace_root()),
        Some("p05-report") => p05_report(&workspace_root()),
        Some("validate-manifests") => manifests::validate_manifests(&workspace_root()),
        Some("scan-secrets") => public::scan_secrets(args.next()),
        Some("--help" | "-h" | "help") | None => {
            print_help();
            Ok(())
        }
        Some(command) => bail!("unknown xtask command: {command}"),
    }
}

fn print_help() {
    println!(
        "Aura repository tasks\n\n\
         Usage:\n\
           cargo xtask check-all [--skip-e2e]\n\
           cargo xtask check-architecture\n\
           cargo xtask check-fast\n\
           cargo xtask check-r06-spike\n\
           cargo xtask r06-mechanism\n\
           cargo xtask generate-contracts\n\
           cargo xtask contracts --check\n\
           cargo xtask test-permission\n\
           cargo xtask test-sandbox\n\
           cargo xtask test-network\n\
           cargo xtask test-tool <namespace-or-tool>\n\
           cargo xtask appcontainer-network-evidence\n\
           cargo xtask experimental-processmodel-proxy\n\
           cargo xtask p04-report\n\
           cargo xtask p05-report\n\
           cargo xtask validate-manifests"
    );
}

fn test_tool(root: &Path, target: &str) -> Result<()> {
    ensure!(
        !target.is_empty() && !target.starts_with('-'),
        "test-tool target must be a namespace or Rust test filter"
    );
    match target {
        "conformance" => {
            run(
                root,
                "cargo",
                &[
                    "test",
                    "-p",
                    "aura-tool-domain",
                    "-p",
                    "aura-tool-package",
                    "-p",
                    "aura-tool-fixtures",
                    "--no-fail-fast",
                ],
            )?;
            build_tool_runtime_fixtures(root)?;
            run(root, "cargo", &["test", "-p", "aura-core", "--lib", "package_program::tests"])?;
        }
        // Retired the P07 Draft path; packages are imported, verified and installed
        // through `packages.*` and tested below.
        "package" => {
            build_tool_runtime_fixtures(root)?;
            run(root, "cargo", &["test", "-p", "aura-tool-package"])?;
            let stores = ["package_install", "package_content", "package_schema_migration"];
            for store in stores {
                run(root, "cargo", &["test", "-p", "aura-persistence-v2", "--test", store])?;
            }
            let core = ["package_program::", "package_services::", "l04_packages::"];
            run(
                root,
                "cargo",
                &["test", "-p", "aura-core", "--lib", "--", core[0], core[1], core[2]],
            )?;
        }
        "registry" => {
            run(
                root,
                "cargo",
                &[
                    "test",
                    "-p",
                    "aura-tool-contract",
                    "-p",
                    "aura-tool-domain",
                    "-p",
                    "aura-tool-registry",
                    "-p",
                    "aura-tool-projection",
                    "--no-fail-fast",
                ],
            )?;
            run(root, "cargo", &["test", "-p", "aura-core", "--lib", "l04_package_registry"])?;
        }
        "scheduler" => {
            run(root, "cargo", &["test", "-p", "aura-tool-scheduler"])?;
            run(
                root,
                "cargo",
                &["test", "-p", "aura-persistence", "--test", "p07_tool_execution"],
            )?;
        }
        "runtime" => {
            build_tool_runtime_fixtures(root)?;
            run(
                root,
                "cargo",
                &[
                    "test",
                    "-p",
                    "aura-tool-runtime",
                    "-p",
                    "aura-sandbox",
                    "-p",
                    "aura-sandbox-host",
                    "-p",
                    "aura-windows-platform",
                    "--no-fail-fast",
                ],
            )?;
            run(root, "cargo", &["test", "-p", "aura-core", "w7_host_adapter", "--lib"])?;
            // The source-built fixtures run as installed package programs.
            run(root, "cargo", &["test", "-p", "aura-core", "--lib", "package_program::tests"])?;
        }
        test_filter => {
            run(
                root,
                "cargo",
                &[
                    "test",
                    "-p",
                    "aura-tool-contract",
                    "-p",
                    "aura-artifact-contract",
                    "-p",
                    "aura-tool-domain",
                    "-p",
                    "aura-tool-registry",
                    "-p",
                    "aura-tool-scheduler",
                    "-p",
                    "aura-tool-runtime",
                    "-p",
                    "aura-tool-package",
                    "-p",
                    "aura-tool-projection",
                    "-p",
                    "aura-tool-fixtures",
                    test_filter,
                    "--no-fail-fast",
                ],
            )?;
        }
    }
    println!("P07 Tool test target '{target}' passed.");
    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must be located below the workspace root")
        .to_path_buf()
}

fn check_all(skip_e2e: bool) -> Result<()> {
    let root = workspace_root();
    architecture::check(&root)?;
    run(&root, "cargo", &["fmt", "--all", "--", "--check"])?;
    run(
        &root,
        "cargo",
        &["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"],
    )?;
    // Package program tests run the source-built fixtures from target/debug.
    build_tool_runtime_fixtures(&root)?;
    git_test_profile::run_tests(&root, &["--workspace", "--all-features"])?;
    contracts::check()?;
    manifests::validate_manifests(&root)?;
    let pnpm_check = if skip_e2e { "check:no-e2e" } else { "check" };
    run(&root, corepack_command(), &["pnpm", pnpm_check])?;
    println!(
        "Aura repository checks passed{}.",
        if skip_e2e { " without Electron E2E" } else { "" }
    );
    Ok(())
}

fn test_permission(root: &Path) -> Result<()> {
    run(
        root,
        "cargo",
        &[
            "test",
            "-p",
            "aura-permission-domain",
            "-p",
            "aura-permission-engine",
            "-p",
            "aura-approval",
        ],
    )?;
    run(root, "cargo", &["test", "-p", "aura-persistence", "--test", "p04_security"])?;
    run(root, "cargo", &["test", "-p", "aura-core", "p04"])?;
    println!("P04 permission and approval tests passed.");
    Ok(())
}

fn test_sandbox(root: &Path) -> Result<()> {
    run(
        root,
        "cargo",
        &[
            "test",
            "-p",
            "aura-sandbox",
            "-p",
            "aura-sandbox-host",
            "-p",
            "aura-elevated-helper",
            "-p",
            "aura-windows-platform",
        ],
    )?;
    println!(
        "P04 sandbox and elevation automated tests passed. Real AppContainer lowbox/loopback evidence is collected separately."
    );
    Ok(())
}

fn test_network(root: &Path) -> Result<()> {
    run(root, "cargo", &["test", "-p", "aura-network-control", "-p", "aura-egress-proxy"])?;
    println!("P04 network control and egress proxy tests passed.");
    Ok(())
}

fn experimental_processmodel_proxy(root: &Path) -> Result<()> {
    let output_path = root.join("target/experimental/processmodel-proxy/report.json");
    if output_path.exists() {
        fs::remove_file(&output_path)
            .with_context(|| format!("failed to remove stale {}", output_path.display()))?;
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    println!(
        "$ cargo test -p aura-windows-platform --features experimental-processmodel-proxy --test experimental_processmodel_proxy -- --ignored --nocapture"
    );
    let command_status = process::command("cargo")
        .args([
            "test",
            "-p",
            "aura-windows-platform",
            "--features",
            "experimental-processmodel-proxy",
            "--test",
            "experimental_processmodel_proxy",
            "--",
            "--ignored",
            "--nocapture",
        ])
        .env("AURA_EXPERIMENT_PROCESSMODEL_PROXY_REPORT", &output_path)
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to start experimental ProcessModel proxy probe")?;

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
    ensure!(
        command_status.success(),
        "experimental probe harness failed with {command_status}; see {}",
        output_path.display()
    );

    println!(
        "Experimental ProcessModel proxy probe recorded {report_status}: {}",
        output_path.display()
    );
    Ok(())
}

struct ReportCheck<'a> {
    id: &'a str,
    program: &'a str,
    args: &'a [&'a str],
}

const APPCONTAINER_EVIDENCE_FILE: &str = "target/p04-report/appcontainer-network-evidence.json";
const APPCONTAINER_EVIDENCE_TEST: &str =
    "tests::compatibility_appcontainer_has_lowbox_token_and_denies_loopback";
const P04_PHASE_STATUS: &str = "Pause";
const P04_PAUSE_DECISION_DATE: &str = "2026-08-30";
const P04_RESUME_AFTER_PHASE: &str = "P19";
const P04_RESUME_CONDITION: &str =
    "P19 phase is complete and the user explicitly resumes P04 acceptance";

fn appcontainer_network_evidence(root: &Path) -> Result<()> {
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

fn collect_appcontainer_network_evidence(root: &Path) -> Result<Value> {
    let evidence_path = root.join(APPCONTAINER_EVIDENCE_FILE);
    if evidence_path.exists() {
        fs::remove_file(&evidence_path)
            .with_context(|| format!("failed to remove stale {}", evidence_path.display()))?;
    }
    if let Some(parent) = evidence_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let command = format!(
        "cargo test -p aura-sandbox {APPCONTAINER_EVIDENCE_TEST} -- --ignored --exact --nocapture --test-threads=1"
    );
    println!("$ {command}");
    let command_status = process::command("cargo")
        .args([
            "test",
            "-p",
            "aura-sandbox",
            APPCONTAINER_EVIDENCE_TEST,
            "--",
            "--ignored",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("AURA_APPCONTAINER_NETWORK_EVIDENCE_REPORT", &evidence_path)
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to start AppContainer network evidence probe")?;

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
            "probe_exit_code": command_status.code()
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
    let valid_pass = command_status.success()
        && reported_status == "passed"
        && host_preparation_available
        && proves_lowbox
        && proves_loopback_denial;
    let reported_gate_status =
        evidence.get("gate_status").and_then(Value::as_str).unwrap_or("failed");
    let valid_pending_host_preparation = !command_status.success()
        && reported_status == "PendingHostPreparation"
        && reported_gate_status == "Inconclusive"
        && !host_preparation_available
        && !proves_lowbox
        && !proves_loopback_denial;

    let object =
        evidence.as_object_mut().context("AppContainer evidence report must be a JSON object")?;
    object.insert("probe_command".to_owned(), Value::String(command));
    object.insert("probe_exit_code".to_owned(), json!(command_status.code()));
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

fn p04_report(root: &Path) -> Result<()> {
    let corepack = corepack_command();
    let checks = [
        ReportCheck {
            id: "permission_and_approval",
            program: "cargo",
            args: &["xtask", "test-permission"],
        },
        ReportCheck {
            id: "sandbox_and_elevation",
            program: "cargo",
            args: &["xtask", "test-sandbox"],
        },
        ReportCheck {
            id: "network_and_quarantine",
            program: "cargo",
            args: &["xtask", "test-network"],
        },
        ReportCheck {
            id: "p03_p04_write_gate",
            program: "cargo",
            args: &[
                "test",
                "-p",
                "aura-core",
                "authenticated_main_gate_covers_every_workspace_mutation_and_no_lifecycle_command",
            ],
        },
        ReportCheck {
            id: "generated_contracts",
            program: "cargo",
            args: &["xtask", "contracts", "--check"],
        },
        ReportCheck {
            id: "release_manifests",
            program: "cargo",
            args: &["xtask", "validate-manifests"],
        },
        ReportCheck {
            id: "desktop_typecheck",
            program: corepack,
            args: &["pnpm", "--filter", "@aura/desktop", "typecheck"],
        },
        ReportCheck {
            id: "desktop_unit_tests",
            program: corepack,
            args: &["pnpm", "--filter", "@aura/desktop", "test:unit"],
        },
        ReportCheck {
            id: "desktop_electron_e2e",
            program: corepack,
            args: &["pnpm", "--filter", "@aura/desktop", "test:e2e"],
        },
    ];

    let mut results = Vec::with_capacity(checks.len());
    for check in checks {
        println!("$ {} {}", check.program, check.args.join(" "));
        let status = process::command(check.program)
            .args(check.args)
            .current_dir(root)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("failed to start P04 report check {}", check.id))?;
        results.push(json!({
            "id": check.id,
            "command": format!("{} {}", check.program, check.args.join(" ")),
            "status": if status.success() { "passed" } else { "failed" },
            "exit_code": status.code(),
        }));
    }

    let all_passed = results.iter().all(|result| result["status"] == "passed");
    let appcontainer_evidence =
        collect_appcontainer_network_evidence(root).unwrap_or_else(|error| {
            json!({
                "schema_version": 1,
                "evidence_id": "appcontainer_lowbox_token_and_loopback_denial",
                "status": "failed",
                "gate_status": "failed",
                "reason": format!("evidence_probe_harness_failed: {error:#}"),
                "host_preparation_available": null,
                "required_assertions": {
                    "child_token_is_appcontainer_lowbox": null,
                    "loopback_connection_denied": null
                }
            })
        });
    let appcontainer_evidence_status =
        appcontainer_evidence.get("status").and_then(Value::as_str).unwrap_or("failed");
    let appcontainer_evidence_passed = appcontainer_evidence_status == "passed";
    let generated_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is earlier than the Unix epoch")?
        .as_millis();
    let report = json!({
        "schema_version": 1,
        "phase": "P04",
        "phase_status": P04_PHASE_STATUS,
        "generated_at_unix_ms": generated_at_unix_ms,
        "automated_status": if all_passed { "passed" } else { "failed" },
        "release_gate_status": if all_passed { "Inconclusive" } else { "failed" },
        "completion_claim": false,
        "scope_note": "P04 is paused by user decision. This report records repeatable automated evidence only and does not replace the frozen P04 exit gates or claim completion.",
        "pause": {
            "decision_date": P04_PAUSE_DECISION_DATE,
            "resume_after_phase": P04_RESUME_AFTER_PHASE,
            "resume_condition": P04_RESUME_CONDITION,
            "deferred_acceptance": [
                {
                    "id": "production_release_trust_and_host_preparation",
                    "description": "an Aura Production Root-trusted release bundle completes real Host Preparation and the user-authorized UAC binding"
                },
                {
                    "id": "real_appcontainer_isolation",
                    "description": "a real Compatibility AppContainer proves a lowbox token and loopback denial with zero network capabilities and no loopback exemption"
                }
            ]
        },
        "checks": results,
        "appcontainer_network_evidence": appcontainer_evidence,
        "required_manual_evidence": [
            "standard-user and administrator launch paths",
            "Sandbox Host and Egress Proxy authenticated process control",
            "a real Compatibility AppContainer child has a lowbox/AppContainer token and loopback connection is denied with zero network capabilities and no loopback exemption",
            "host-preparation UAC confirmation binding",
            "quarantine release and discard confirmation binding",
            "post-test process, ACL, firewall, AppContainer profile, grant, and temporary-file cleanup"
        ]
    });
    let output_dir = root.join("target/p04-report");
    fs::create_dir_all(&output_dir).context("failed to create P04 report directory")?;
    let json_path = output_dir.join("P04-test-report.json");
    fs::write(&json_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("failed to write {}", json_path.display()))?;
    let html_path = output_dir.join("P04-test-report.html");
    fs::write(&html_path, render_p04_report_html(&report))
        .with_context(|| format!("failed to write {}", html_path.display()))?;
    println!("generated {}", display_relative(root, &json_path));
    println!("generated {}", display_relative(root, &html_path));

    ensure!(all_passed, "one or more P04 automated report checks failed");
    ensure!(
        appcontainer_evidence_passed,
        "required AppContainer lowbox-token and loopback-denial evidence is {appcontainer_evidence_status}; the P04 report was generated but remains incomplete"
    );
    Ok(())
}

fn render_p04_report_html(report: &Value) -> String {
    let checks = report["checks"].as_array().expect("P04 report checks");
    let rows = checks
        .iter()
        .map(|check| {
            let status = check["status"].as_str().unwrap_or("failed");
            format!(
                "<tr><td>{}</td><td><code>{}</code></td><td class=\"{}\">{}</td></tr>",
                escape_html(check["id"].as_str().unwrap_or("unknown")),
                escape_html(check["command"].as_str().unwrap_or("")),
                status,
                status
            )
        })
        .collect::<String>();
    let evidence = report["required_manual_evidence"]
        .as_array()
        .expect("P04 manual evidence")
        .iter()
        .map(|item| format!("<li>{}</li>", escape_html(item.as_str().unwrap_or(""))))
        .collect::<String>();
    let appcontainer_evidence = &report["appcontainer_network_evidence"];
    let appcontainer_evidence_status = appcontainer_evidence["status"].as_str().unwrap_or("failed");
    let appcontainer_evidence_reason = appcontainer_evidence["reason"].as_str().unwrap_or("none");
    let appcontainer_evidence_gate_status =
        appcontainer_evidence["gate_status"].as_str().unwrap_or("failed");
    let release_gate_status = report["release_gate_status"].as_str().unwrap_or("Inconclusive");
    let phase_status = report["phase_status"].as_str().unwrap_or("unknown");
    let resume_condition =
        report.pointer("/pause/resume_condition").and_then(Value::as_str).unwrap_or("unspecified");
    let deferred_acceptance = report
        .pointer("/pause/deferred_acceptance")
        .and_then(Value::as_array)
        .expect("P04 deferred acceptance")
        .iter()
        .map(|item| {
            format!(
                "<li><code>{}</code>: {}</li>",
                escape_html(item["id"].as_str().unwrap_or("unknown")),
                escape_html(item["description"].as_str().unwrap_or(""))
            )
        })
        .collect::<String>();
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Aura P04 test report</title><style>body{{margin:0;background:#f7f9fc;color:#172033;font:15px/1.5 system-ui,sans-serif}}main{{max-width:1040px;margin:40px auto;padding:32px;background:#fff;border:1px solid #dce3ee;border-radius:16px;box-shadow:0 12px 40px rgba(43,58,85,.08)}}h1{{margin-top:0}}table{{width:100%;border-collapse:collapse}}th,td{{padding:12px;text-align:left;border-bottom:1px solid #e6ebf2}}code{{color:#34445f;word-break:break-word}}.passed{{color:#177245;font-weight:700}}.failed{{color:#b42318;font-weight:700}}.Pause,.PendingHostPreparation,.Inconclusive{{color:#9a6700;font-weight:700}}.note{{padding:12px 16px;background:#f0f5ff;border-radius:10px}}</style></head><body><main><h1>Aura P04 automated test report</h1><p class=\"note\">{}</p><p>Phase status: <strong class=\"{}\">{}</strong></p><p>Automated status: <strong>{}</strong></p><p>Release gate status: <strong class=\"{}\">{}</strong></p><table><thead><tr><th>Check</th><th>PowerShell command</th><th>Status</th></tr></thead><tbody>{}</tbody></table><h2>Pause decision</h2><p>Resume condition: {}</p><ul>{}</ul><h2>AppContainer network evidence</h2><p>Status: <strong class=\"{}\">{}</strong>; gate: <strong class=\"{}\">{}</strong>; reason: <code>{}</code>.</p><p>Required proof: the child token is lowbox/AppContainer and loopback connection is denied with zero network capabilities and no loopback exemption.</p><h2>Required manual evidence</h2><ul>{}</ul></main></body></html>",
        escape_html(report["scope_note"].as_str().unwrap_or("")),
        escape_html(phase_status),
        escape_html(phase_status),
        escape_html(report["automated_status"].as_str().unwrap_or("failed")),
        escape_html(release_gate_status),
        escape_html(release_gate_status),
        rows,
        escape_html(resume_condition),
        deferred_acceptance,
        escape_html(appcontainer_evidence_status),
        escape_html(appcontainer_evidence_status),
        escape_html(appcontainer_evidence_gate_status),
        escape_html(appcontainer_evidence_gate_status),
        escape_html(appcontainer_evidence_reason),
        evidence
    )
}

fn p05_report(root: &Path) -> Result<()> {
    let corepack = corepack_command();
    let checks = [
        ReportCheck {
            id: "contract_generation",
            program: "cargo",
            args: &["xtask", "contracts", "--check"],
        },
        ReportCheck {
            id: "model_config_and_catalog",
            program: "cargo",
            args: &[
                "test",
                "-p",
                "aura-model-domain",
                "-p",
                "aura-model-config",
                "-p",
                "aura-model-catalog",
                "--all-features",
            ],
        },
        ReportCheck {
            id: "transport_and_protocol_adapters",
            program: "cargo",
            args: &[
                "test",
                "-p",
                "aura-model-transport",
                "-p",
                "aura-model-adapter-openai-responses",
                "-p",
                "aura-model-adapter-openai-chat",
                "-p",
                "aura-model-adapter-anthropic",
                "-p",
                "aura-model-fixtures",
                "--all-features",
            ],
        },
        ReportCheck {
            id: "gateway_and_persistence",
            program: "cargo",
            args: &[
                "test",
                "-p",
                "aura-network-control",
                "-p",
                "aura-persistence",
                "--all-features",
            ],
        },
        ReportCheck {
            id: "core_p05_vertical_paths",
            program: "cargo",
            args: &["test", "-p", "aura-core", "p05", "--all-features"],
        },
        ReportCheck {
            id: "desktop_typecheck",
            program: corepack,
            args: &["pnpm", "--filter", "@aura/desktop", "typecheck"],
        },
        ReportCheck {
            id: "desktop_unit_tests",
            program: corepack,
            args: &["pnpm", "--filter", "@aura/desktop", "test:unit"],
        },
        ReportCheck {
            id: "desktop_electron_e2e",
            program: corepack,
            args: &["pnpm", "--filter", "@aura/desktop", "test:e2e"],
        },
    ];

    let mut results = Vec::with_capacity(checks.len());
    for check in checks {
        println!("$ {} {}", check.program, check.args.join(" "));
        let status = process::command(check.program)
            .args(check.args)
            .current_dir(root)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("failed to start P05 report check {}", check.id))?;
        results.push(json!({
            "id": check.id,
            "command": format!("{} {}", check.program, check.args.join(" ")),
            "status": if status.success() { "passed" } else { "failed" },
            "exit_code": status.code(),
        }));
    }

    let all_passed = results.iter().all(|result| result["status"] == "passed");
    let generated_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is earlier than the Unix epoch")?
        .as_millis();
    let report = json!({
        "schema_version": 1,
        "phase": "P05",
        "generated_at_unix_ms": generated_at_unix_ms,
        "automated_status": if all_passed { "passed" } else { "failed" },
        "real_profile_acceptance": "pending_user_configuration",
        "completion_claim": false,
        "scope_note": "This report contains only stable check identifiers, commands, and terminal status. It never records API keys, Provider response bodies, prompt text, user source, request sizes, masks, or character features.",
        "checks": results,
        "remaining_manual_acceptance": [
            "The user configures one real Model Profile for each supported protocol adapter.",
            "Each saved Profile completes one real low-cost models.profile.test_availability request through the P04 Gateway.",
            "The generated report and diagnostic artifacts are inspected for secret, prompt, and user-source leakage."
        ]
    });
    let output_dir = root.join("target/p05-report");
    fs::create_dir_all(&output_dir).context("failed to create P05 report directory")?;
    let json_path = output_dir.join("P05-test-report.json");
    fs::write(&json_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("failed to write {}", json_path.display()))?;
    let html_path = output_dir.join("P05-test-report.html");
    fs::write(&html_path, render_p05_report_html(&report))
        .with_context(|| format!("failed to write {}", html_path.display()))?;
    println!("generated {}", display_relative(root, &json_path));
    println!("generated {}", display_relative(root, &html_path));

    ensure!(all_passed, "one or more P05 automated report checks failed");
    Ok(())
}

fn render_p05_report_html(report: &Value) -> String {
    let rows = report["checks"]
        .as_array()
        .expect("P05 report checks")
        .iter()
        .map(|check| {
            let status = check["status"].as_str().unwrap_or("failed");
            format!(
                "<tr><td>{}</td><td><code>{}</code></td><td class=\"{}\">{}</td></tr>",
                escape_html(check["id"].as_str().unwrap_or("unknown")),
                escape_html(check["command"].as_str().unwrap_or("")),
                status,
                status
            )
        })
        .collect::<String>();
    let manual = report["remaining_manual_acceptance"]
        .as_array()
        .expect("P05 manual acceptance")
        .iter()
        .map(|item| format!("<li>{}</li>", escape_html(item.as_str().unwrap_or(""))))
        .collect::<String>();
    let automated_status = report["automated_status"].as_str().unwrap_or("failed");
    let real_profile_status =
        report["real_profile_acceptance"].as_str().unwrap_or("pending_user_configuration");
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Aura P05 test report</title><style>body{{margin:0;background:#f7f9fc;color:#172033;font:15px/1.5 system-ui,sans-serif}}main{{max-width:1040px;margin:40px auto;padding:32px;background:#fff;border:1px solid #dce3ee;border-radius:16px;box-shadow:0 12px 40px rgba(43,58,85,.08)}}h1{{margin-top:0}}table{{width:100%;border-collapse:collapse}}th,td{{padding:12px;text-align:left;border-bottom:1px solid #e6ebf2}}code{{color:#34445f;word-break:break-word}}.passed{{color:#177245;font-weight:700}}.failed{{color:#b42318;font-weight:700}}.pending_user_configuration{{color:#9a6700;font-weight:700}}.note{{padding:12px 16px;background:#f0f5ff;border-radius:10px}}</style></head><body><main><h1>Aura P05 test report</h1><p class=\"note\">{}</p><p>Automated status: <strong class=\"{}\">{}</strong></p><p>Real Profile acceptance: <strong class=\"{}\">{}</strong></p><table><thead><tr><th>Check</th><th>Command</th><th>Status</th></tr></thead><tbody>{}</tbody></table><h2>Remaining manual acceptance</h2><ul>{}</ul></main></body></html>",
        escape_html(report["scope_note"].as_str().unwrap_or("")),
        escape_html(automated_status),
        escape_html(automated_status),
        escape_html(real_profile_status),
        escape_html(real_profile_status),
        rows,
        manual
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn generate_contracts(root: &Path) -> Result<()> {
    let generated = aura_contract_schema::write_rust_generated_artifacts(root)?;
    let mut actual = generated.clone();
    let mut registered =
        RUST_GENERATED_PATHS.iter().map(|path| root.join(path)).collect::<Vec<_>>();
    actual.sort();
    registered.sort();
    ensure!(actual == registered, "generated artifact inventory differs from RUST_GENERATED_PATHS");
    for path in generated {
        println!("generated {}", display_relative(root, &path));
    }

    let schema = root.join(SCHEMA_PATH);
    let output = root.join(TYPESCRIPT_CONTRACT_PATH);
    run_node_generator(
        &workspace_root(),
        &schema,
        &output,
        &root.join(BUILTIN_TOOL_DEFINITION_SET_PATH),
    )?;
    println!("generated {}", display_relative(root, &output));
    run(
        &workspace_root(),
        "node",
        &[
            "scripts/generate-routes.mjs",
            "--input",
            &root.join("resources/ipc/routes.v1.json").to_string_lossy(),
            "--output",
            &root.join(TYPESCRIPT_ROUTES_PATH).to_string_lossy(),
        ],
    )?;
    Ok(())
}

fn run_node_generator(
    command_root: &Path,
    schema: &Path,
    output: &Path,
    builtin_definitions: &Path,
) -> Result<()> {
    let schema = schema.to_string_lossy().into_owned();
    let output = output.to_string_lossy().into_owned();
    let builtin_definitions = builtin_definitions.to_string_lossy().into_owned();
    run(
        command_root,
        "node",
        &[
            "scripts/generate-contracts.mjs",
            "--schema",
            schema.as_str(),
            "--output",
            output.as_str(),
            "--builtin-definitions",
            builtin_definitions.as_str(),
        ],
    )
}

pub(crate) fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .with_context(|| format!("{field} must be a non-empty string"))
}

fn build_tool_runtime_fixtures(root: &Path) -> Result<()> {
    run(
        root,
        "cargo",
        &[
            "build",
            "-p",
            "aura-tool-fixtures",
            "--bin",
            "aura-stdio-fixture",
            "--bin",
            "aura-command-fixture",
        ],
    )?;
    let target_root = env::var_os("CARGO_TARGET_DIR").map_or_else(
        || root.join("target"),
        |value| {
            let path = PathBuf::from(value);
            if path.is_absolute() { path } else { root.join(path) }
        },
    );
    let stdio = target_root.join("debug").join(if cfg!(windows) {
        "aura-stdio-fixture.exe"
    } else {
        "aura-stdio-fixture"
    });
    ensure!(stdio.is_file(), "source-built Aura Stdio fixture is missing at {}", stdio.display());
    let command = target_root.join("debug").join(if cfg!(windows) {
        "aura-command-fixture.exe"
    } else {
        "aura-command-fixture"
    });
    ensure!(
        command.is_file(),
        "source-built declarative command fixture is missing at {}",
        command.display()
    );
    Ok(())
}

#[cfg(windows)]
const fn corepack_command() -> &'static str {
    "corepack.cmd"
}

#[cfg(not(windows))]
const fn corepack_command() -> &'static str {
    "corepack"
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}
