//! Agent Web application. Receives a reserved Core dispatch, never a Main
//! network grant or a model-supplied channel/identity. R08 invokes this port.
mod download;
mod execution;
mod http;
mod inspection;
mod package;
mod proxy;
mod rendering;
mod transport;
mod web_actions;
pub(crate) mod web_service;
use crate::activity::{ActivityCapability, AgentActivities};
use crate::agent_store::{AgentStore, StoreAccess, StoreStep};
use anyhow::{Result, ensure};
use aura_domain_common::{AgentRunId, ArtifactId, ContentHash, SessionId, UtcTimestamp};
use aura_execution_contract::{
    AuthorizedDispatch, DispatchRequest, OperationNetwork, SecuredOperation,
};
use aura_network_control::{CoreHttpClientGateway, GatewayRequest};
use aura_security_contract::{
    InvocationActor, ResolvedCapability, ResourceDescriptor, SecuritySourceRef,
};
use aura_web_domain::*;
use aura_web_runtime::{DOCUMENT_MEDIA_TYPE, WebError};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::watch;

/// Cheap to clone: every field is an `Arc` or a handle, which is what lets one synchronous
/// section own exactly the parts it needs without borrowing the caller.
#[derive(Clone)]
pub(crate) struct ControlledWeb {
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
    journal: Arc<crate::broker::BrokerJournal>,
    gateway: Arc<CoreHttpClientGateway>,
    activities: AgentActivities,
    /// Hop route for the operation that is about to be receipted. Absent when the call never
    /// opened a network hop, including web.read.
    network_routes:
        std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, serde_json::Value>>>,
    /// The one bounded owner of Agent Store access.
    ///
    /// This is the **same** `Arc<AgentStore>` the application and execution layers hold -
    /// one pool, one admission capacity, one receipt owner for the whole process, over the
    /// same `BrokerJournal` - so it is not a second source of state truth. The web steps
    /// below take the journal lock inside helpers (`document`, `inspect`, the artifact
    /// publishes, `gateway_get`'s grant) with no `journal.lock()` literal at the call site;
    /// they run their real synchronous section on this owner instead of on the reactive
    /// task that drives the Agent's web future.
    store: Arc<AgentStore>,
    /// The DeepSeek service the settings choose, set once the model services are open.
    web_service: Arc<std::sync::OnceLock<Arc<dyn web_service::WebServiceSource>>>,
}
impl ControlledWeb {
    pub fn new(
        persistence: Arc<aura_persistence::PersistenceCoordinator>,
        journal: Arc<crate::broker::BrokerJournal>,
        gateway: Arc<CoreHttpClientGateway>,
        activities: AgentActivities,
        store: Arc<AgentStore>,
    ) -> Self {
        Self {
            persistence,
            journal,
            gateway,
            activities,
            network_routes: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::BTreeMap::new(),
            )),
            store,
            web_service: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// One real synchronous section of this application, run whole on the bounded owner.
    ///
    /// The section is handed over **whole**: its lock order (permission authority, then the
    /// journal) and its indivisibility are unchanged, and it is still the same journal over
    /// the same database. Nothing is re-implemented here and no `await` moves inside.
    async fn section<T, F>(&self, step: StoreStep, access: StoreAccess, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        self.store.run_critical(step, access, work).await
    }

    /// [`Self::inspect`] on the bounded path, for a caller that is an async backend future.
    ///
    /// `inspect` reads the live mode, the workspace projection and the stored web
    /// document through the journal, so an async entry point reaches it only through this
    /// wrapper.
    pub async fn inspect_section(
        &self,
        run: AgentRunId,
        operation: &SecuredOperation,
    ) -> Result<serde_json::Value> {
        let web = self.clone();
        let operation = operation.clone();
        self.section(StoreStep::run(run, "agent.web.inspect"), StoreAccess::Read, move || {
            web.inspect(&operation)
        })
        .await
    }
}

/// The section identity of one web step: the run when the call carries one.
///
/// A web call that reached this application already proved it has an agent run
/// (`web.agent_authority_required`), so the `None` arm is the honest identity for a step
/// reached before that proof rather than a guessed run.
fn store_step(run: Option<AgentRunId>, operation: &'static str) -> StoreStep {
    match run {
        Some(run) => StoreStep::run(run, operation),
        None => StoreStep::operation(operation),
    }
}

struct FetchedPage<'a> {
    dispatch: &'a AuthorizedDispatch,
    url: &'a url::Url,
    status: u16,
    content_type: &'a str,
    body: &'a [u8],
    max_bytes: u32,
}

struct InvokeFinish {
    value: Result<serde_json::Value>,
    cancelled: bool,
    stop_confirmed: bool,
}

impl InvokeFinish {
    fn cancelled(stop_confirmed: bool) -> Self {
        Self { value: Err(anyhow::anyhow!("web.cancelled")), cancelled: true, stop_confirmed }
    }
}

struct RenderedDocument {
    document: Result<WebDocument>,
    stop_confirmed: bool,
}

enum OrCancel<T> {
    Cancelled { stop_confirmed: bool },
    Value(T),
}

struct RenderWait {
    result: Result<aura_windows_platform::RenderedHtml, WebError>,
    cancelled: bool,
    stop_confirmed: bool,
}

fn document_from_render_wait(
    wait: RenderWait,
    convert: impl FnOnce(&aura_windows_platform::RenderedHtml) -> Result<WebDocument>,
) -> OrCancel<RenderedDocument> {
    if wait.cancelled {
        return OrCancel::Cancelled { stop_confirmed: wait.stop_confirmed };
    }
    match wait.result {
        Ok(rendered) => OrCancel::Value(RenderedDocument {
            document: convert(&rendered),
            stop_confirmed: wait.stop_confirmed,
        }),
        Err(error) => OrCancel::Value(RenderedDocument {
            document: Err(error.into()),
            stop_confirmed: wait.stop_confirmed,
        }),
    }
}

#[cfg(test)]
fn invoke_finish_from_fetched(fetched: OrCancel<RenderedDocument>) -> InvokeFinish {
    match fetched {
        OrCancel::Cancelled { stop_confirmed } => InvokeFinish::cancelled(stop_confirmed),
        OrCancel::Value(RenderedDocument { document, stop_confirmed }) => InvokeFinish {
            value: document.map(|_| serde_json::Value::Null),
            cancelled: false,
            stop_confirmed,
        },
    }
}

#[cfg(test)]
use execution::{persist_web_execution, web_result_states};

async fn or_cancel<T>(
    cancel: &mut watch::Receiver<bool>,
    future: impl Future<Output = T>,
) -> OrCancel<T> {
    if *cancel.borrow() {
        return OrCancel::Cancelled { stop_confirmed: true };
    }
    tokio::select! {biased;
        _ = cancel.changed() => OrCancel::Cancelled { stop_confirmed: true },
        value = future => OrCancel::Value(value),
    }
}

async fn await_render_session(
    session: aura_windows_platform::HeadlessRenderSession,
    cancel: watch::Receiver<bool>,
    timeout: Duration,
) -> RenderWait {
    await_render_session_with_reap(session, cancel, timeout, Duration::from_secs(5)).await
}

async fn await_render_session_with_reap(
    session: aura_windows_platform::HeadlessRenderSession,
    mut cancel: watch::Receiver<bool>,
    timeout: Duration,
    output_timeout: Duration,
) -> RenderWait {
    let abort = match session.abort_handle() {
        Ok(abort) => abort,
        Err(error) => {
            return RenderWait {
                result: Err(map_render_error(error)),
                cancelled: *cancel.borrow(),
                stop_confirmed: false,
            };
        }
    };
    let finish = tokio::task::spawn_blocking(move || session.finish(timeout));
    tokio::pin!(finish);
    if *cancel.borrow() {
        return reap_cancelled_render(abort, finish, output_timeout).await;
    }
    tokio::select! {biased;
        _ = cancel.changed() => reap_cancelled_render(abort, finish, output_timeout).await,
        joined = &mut finish => match joined {
            Ok(outcome) => {
                let stop_confirmed = outcome.stop_confirmed();
                RenderWait {
                    result: outcome.rendered.map_err(map_render_error),
                    cancelled: false,
                    stop_confirmed,
                }
            }
            Err(_) => RenderWait {
                result: Err(WebError::RenderingUnavailable),
                cancelled: false,
                stop_confirmed: abort.job_is_empty().unwrap_or(false),
            },
        }
    }
}

async fn reap_cancelled_render(
    abort: aura_windows_platform::RenderAbort,
    finish: impl Future<Output = Result<aura_windows_platform::HeadlessFinish, tokio::task::JoinError>>,
    output_timeout: Duration,
) -> RenderWait {
    let job_empty = abort.terminate_and_confirm(Duration::from_secs(5)).unwrap_or(false);
    let output_reaped = match tokio::time::timeout(output_timeout, finish).await {
        Ok(Ok(done)) => done.output_reaped && done.job_empty,
        Ok(Err(_)) | Err(_) => false,
    };
    RenderWait {
        result: Err(WebError::RenderingUnavailable),
        cancelled: true,
        stop_confirmed: job_empty && output_reaped && abort.job_is_empty().unwrap_or(false),
    }
}

fn map_render_error(error: aura_windows_platform::HeadlessRenderError) -> WebError {
    match error {
        aura_windows_platform::HeadlessRenderError::ResponseTooLarge => WebError::ResponseTooLarge,
        aura_windows_platform::HeadlessRenderError::Unavailable => WebError::RenderingUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::SecurityOperationId;
    use aura_execution_contract::{ExecutionOutcome, TerminationEvidence};
    use aura_security_contract::PermissionMode;

    #[test]
    fn oversized_render_stays_response_too_large() {
        assert!(matches!(
            map_render_error(aura_windows_platform::HeadlessRenderError::ResponseTooLarge),
            WebError::ResponseTooLarge
        ));
    }

    #[test]
    fn cancel_without_job_empty_is_not_confirmed() {
        let (outcome, confirmed) = web_result_states(true, false, false);
        assert_eq!(outcome, ExecutionOutcome::Unknown);
        assert!(!confirmed);
        let (outcome, confirmed) = web_result_states(true, true, false);
        assert_eq!(outcome, ExecutionOutcome::Cancelled);
        assert!(confirmed);
    }

    #[tokio::test]
    async fn core_render_cancel_confirms_only_after_the_job_is_empty() {
        let html = "<!doctype html><html><body><div id=\"app\">static</div><script>document.getElementById('app').textContent=String.fromCharCode(74,83,45,79,75,45,57);</script></body></html>";
        let (_, open) = watch::channel(false);
        let session = aura_windows_platform::start_headless_render_cancellable(
            aura_windows_platform::HeadlessPage { html: html.into(), scripts: Vec::new() },
            open,
        )
        .await
        .expect("start core render")
        .expect("not cancelled before start");
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let abort = session.abort_handle().expect("abort");
        let work = tokio::spawn(await_render_session(session, cancel_rx, Duration::from_secs(25)));
        tokio::time::sleep(Duration::from_millis(150)).await;
        cancel_tx.send(true).expect("cancel");
        let wait = work.await.expect("join");
        assert!(wait.cancelled);
        assert!(wait.stop_confirmed, "Confirmed must wait until the render Job is empty");
        assert!(abort.job_is_empty().expect("empty"));
        assert!(matches!(wait.result, Err(WebError::RenderingUnavailable)));
        let result = persist_web_execution(
            wait.cancelled,
            wait.stop_confirmed,
            false,
            dummy_attempt(),
            ArtifactId::new(),
            UtcTimestamp::now(),
            None,
        );
        assert_eq!(result.outcome, ExecutionOutcome::Cancelled);
        assert!(matches!(result.termination, TerminationEvidence::Confirmed { .. }));
        result.validate().expect("Cancelled requires Confirmed");
    }

    #[tokio::test]
    async fn running_render_retains_its_original_operation_authority() {
        let registry = crate::activity::AgentActivities::default();
        let session = SessionId::new();
        let html = "<!doctype html><html><body><div id=\"app\">static</div><script>document.getElementById('app').textContent='x';</script></body></html>";
        let (_, open) = watch::channel(false);
        let render = aura_windows_platform::start_headless_render_cancellable(
            aura_windows_platform::HeadlessPage { html: html.into(), scripts: Vec::new() },
            open,
        )
        .await
        .expect("start tighten render")
        .expect("not cancelled before start");
        let abort = render.abort_handle().expect("abort");
        let guard = registry
            .register(
                aura_domain_common::SecurityOperationId::new(),
                session,
                crate::activity::ActivityCapability::ControlledWeb,
                PermissionMode::FullAccess,
            )
            .expect("register");
        let work = tokio::spawn(await_render_session(
            render,
            guard.cancel.clone(),
            Duration::from_secs(25),
        ));
        let approved = registry
            .register(
                SecurityOperationId::new(),
                session,
                crate::activity::ActivityCapability::ControlledWeb,
                PermissionMode::ReadOnly,
            )
            .expect("S07 dispatch authority may approve a ReadOnly request");
        assert!(!*approved.cancel.borrow());
        assert!(!*guard.cancel.borrow());
        let wait = work.await.expect("render join");
        assert!(!wait.cancelled);
        assert!(wait.stop_confirmed);
        assert!(abort.job_is_empty().expect("empty"));
        drop(approved);
        drop(guard);
    }

    fn dummy_attempt() -> aura_execution_contract::AttemptRef {
        aura_execution_contract::AttemptRef {
            operation_id: aura_domain_common::SecurityOperationId::new(),
            ordinal: std::num::NonZeroU32::MIN,
        }
    }

    #[test]
    fn unconfirmed_render_error_is_persisted_unknown_not_confirmed() {
        let wait = RenderWait {
            result: Err(WebError::RenderingUnavailable),
            cancelled: false,
            stop_confirmed: false,
        };
        let fetched = document_from_render_wait(wait, |_| unreachable!("error path"));
        let finish = invoke_finish_from_fetched(fetched);
        assert!(!finish.cancelled);
        assert!(!finish.stop_confirmed);
        assert!(finish.value.is_err());
        let result = persist_web_execution(
            finish.cancelled,
            finish.stop_confirmed,
            finish.value.is_ok(),
            dummy_attempt(),
            ArtifactId::new(),
            UtcTimestamp::now(),
            None,
        );
        assert_eq!(result.outcome, ExecutionOutcome::Unknown);
        assert!(matches!(result.termination, TerminationEvidence::Unknown));
        result.validate().expect("Unknown stop stays Unknown");
    }

    #[test]
    fn confirmed_failed_render_stays_failed_with_confirmed_stop() {
        let wait = RenderWait {
            result: Err(WebError::RenderingUnavailable),
            cancelled: false,
            stop_confirmed: true,
        };
        let fetched = document_from_render_wait(wait, |_| unreachable!("error path"));
        let finish = invoke_finish_from_fetched(fetched);
        let result = persist_web_execution(
            finish.cancelled,
            finish.stop_confirmed,
            finish.value.is_ok(),
            dummy_attempt(),
            ArtifactId::new(),
            UtcTimestamp::now(),
            None,
        );
        assert_eq!(result.outcome, ExecutionOutcome::Failed);
        assert!(matches!(result.termination, TerminationEvidence::Confirmed { .. }));
        result.validate().expect("Failed+Confirmed is valid");
    }

    #[tokio::test]
    async fn cancel_output_reap_timeout_is_not_confirmed() {
        let finish = std::future::pending::<
            Result<aura_windows_platform::HeadlessFinish, tokio::task::JoinError>,
        >();
        tokio::pin!(finish);
        let output_reaped = match tokio::time::timeout(Duration::from_millis(0), finish).await {
            Ok(Ok(done)) => done.output_reaped && done.job_empty,
            Ok(Err(_)) | Err(_) => false,
        };
        assert!(
            !output_reaped,
            "an unfinished render future must stay unreaped even when Timeout polls it first"
        );
        let wait = RenderWait {
            result: Err(WebError::RenderingUnavailable),
            cancelled: true,
            stop_confirmed: false,
        };
        let finish =
            invoke_finish_from_fetched(document_from_render_wait(wait, |_| unreachable!()));
        assert!(!finish.stop_confirmed);
        let result = persist_web_execution(
            finish.cancelled,
            finish.stop_confirmed,
            false,
            dummy_attempt(),
            ArtifactId::new(),
            UtcTimestamp::now(),
            None,
        );
        assert_eq!(result.outcome, ExecutionOutcome::Unknown);
        assert!(matches!(result.termination, TerminationEvidence::Unknown));
        result.validate().expect("unconfirmed cancel is Unknown");
    }

    #[tokio::test]
    async fn already_reaped_finish_confirms_even_with_zero_timeout() {
        let done = aura_windows_platform::HeadlessFinish {
            rendered: Err(aura_windows_platform::HeadlessRenderError::Unavailable),
            job_empty: true,
            output_reaped: true,
            outcome: aura_windows_platform::RenderOutcome::EmptyDom,
        };
        let finish = std::future::ready(Ok::<_, tokio::task::JoinError>(done));
        tokio::pin!(finish);
        let output_reaped = match tokio::time::timeout(Duration::from_millis(0), finish).await {
            Ok(Ok(done)) => done.output_reaped && done.job_empty,
            Ok(Err(_)) | Err(_) => false,
        };
        assert!(
            output_reaped,
            "a ready reaped finish must stay Confirmed; Timeout must not force Unknown"
        );
        let result = persist_web_execution(
            true,
            true,
            false,
            dummy_attempt(),
            ArtifactId::new(),
            UtcTimestamp::now(),
            None,
        );
        assert_eq!(result.outcome, ExecutionOutcome::Cancelled);
        assert!(matches!(result.termination, TerminationEvidence::Confirmed { .. }));
        result.validate().expect("reaped cancel is Cancelled+Confirmed");
    }

    fn computed_page() -> aura_windows_platform::HeadlessPage {
        aura_windows_platform::HeadlessPage {
            html: "<!doctype html><html><body><div id=\"app\">static</div><script>document.getElementById('app').textContent='x';</script></body></html>".into(),
            scripts: Vec::new(),
        }
    }

    #[tokio::test]
    async fn queued_render_cancel_does_not_start_a_process() {
        let (_, open) = watch::channel(false);
        let first = aura_windows_platform::start_headless_render_cancellable(computed_page(), open)
            .await
            .expect("first start")
            .expect("first started");
        assert!(first.active_processes().expect("count") >= 1);
        let (tx, rx) = watch::channel(false);
        let second = tokio::spawn(async move {
            aura_windows_platform::start_headless_render_cancellable(computed_page(), rx).await
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        tx.send(true).expect("cancel queued");
        let queued = tokio::time::timeout(Duration::from_secs(2), second)
            .await
            .expect("queued cancel must finish bounded")
            .expect("join");
        match queued {
            Ok(None) => {}
            Ok(Some(_)) => panic!("queued cancel started a process"),
            Err(error) => panic!("queued cancel start failed: {error:?}"),
        }
        assert!(first.active_processes().expect("still first") >= 1);
        drop(first);
    }

    #[tokio::test]
    async fn queued_render_manual_stop_does_not_start_a_process() {
        let (_, open) = watch::channel(false);
        let first = aura_windows_platform::start_headless_render_cancellable(computed_page(), open)
            .await
            .expect("first start")
            .expect("first started");
        let registry = crate::activity::AgentActivities::default();
        let session = SessionId::new();
        let operation = SecurityOperationId::new();
        let guard = registry
            .register(
                operation,
                session,
                crate::activity::ActivityCapability::ControlledWeb,
                PermissionMode::FullAccess,
            )
            .expect("register");
        let cancel = guard.cancel.clone();
        let second = tokio::spawn(async move {
            aura_windows_platform::start_headless_render_cancellable(computed_page(), cancel).await
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        registry.cancel(operation).expect("manual stop");
        let queued = tokio::time::timeout(Duration::from_secs(2), second)
            .await
            .expect("queued stop must finish bounded")
            .expect("join");
        assert!(
            matches!(queued, Ok(None)),
            "manual stop of queued render must not start a process"
        );
        drop(guard);
        drop(first);
    }
}
