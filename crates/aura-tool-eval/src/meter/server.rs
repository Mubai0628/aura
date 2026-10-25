use super::{
    diagnostics::{ProviderError, provider_error},
    request_sequence::RequestSequence,
    usage::read_usage_for,
    wire,
};
use crate::{
    budget::Ledger,
    owner::{ApiFormat, OwnerTarget},
    replay::Replay,
};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

pub enum Upstream {
    Replay(Arc<Replay>),
    /// Only constructed after the owner approves an estimate. Secrets stay in memory.
    DeepSeek {
        target: Box<OwnerTarget>,
    },
    /// Deterministic local boundary tests, never an arbitrary remote URL.
    Local(String),
}
#[derive(Clone, Copy)]
pub enum MeterPurpose {
    Coding,
    Probe,
}
pub struct MeterConfig {
    pub task: String,
    pub output_cap: u64,
    pub ledger: Arc<Ledger>,
    pub upstream: Upstream,
    pub purpose: MeterPurpose,
    pub api_format: ApiFormat,
}
pub struct Meter {
    pub url: String,
    task: JoinHandle<()>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task_key: String,
    diagnostics: Arc<Mutex<BTreeMap<i64, ProviderError>>>,
    api_format: ApiFormat,
    output_cap: u64,
}
impl Drop for Meter {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Meter {
    pub async fn start(config: MeterConfig) -> Result<Self> {
        if let Upstream::Local(url) = &config.upstream {
            let url = reqwest::Url::parse(url)?;
            ensure!(
                url.scheme() == "http"
                    && url.host_str().is_some_and(|h| h == "127.0.0.1" || h == "::1"),
                "eval.local_service_only"
            );
        }
        ensure!(config.output_cap > 0, "eval.output_cap_missing");
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/v1", listener.local_addr()?);
        let task_key = config.task.clone();
        let api_format = config.api_format;
        let output_cap = config.output_cap;
        let config = Arc::new(config);
        let diagnostics = Arc::new(Mutex::new(BTreeMap::new()));
        let sequence = Arc::new(RequestSequence::default());
        let observed = Arc::clone(&diagnostics);
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                let accepted = tokio::select! {
                    accepted = listener.accept() => accepted,
                    _ = &mut stopped => break,
                };
                let Ok((socket, _)) = accepted else { break };
                let config = Arc::clone(&config);
                let diagnostics = Arc::clone(&observed);
                let sequence = Arc::clone(&sequence);
                children.spawn(async move {
                    if tokio::time::timeout(
                        std::time::Duration::from_secs(super::upstream::CONNECTION_TIMEOUT_SECONDS),
                        serve(socket, &config, &diagnostics, &sequence),
                    )
                    .await
                    .is_err()
                    {
                        config.ledger.halt();
                    }
                });
                while children.try_join_next().is_some() {}
            }
            drop(listener);
            // A request already sent upstream is billed even if Core stopped waiting for it;
            // each exchange is bounded by the connection timeout, so wait for its settlement.
            while children.join_next().await.is_some() {}
        });
        Ok(Self { url, task, stop: Some(stop), task_key, diagnostics, api_format, output_cap })
    }
    /// Stops accepting requests and waits until every received request is settled or marked
    /// unknown, so a report never leaves a reservation pending that the meter could still settle.
    pub async fn finish(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let bound =
            std::time::Duration::from_secs(super::upstream::CONNECTION_TIMEOUT_SECONDS + 10);
        tokio::time::timeout(bound, &mut self.task)
            .await
            .map_err(|_| anyhow::anyhow!("eval.meter_drain_timeout"))??;
        Ok(())
    }
    pub fn diagnostic(&self, id: i64) -> Result<Option<ProviderError>> {
        Ok(self
            .diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("eval.diagnostic_lock_failed"))?
            .get(&id)
            .cloned())
    }
    pub fn task_key(&self) -> &str {
        &self.task_key
    }
    pub fn api_format(&self) -> ApiFormat {
        self.api_format
    }
    pub fn output_cap(&self) -> u64 {
        self.output_cap
    }
}
async fn serve(
    mut socket: TcpStream,
    config: &MeterConfig,
    diagnostics: &Mutex<BTreeMap<i64, ProviderError>>,
    sequence: &RequestSequence,
) -> Result<()> {
    match exchange(&mut socket, config, diagnostics, sequence).await {
        Ok(()) => Ok(()),
        Err(error) => {
            if config.ledger.stop_reason()?.is_none()
                && config.ledger.task_stop_reason(&config.task)?.is_none()
            {
                let reason = if error.to_string() == "eval.output_limit_exceeds_approved_cap" {
                    "budget_exhausted"
                } else if error.to_string() == "eval.core_disconnected" {
                    "core_disconnected"
                } else {
                    "invalid_request"
                };
                config.ledger.stop_task(&config.task, reason)?;
            }
            // Never return upstream bodies, headers or error strings that might carry secrets.
            wire::respond(&mut socket,429,br#"{"error":{"code":"eval_budget_or_ledger_stopped","message":"Evaluation stopped before any further requests."}}"#,"application/json").await
        }
    }
}
async fn exchange(
    socket: &mut TcpStream,
    config: &MeterConfig,
    diagnostics: &Mutex<BTreeMap<i64, ProviderError>>,
    sequence: &RequestSequence,
) -> Result<()> {
    let (_, body) = wire::request(socket, config.api_format).await?;
    let mut request: Value = serde_json::from_slice(&body)?;
    let output = match config.api_format {
        ApiFormat::OpenAiResponses => request.get("max_output_tokens"),
        ApiFormat::OpenAiChatCompletions => {
            request.get("max_tokens").or_else(|| request.get("max_completion_tokens"))
        }
    }
    .and_then(Value::as_u64)
    .ok_or_else(|| anyhow::anyhow!("eval.output_limit_required"))?;
    ensure!(output > 0 && output <= config.output_cap, "eval.output_limit_exceeds_approved_cap");
    if let Upstream::DeepSeek { target } = &config.upstream {
        target.verify_current()?;
    }
    // At most one token per serialized UTF-8 byte, plus fixed protocol/model overhead.
    // This intentionally over-reserves; provider usage settles actual input including cache hits.
    let input_bound = u64::try_from(body.len())?
        .checked_add(4096)
        .ok_or_else(|| anyhow::anyhow!("eval.input_overflow"))?;
    let id = sequence.reserve(
        &config.ledger,
        &config.task,
        config.purpose,
        &body,
        input_bound,
        output,
    )?;
    config.ledger.request_body_bytes(id, u64::try_from(body.len())?)?;
    let definitions =
        request.get("tools").map(serde_json::to_vec).transpose()?.map_or(0, |v| v.len());
    config.ledger.tool_definition_bytes(id, u64::try_from(definitions)?)?;
    let result = super::upstream::send(&mut request, &config.upstream, config.api_format).await;
    let (status, response, content_type) = match result {
        Ok(value) => value,
        // Sent, then lost (timeout or connection): charged in full, and only this task stops.
        Err(error) => {
            config.ledger.charge_unconfirmed(id)?;
            return Err(error);
        }
    };
    // Numeric status only: diagnosis of provider schema refusal must not retain its body.
    config.ledger.provider_status(id, status)?;
    if matches!(config.purpose, MeterPurpose::Probe)
        && let Some(error) = provider_error(&response)
    {
        diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("eval.diagnostic_lock_failed"))?
            .insert(id, error);
    }
    if (400..500).contains(&status) && !response.is_empty() {
        config.ledger.charge_refusal(id, matches!(config.purpose, MeterPurpose::Coding))?;
        return wire::respond(socket,status,br#"{"error":{"code":"eval_provider_refused","message":"Provider refused this request; the reservation was charged."}}"#,"application/json").await.map_err(|_| anyhow::anyhow!("eval.core_disconnected"));
    }
    // A 5xx confirms no usage either: charged in full, and only this task stops. Any other
    // status outside 2xx (an empty refusal, a redirect), and a 2xx whose usage is missing or
    // contradicts the reservation, is a shape the meter cannot account for: every task halts.
    if (500..600).contains(&status) {
        config.ledger.charge_unconfirmed(id)?;
        anyhow::bail!("eval.provider_usage_unconfirmed");
    }
    if !(200..300).contains(&status) {
        config.ledger.unresolved(id)?;
        anyhow::bail!("eval.provider_usage_unknown");
    }
    let usage = match read_usage_for(config.api_format, &response) {
        Ok(usage) => usage,
        Err(error) => {
            config.ledger.unresolved(id)?;
            return Err(error);
        }
    };
    config.ledger.settle(id, usage)?;
    wire::respond(socket, status, &response, &content_type)
        .await
        .map_err(|_| anyhow::anyhow!("eval.core_disconnected"))
}
