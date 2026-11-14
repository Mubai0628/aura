//! The built-in browser's tools. Core resolves and authorizes each call, sends the browser
//! host (Electron Main) one command for the session's model page, and records what comes back.
//! The page itself, its input and its site policy live in the host.
mod bridge;
mod copies;
mod invoke;
mod local_path;
mod resolve;
#[cfg(test)]
mod tests;

use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_core::BrokerPort;
use aura_domain_common::jcs::canonical_sha256;
pub use bridge::BrowserBridge;
pub(in crate::agent_execution) use invoke::invoke;
pub(in crate::agent_execution) use resolve::admission_feedback;

pub(super) const BACKEND_ID: &str = "agent-browser-v1";

/// Text that is not Aura's own sentence (a library's error, a page's exception, an extension's
/// report, Windows) as a detail inside one: its first non-empty line, at most `limit`
/// characters, with control characters replaced and braces turned into parentheses, so that the
/// message the model reads stays one readable line. A trailing full stop is left to the caller.
pub(in crate::agent_execution) fn outside_detail(text: &str, limit: usize) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let mut detail: String = line
        .chars()
        .take(limit)
        .map(|c| match c {
            '{' => '(',
            '}' => ')',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    if line.chars().count() > limit {
        detail.push('…');
    }
    let detail = detail.trim_end_matches(['.', '。', ' ']);
    if detail.is_empty() { "没有给出原因".to_owned() } else { detail.to_owned() }
}

/// `text` closed as a sentence, so that another sentence can follow it directly.
pub(in crate::agent_execution) fn ended(text: &str) -> String {
    let text = text.trim_end();
    if text.is_empty() || text.ends_with(['。', '！', '？', '；', '.', '!', '?']) {
        text.to_owned()
    } else {
        format!("{text}。")
    }
}

/// The backend a browser call runs on: Core's broker, carrying the browser capabilities.
pub(super) struct BrowserBackend {
    descriptor: BackendDescriptor,
    port: broker::Port,
}

pub(super) fn backend(
    service: Arc<AgentExecution>,
    call: &CallContext,
) -> anyhow::Result<Arc<dyn ExecutionBackend>> {
    let mut descriptor = broker::descriptor(service.runtime);
    descriptor.id = BackendId(BACKEND_ID.into());
    descriptor.protocol_fingerprint = canonical_sha256(&"aura.agent-browser.v1")?;
    descriptor.capabilities = [
        ResolvedCapability::BrowserRead,
        ResolvedCapability::BrowserInteraction,
        ResolvedCapability::BrowserSensitiveAction,
        ResolvedCapability::WorkspaceFileRead,
    ]
    .into();
    Ok(Arc::new(BrowserBackend {
        descriptor,
        port: broker::Port { service, call: call.clone(), commands: None },
    }))
}

impl ExecutionBackend for BrowserBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            if request.operation.validate().is_err()
                || !self.descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(preparation(
                    "browser.unsupported_operation: 这次调用要的能力超出了内置浏览器能做的范围，没有开始，什么都没改动。",
                ));
            }
            let facts = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation: facts,
            })
            .map_err(|_| {
                preparation(
                    "browser.invalid_preparation: Aura 没能为这次浏览器调用做好准备，没有开始，什么都没改动；可以再调用一次。",
                )
            })
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request();
            if request.plan.validate().is_err()
                || request.plan.prepared.backend != self.descriptor
                || self.port.inspect(&request.operation).await? != request.plan.prepared.preparation
            {
                return Err(rejected(
                    "browser.preparation_changed: 这次浏览器调用准备好之后、开始之前，会话的权限或运行状态变了，所以没有开始，什么都没改动；还需要的话再调用一次。",
                ));
            }
            self.port.claim(&dispatch).await?;
            self.port.invoke(request).await.map_err(|_| BackendError::DispatchUnknown {
                attempt: request.binding.attempt.clone(),
            })?;
            Ok(ExecutionHandle {
                attempt: request.binding.attempt.clone(),
                dispatch_fingerprint: canonical_sha256(&request.binding).map_err(|_| {
                    BackendError::DispatchUnknown { attempt: request.binding.attempt.clone() }
                })?,
            })
        })
    }
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        self.port.cancel(handle)
    }
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        self.port.reconcile(attempt)
    }
}

impl AgentExecution {
    /// Closes the model page of `session`, when it opened one. Best effort: a sub-agent that ended
    /// leaves no page behind, and a page Main no longer has needs nothing.
    pub async fn close_model_page(&self, session: SessionId) {
        if self.browser.page_url(session, resolve::MODEL_PAGE).is_none() {
            return;
        }
        let action = aura_browser_domain::BrowserHostActionV1::Close;
        let command = aura_browser_domain::BrowserHostCommandV1 {
            command_id: String::new(),
            runtime_instance_id: self.runtime,
            session_id: session,
            page: None,
            expected_host: None,
            access: aura_browser_domain::BrowserPageAccessV1 {
                read_only: true,
                approved_hosts: vec![],
                file_root: None,
            },
            timeout_ms: resolve::timeout_ms(&action),
            action,
        };
        let (_keep, stop) = tokio::sync::watch::channel(false);
        let _ = self.browser.send(command, stop).await;
    }
}
