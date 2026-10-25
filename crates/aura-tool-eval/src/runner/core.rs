use crate::{
    budget::{BudgetStage, Ledger, Limits, RequestRecord},
    fixtures::{prepare, score, task},
    meter::{Meter, MeterConfig, MeterPurpose, Upstream},
    owner::{OwnerTarget, RequestSettings},
    replay::{Replay, ReplayStep},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    pub task_id: String,
    pub run_id: String,
    pub root: PathBuf,
    pub ledger_path: PathBuf,
    pub desktop_root: PathBuf,
    pub limits: Limits,
    pub mode: String,
    pub approval_decision: String,
    pub deadline_seconds: u64,
    pub replay: Vec<ReplayStep>,
    /// Real calls require a separate explicit runtime option, never inferred from a config file.
    pub model: String,
    pub request_settings: RequestSettings,
    /// The stage whose approved budget and ledger real calls use; older configs are `S09`'s.
    #[serde(default)]
    pub budget_stage: BudgetStage,
}
#[derive(Serialize)]
pub struct RunReport {
    pub task: String,
    pub provider: String,
    pub ipc: Value,
    pub requests: Vec<RequestRecord>,
    pub charged_tokens: u64,
    pub cumulative_tokens: u64,
    pub cached_input_tokens: u64,
    pub score: Option<super::super::fixtures::Score>,
    pub outcome: String,
    pub stop_reason: Option<String>,
    pub versions: Value,
    pub target: Option<OwnerTarget>,
    pub configuration_differences: Value,
}
pub async fn run(config: RunConfig, real: Option<OwnerTarget>) -> Result<RunReport> {
    ensure!(
        ["read_only", "workspace_read_write", "full_access"].contains(&config.mode.as_str()),
        "eval.invalid_mode"
    );
    ensure!(
        ["allow_once", "deny"].contains(&config.approval_decision.as_str()),
        "eval.invalid_approval_policy"
    );
    ensure!((10..=1800).contains(&config.deadline_seconds), "eval.invalid_deadline");
    ensure!(
        config.limits.request_output == config.request_settings.output_cap()?,
        "eval.output_limit_not_owner_setting"
    );
    let task = task(&config.task_id).ok_or_else(|| anyhow::anyhow!("eval.unknown_task"))?;
    ensure!(
        !config.run_id.is_empty()
            && config.run_id.len() <= 64
            && config.run_id.chars().all(|v| v.is_ascii_alphanumeric() || v == '-'),
        "eval.invalid_run_id"
    );
    if let Some(target) = &real {
        target.verify_current()?;
        ensure!(target.model == config.model, "eval.owner_model_mismatch");
        ensure!(target.settings == config.request_settings, "eval.owner_settings_mismatch");
    }
    let prepared = prepare(&config.root, task)?;
    let ledger_path = if real.is_some() {
        ensure!(config.budget_stage.admits(&config.limits), "eval.stage_budget_exceeded");
        crate::budget::real_ledger_path(config.budget_stage)?
    } else {
        config.ledger_path.clone()
    };
    let ledger = Arc::new(Ledger::open(&ledger_path, config.limits)?);
    let provider = if real.is_some() { "DeepSeek" } else { "replay" };
    let upstream = match &real {
        Some(target) => Upstream::DeepSeek { target: Box::new(target.clone()) },
        None => Upstream::Replay(Arc::new(Replay::new(config.replay))),
    };
    let key = format!("{}-{}", task.id, config.run_id);
    let meter = Meter::start(MeterConfig {
        task: key.clone(),
        output_cap: config.limits.request_output,
        ledger: Arc::clone(&ledger),
        upstream,
        purpose: MeterPurpose::Coding,
        api_format: config.request_settings.api_format,
    })
    .await?;
    let prompt = format!(
        "Work only in this isolated task workspace. Do not access Aura, other repositories, host configuration, credentials or network services. Do not install anything. Use only existing local tools. Finish with a concise explanation. Task: {}",
        task.prompt
    );
    let input = serde_json::json!({"workspace":prepared.workspace,"data_root":config.root.join("core-data").join(task.id),"desktop_root":config.desktop_root,"provider_url":meter.url,"model":config.model,"prompt":prompt,"mode":config.mode,"approval_decision":config.approval_decision,"request_settings":config.request_settings,"timeouts":crate::meter::core_timeouts(),"deadline_seconds":config.deadline_seconds});
    let path = config.root.join(format!("{}-ipc-config.json", task.id));
    std::fs::write(&path, serde_json::to_vec_pretty(&input)?)?;
    let mut command = std::process::Command::new("node");
    command
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("support/core-ipc.mjs"))
        .arg(&path);
    let timeout = std::time::Duration::from_secs(config.deadline_seconds + 120);
    let output =
        tokio::task::spawn_blocking(move || crate::process::run(&command, timeout)).await??;
    let meter_url = meter.url.clone();
    // Core can abandon a request whose upstream call still completes; settle it before reporting.
    if meter.finish().await.is_err() {
        for request in
            ledger.records()?.into_iter().filter(|r| r.task == key && r.state == "pending")
        {
            ledger.unresolved(request.id)?;
        }
    }
    if provider == "replay" {
        std::fs::write(
            config.root.join(format!("{}-controller-replay.log", task.id)),
            &output.stderr,
        )?;
    }
    let ipc: Value = if output.timed_out || output.exit_code != Some(0) {
        stop_controller(&ledger, &key)?;
        serde_json::json!({"state":"controller_failed","stop_confirmed":false,"controller_exit_code":output.exit_code,"controller_timed_out":output.timed_out})
    } else {
        match serde_json::from_slice(&output.stdout) {
            Ok(value) => value,
            Err(_) => {
                stop_controller(&ledger, &key)?;
                serde_json::json!({"state":"controller_report_invalid","stop_confirmed":false})
            }
        }
    };
    let requests = ledger.records()?.into_iter().filter(|r| r.task == key).collect::<Vec<_>>();
    let global_stop_reason = ledger.stop_reason()?;
    let stop_reason = global_stop_reason.or(ledger.task_stop_reason(&key)?);
    let stopped = stop_reason.is_some()
        || requests.iter().any(|r| r.state != "settled")
        || ipc["state"] != "completed"
        || ipc["stop_confirmed"] != true;
    let score = if !stopped { Some(score(&prepared)?) } else { None };
    let outcome = if stop_reason.as_deref() == Some("budget_exhausted") {
        "budget_terminated"
    } else if stop_reason.is_some() || requests.iter().any(|r| r.state != "settled") {
        match stop_reason.as_deref() {
            Some("provider_refused") => "provider_refused",
            // A 5xx, timeout or lost connection: charged in full, this task only.
            Some("provider_unconfirmed") => "provider_unconfirmed",
            _ => "usage_or_ledger_stop",
        }
    } else if ipc["stop_confirmed"] != true {
        "stop_unconfirmed"
    } else if ipc["state"] != "completed" {
        "incomplete"
    } else if score.as_ref().is_some_and(|s| s.passed) {
        "passed"
    } else {
        "failed"
    };
    let report = RunReport {
        task: task.id.into(),
        provider: provider.into(),
        ipc,
        charged_tokens: requests
            .iter()
            .map(|r| r.input.zip(r.output).map_or(r.reserved, |(a, b)| a + b))
            .sum(),
        cumulative_tokens: ledger.charged()?,
        cached_input_tokens: requests.iter().filter_map(|r| r.cached_input).sum(),
        requests,
        score,
        outcome: outcome.into(),
        stop_reason,
        versions: serde_json::json!({"fixture":if matches!(task.id,"R06"|"T06"|"P06"){"coding-scale-v1"}else{"coding-v1"},"request_goldens":"s10-followups-v1","budget_stage":config.budget_stage,"model":config.model,"request_settings":config.request_settings,"mode":config.mode,"approval_policy":config.approval_decision,"source_base":source_base(&config.desktop_root)?}),
        target: real,
        configuration_differences: serde_json::json!({"base_url":meter_url,"authentication":"none at Core; existing WinCred in meter memory","core_timeouts":input["timeouts"],"upstream_timeout_seconds":crate::meter::UPSTREAM_TIMEOUT_SECONDS,"meter_connection_timeout_seconds":crate::meter::CONNECTION_TIMEOUT_SECONDS}),
    };
    std::fs::write(
        config.root.join(format!("{}-report.json", task.id)),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn stop_controller(ledger: &Ledger, key: &str) -> Result<()> {
    let pending = ledger
        .records()?
        .into_iter()
        .filter(|r| r.task == key && r.state == "pending")
        .collect::<Vec<_>>();
    if pending.is_empty() {
        if ledger.stop_reason()?.is_none() {
            ledger.stop_task(key, "controller_failed")?;
        }
    } else {
        for request in pending {
            ledger.unresolved(request.id)?;
        }
    }
    Ok(())
}

fn source_base(desktop: &std::path::Path) -> Result<String> {
    let result = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(desktop)
        .output()?;
    ensure!(result.status.success(), "eval.source_version_unavailable");
    Ok(String::from_utf8(result.stdout)?.trim().into())
}
