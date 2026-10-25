use super::{arguments, cases_for};
use crate::{
    budget::{BudgetStage, Ledger, Limits, RequestRecord, real_ledger_path},
    meter::{Meter, MeterConfig, MeterPurpose, ProviderError, Upstream},
    owner::{OwnerTarget, RequestSettings},
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{path::Path, sync::Arc};

#[derive(Serialize)]
pub struct ProbeResult {
    pub case: String,
    pub provider_status: Option<u16>,
    pub valid_function_arguments: Option<bool>,
    pub outcome: String,
    pub provider_error: Option<ProviderError>,
    pub produced_function_call: bool,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub incomplete_reason: Option<String>,
}
#[derive(Serialize)]
pub struct ProbeReport {
    pub cases: Vec<ProbeResult>,
    pub requests: Vec<RequestRecord>,
    pub cumulative_tokens: u64,
    pub stop_reason: Option<String>,
    pub target: OwnerTarget,
    pub run_id: String,
    pub configuration_differences: serde_json::Value,
}

/// Only called by the separately confirmed CLI command. No raw response is written.
pub async fn run_probe(root: &Path, target: OwnerTarget) -> Result<ProbeReport> {
    target.verify_current()?;
    let run_id = root
        .file_name()
        .and_then(|v| v.to_str())
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 64
                && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        .ok_or_else(|| anyhow::anyhow!("eval.invalid_probe_run_id"))?
        .to_owned();
    let task_key = format!("s09-schema-probe-{run_id}");
    let output = target.settings.output_cap()?;
    ensure!(output == 8192, "eval.probe_output_changed_reestimate_required");
    ensure!(!root.exists(), "eval.probe_directory_already_used");
    std::fs::create_dir_all(root)?;
    let ledger = Arc::new(Ledger::open(
        &real_ledger_path(BudgetStage::S09)?,
        Limits {
            cumulative: 50_000_000,
            task: 98_304,
            request_input: 262_144,
            request_output: output,
        },
    )?);
    let meter = Meter::start(MeterConfig {
        task: task_key.clone(),
        output_cap: output,
        ledger: Arc::clone(&ledger),
        upstream: Upstream::DeepSeek { target: Box::new(target.clone()) },
        purpose: MeterPurpose::Probe,
        api_format: target.settings.api_format,
    })
    .await?;
    let results =
        evaluate_cases_with_settings(&target.model, &target.settings, &meter, &ledger).await?;
    let report = ProbeReport {
        cases: results,
        requests: ledger.records()?.into_iter().filter(|r| r.task == task_key).collect(),
        cumulative_tokens: ledger.charged()?,
        stop_reason: ledger.stop_reason()?.or(ledger.task_stop_reason(&task_key)?),
        target,
        run_id,
        configuration_differences: serde_json::json!({"authentication":"in-memory meter only; none at isolated Core","upstream_timeout_seconds":crate::meter::UPSTREAM_TIMEOUT_SECONDS,"connection_timeout_seconds":crate::meter::CONNECTION_TIMEOUT_SECONDS,"base_url":meter.url}),
    };
    std::fs::write(root.join("schema-probe-report.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}

/// The same probe loop is used with loopback providers in deterministic tests.
pub async fn evaluate_cases(
    model: &str,
    meter: &Meter,
    ledger: &Ledger,
) -> Result<Vec<ProbeResult>> {
    let settings = RequestSettings {
        api_format: meter.api_format(),
        request_defaults: aura_model_domain::ModelRequestDefaults {
            max_output_tokens: Some(meter.output_cap()),
            ..Default::default()
        },
        ..Default::default()
    };
    evaluate_cases_with_settings(model, &settings, meter, ledger).await
}
pub async fn evaluate_cases_with_settings(
    model: &str,
    settings: &RequestSettings,
    meter: &Meter,
    ledger: &Ledger,
) -> Result<Vec<ProbeResult>> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(crate::meter::CONNECTION_TIMEOUT_SECONDS))
        .build()?;
    let mut results = Vec::new();
    for case in cases_for(model, settings) {
        if ledger.stop_reason()?.is_some() || ledger.task_stop_reason(meter.task_key())?.is_some() {
            break;
        }
        ensure!(
            serde_json::to_vec(&case.request)?.len() + 4096 <= 8192,
            "eval.probe_exceeds_approved_reservation"
        );
        let previous_id = ledger.records()?.last().map_or(0, |r| r.id);
        let response = client
            .post(format!("{}{}", meter.url, settings.api_format.suffix()))
            .json(&case.request)
            .send()
            .await;
        let schema = match settings.api_format {
            crate::owner::ApiFormat::OpenAiResponses => &case.request["tools"][0]["parameters"],
            _ => &case.request["tools"][0]["function"]["parameters"],
        };
        let answer = match response {
            Ok(response) if response.status().is_success() => {
                Some(arguments::inspect(&response.bytes().await?, schema, settings.api_format)?)
            }
            Ok(_) => None,
            Err(_) => {
                ledger.halt();
                None
            }
        };
        let record = ledger
            .records()?
            .into_iter()
            .find(|r| r.id > previous_id && r.task == meter.task_key());
        let valid = answer.as_ref().map(|v| v.valid);
        results.push(ProbeResult {
            case: case.name.into(),
            provider_status: record.as_ref().and_then(|r| r.provider_status),
            valid_function_arguments: valid,
            provider_error: record.as_ref().map(|r| meter.diagnostic(r.id)).transpose()?.flatten(),
            produced_function_call: answer.as_ref().is_some_and(|v| v.produced_function_call),
            output_tokens: record.as_ref().and_then(|v| v.output),
            reasoning_tokens: record.as_ref().and_then(|v| v.reasoning_tokens),
            incomplete_reason: answer.and_then(|v| v.incomplete_reason),
            outcome: if ledger.stop_reason()?.is_some()
                || ledger.task_stop_reason(meter.task_key())?.is_some()
            {
                "stopped_pending_accounting"
            } else if valid == Some(true) {
                "accepted_valid"
            } else {
                "refused_or_invalid"
            }
            .into(),
        });
    }
    Ok(results)
}
