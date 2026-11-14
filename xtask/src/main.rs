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
mod evidence;
mod git_test_profile;
mod manifests;
mod process;
mod public;
mod r06;
mod rust_tests;
mod stage_tests;
mod sweep_target;

use process::run;

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
        Some("test" | "test-isolated") => rust_tests::test(&workspace_root(), args.collect()),
        Some("clippy") => {
            ensure!(args.next().is_none(), "clippy received unexpected arguments");
            rust_tests::clippy(&workspace_root())
        }
        Some("sweep-target") => sweep_target::run(&workspace_root(), args.collect()),
        Some("generate-contracts") => generate_contracts(&workspace_root()),
        Some("contracts") if args.next().as_deref() == Some("--check") => contracts::check(),
        Some("test-permission") => stage_tests::permission(&workspace_root()),
        Some("test-sandbox") => stage_tests::sandbox(&workspace_root()),
        Some("test-network") => stage_tests::network(&workspace_root()),
        Some("test-tool") => {
            let target = args.next().context("test-tool requires a namespace or test filter")?;
            ensure!(args.next().is_none(), "test-tool received unexpected arguments");
            stage_tests::tool(&workspace_root(), &target)
        }
        Some("appcontainer-network-evidence") => {
            evidence::appcontainer_network_evidence(&workspace_root())
        }
        Some("experimental-processmodel-proxy") => {
            evidence::experimental_processmodel_proxy(&workspace_root())
        }
        Some("p04-report") => p04_report(&workspace_root()),
        Some("p05-report") => p05_report(&workspace_root()),
        Some("validate-manifests") => manifests::validate_manifests(&workspace_root()),
        Some("scan-secrets") => public::scan_secrets(args.collect()),
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
           cargo xtask test [-p <crate>]... [--lib] [--bins] [--tests] [--test <name>]... [-- <test arguments>]\n\
           cargo xtask clippy\n\
           cargo xtask sweep-target [--apply] [--idle-days <n> | --idle-hours <n>] [--unused]\n\
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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must be located below the workspace root")
        .to_path_buf()
}

/// A named step of `check-all` that runs even after an earlier one failed.
type CheckStep<'a> = (&'static str, Box<dyn Fn() -> Result<()> + 'a>);

fn check_all(skip_e2e: bool) -> Result<()> {
    let root = workspace_root();
    architecture::check(&root)?;
    architecture::check_format(&root)?;
    rust_tests::clippy(&root)?;
    // From the tests on, every step runs even after a failure, so one run shows them all.
    let pnpm_check = if skip_e2e { "check:no-e2e" } else { "check" };
    let steps: [CheckStep<'_>; 4] = [
        ("Rust tests", Box::new(|| rust_tests::workspace(&root))),
        ("contracts", Box::new(contracts::check)),
        ("manifests", Box::new(|| manifests::validate_manifests(&root))),
        ("pnpm", Box::new(|| run(&root, corepack_command(), &["pnpm", pnpm_check]))),
    ];
    let failed: Vec<String> = steps
        .iter()
        .filter_map(|(name, step)| step().err().map(|error| format!("{name}: {error:#}")))
        .collect();
    ensure!(failed.is_empty(), "check-all failed:\n  {}", failed.join("\n  "));
    println!(
        "Aura repository checks passed{}.",
        if skip_e2e { " without Electron E2E" } else { "" }
    );
    Ok(())
}

struct ReportCheck<'a> {
    id: &'a str,
    program: &'a str,
    args: &'a [&'a str],
}

const P04_PHASE_STATUS: &str = "Pause";
const P04_PAUSE_DECISION_DATE: &str = "2026-08-30";
const P04_RESUME_AFTER_PHASE: &str = "P19";
const P04_RESUME_CONDITION: &str =
    "P19 phase is complete and the user explicitly resumes P04 acceptance";

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
                "xtask",
                "test",
                "-p",
                "aura-core",
                "--",
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
    let appcontainer_evidence = evidence::collect_appcontainer_network_evidence(root)
        .unwrap_or_else(|error| {
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
                "xtask",
                "test",
                "-p",
                "aura-model-domain",
                "-p",
                "aura-model-config",
                "-p",
                "aura-model-catalog",
            ],
        },
        ReportCheck {
            id: "transport_and_protocol_adapters",
            program: "cargo",
            args: &[
                "xtask",
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
            ],
        },
        ReportCheck {
            id: "gateway_and_persistence",
            program: "cargo",
            args: &["xtask", "test", "-p", "aura-network-control", "-p", "aura-persistence"],
        },
        ReportCheck {
            id: "core_p05_vertical_paths",
            program: "cargo",
            args: &["xtask", "test", "-p", "aura-core", "-p", "aura-model-service", "--", "p05"],
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

#[cfg(windows)]
const fn corepack_command() -> &'static str {
    "corepack.cmd"
}

#[cfg(not(windows))]
const fn corepack_command() -> &'static str {
    "corepack"
}

pub(crate) fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}
