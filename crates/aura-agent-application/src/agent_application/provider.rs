//! The Model channel is derived from the actual configured Endpoint and the
//! prepared context. Tool mode and model-supplied channel flags are absent.
use super::*;
use aura_context_domain::*;
use aura_context_runtime::*;
use aura_core_broker::agent_store;
use aura_model_service::{model_channel, p05::P05Services};
use aura_model_transport::*;
use aura_persistence_v2::agent::stream;
use aura_security_contract::*;
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct Request {
    pub p05: P05Services,
    pub endpoint: ModelEndpointConfig,
    /// The one Store owner, so the read this authorizer performs runs on the same bounded section
    /// as every other Agent step instead of on whatever task the transport drives it from.
    pub store: Arc<agent_store::AgentStore>,
    pub identity: InvocationIdentity,
    pub logical: LogicalModelRequest,
    pub snapshot: ModelExecutionSnapshot,
    pub manifest: ModelEgressManifestV1,
    /// The bytes of the request's attachments (the browser's screenshots), in its order.
    pub attachments: Arc<[aura_model_domain::ResolvedModelAttachment]>,
    /// Another of the provider's connections is still untried in this run, so a temporary
    /// failure goes there instead of being retried on this one.
    pub backup_waiting: bool,
}
impl Request {
    fn endpoint(&self) -> anyhow::Result<ModelEndpointConfig> {
        let endpoint = self.endpoint.clone();
        anyhow::ensure!(
            endpoint.id == self.snapshot.endpoint_id
                && aura_model_config::endpoint_fingerprint(&endpoint)?
                    == self.snapshot.endpoint_fingerprint,
            "model.endpoint_snapshot_mismatch"
        );
        let binding = self.p05.configuration_credential_preflight(&endpoint)?.credential_binding;
        anyhow::ensure!(
            binding.as_ref().map(|v| (
                v.credential_id,
                v.credential_revision,
                &v.credential_binding_fingerprint
            )) == self
                .snapshot
                .credential_id
                .zip(self.snapshot.credential_revision)
                .zip(self.snapshot.credential_binding_fingerprint.as_ref())
                .map(|((id, revision), hash)| (id, revision, hash)),
            "model.credential_changed"
        );
        Ok(endpoint)
    }
}
impl ProviderRequestFactory for Request {
    fn build_request(
        &self,
        operation: &TransportOperationContext,
        removed: &BTreeSet<String>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        let input = ModelRequestCompilationInput {
            request: &self.logical,
            snapshot: &self.snapshot,
            resolved_attachments: &self.attachments,
            operation_context: operation,
            compatibility_removals: removed,
            applied_output_token_field_alias: None,
        };
        match self.snapshot.api_format {
            ModelApiFormat::OpenAiChatCompletions => {
                aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter.compile_request(input)
            }
            ModelApiFormat::OpenAiResponses => {
                aura_model_adapter_openai_responses::OpenAiResponsesAdapter.compile_request(input)
            }
            ModelApiFormat::AnthropicMessages => {
                aura_model_adapter_anthropic::AnthropicMessagesAdapter.compile_request(input)
            }
        }
    }
    fn current_endpoint_fingerprint(&self) -> Result<ContentHash, ModelTransportError> {
        aura_model_config::endpoint_fingerprint(&self.endpoint().map_err(error)?).map_err(error)
    }
}
struct Facts<'a> {
    request: &'a Request,
}
impl ModelEgressAuthorizationFacts for Facts<'_> {
    fn session_model_channel_is_authorized(
        &self,
        session: SessionId,
        profile: ModelProfileId,
    ) -> bool {
        self.request.identity.session_id == Some(session)
            && profile == self.request.snapshot.model_profile_id
    }
    async fn persisted_event(
        &self,
        id: EventId,
    ) -> Result<Option<PersistedContextEventAuthorizationFact>, ModelEgressAuthorizationError> {
        // This fact is a real journal read on the egress verification chain, and the chain
        // is now awaitable from `authorize_model_request` down. It runs on the one Store owner's
        // bounded section, so a slow or unreachable journal is answered as a store fact
        // (`agent.store_refused` / `agent.store_unknown` / `agent.store_unavailable`) inside the
        // budget instead of parking a reactor worker on the shared journal mutex.
        let request = self.request;
        let session =
            request.identity.session_id.ok_or(ModelEgressAuthorizationError::FactStore)?;
        let step = match request.identity.agent_run_id {
            Some(run) => StoreStep::run(run, "agent.provider.persisted_event"),
            None => StoreStep::operation("agent.provider.persisted_event"),
        };
        // A user message a branch carried is checked against the text it carried.
        let user = request
            .store
            .read(step, move |db| {
                aura_persistence_v2::agent::model_context::egress_user_text(db, session, id)
            })
            .await
            .map_err(|_| ModelEgressAuthorizationError::FactStore)?;
        Ok(user.map(|text| PersistedContextEventAuthorizationFact {
            event_id: id,
            session_id: session,
            actor: PersistedContextEventActor::User,
            content_hash: ContentHash::sha256(text.as_bytes()),
            byte_length: text.len() as u64,
            model_visible: true,
            explicitly_user_authored: true,
        }))
    }
    fn source_grant_binding_is_valid(
        &self,
        _: SessionId,
        _: &ContextSourceRef,
        _: ModelEgressSourceKind,
        _: NetworkDataClass,
        _: &SourceAuthorizationBinding,
    ) -> Result<bool, ModelEgressAuthorizationError> {
        // External attachments and Browser shares are not acquired by this
        // minimal Core path. Never silently bless an unimplemented source.
        Ok(false)
    }
    /// A picture the user sent with a message of this conversation.
    async fn user_attachment(
        &self,
        session: SessionId,
        artifact: ArtifactId,
    ) -> Result<Option<(ContentHash, u64)>, ModelEgressAuthorizationError> {
        let request = self.request;
        if request.identity.session_id != Some(session) {
            return Ok(None);
        }
        let step = match request.identity.agent_run_id {
            Some(run) => StoreStep::run(run, "agent.provider.user_attachment"),
            None => StoreStep::operation("agent.provider.user_attachment"),
        };
        request
            .store
            .read(step, move |db| {
                aura_persistence_v2::agent::attachments::sent_by_user(db, session, artifact)
            })
            .await
            .map_err(|_| ModelEgressAuthorizationError::FactStore)
    }
}
impl ModelRequestAuthorizer for Request {
    async fn authorize_model_request(
        &self,
        snapshot: &ModelExecutionSnapshot,
        plan: &ProviderRequestPlan,
        attempt: TransportAttemptId,
        request_id: NetworkRequestId,
    ) -> Result<aura_network_control::ProviderTransportGatewayRequest, ModelTransportError> {
        if *snapshot != self.snapshot {
            return Err(error("model.snapshot_changed"));
        }
        // Boundary: this read is the authorizer's own pre-flight and this method is already
        // `async`, so it runs on the one Store owner's bounded section like every other Agent step.
        // A store that cannot serve it inside the budget answers `agent.store_refused` /
        // `agent.store_unknown` / `agent.store_unavailable`, and the transport reports that as the
        // model step's failure - it is never answered from a direct lock on the shared journal.
        {
            let step = match self.identity.agent_run_id {
                Some(run) => StoreStep::run(run, "agent.provider.owning_step"),
                None => StoreStep::operation("agent.provider.owning_step"),
            };
            let model_step_id = snapshot.model_step_id;
            self.store
                .read(step, move |db| {
                    aura_persistence_v2::model::agent_transport::owning_step(db, model_step_id)
                        .map(|_| ())
                })
                .await
                .map_err(error)?;
        }
        model_channel::authorize_request(
            &self.identity,
            &self.endpoint().map_err(error)?,
            snapshot,
            &self.manifest,
            &Facts { request: self },
            plan,
            attempt,
            request_id,
            UtcTimestamp::now(),
        )
        .await
        .map_err(error)
    }
}
fn error(value: impl std::fmt::Display) -> ModelTransportError {
    ModelTransportError::Store(value.to_string())
}

pub(super) async fn execute(
    app: &AgentApplication,
    request: Request,
    cancel: &ModelCancellationToken,
) -> anyhow::Result<TransportRunTerminal> {
    let endpoint = request.endpoint()?;
    let gate = app.gates.register_or_update(endpoint.id, endpoint.max_parallel_model_requests)?;
    let snapshot = request.snapshot.clone();
    let model_step = snapshot.model_step_id;
    // The step's live tool input is forgotten when this stream ends, however it ends.
    let _live = super::tool_input::Forget(model_step);
    let maintenance = app
        .store
        .read(StoreStep::operation("memory.model.priority"), move |db| {
            aura_persistence_v2::model::memory_maintenance::binding(db, model_step)
                .map(|b| b.is_some())
        })
        .await?;
    let stream_run = request.identity.agent_run_id;
    let store = super::model_store::AgentModelStore {
        store: Arc::clone(&app.store),
        run: stream_run,
        snapshot: snapshot.clone(),
        hold_completion: app.hold_completion(),
    };
    // One shared gateway retains the normal persistent Grant/Credential boundary.
    let factory = request.clone();
    let network = CoreGatewayModelNetwork::new(app.p05.model_gateway.clone(), request);
    // The compiler and authorizer share the same frozen request, never a model
    // supplied network mode. A second immutable value is not a second authority.
    let (events, mut receive) = TransportEventChannel::bounded(128);
    let policy = TransportRetryPolicy::new(
        std::time::Duration::from_millis(250),
        std::time::Duration::from_secs(2),
        std::time::Duration::from_secs(30),
    );
    let transport = ModelTransport::new(
        network,
        store,
        if factory.backup_waiting { policy.without_temporary_retries() } else { policy },
        SseLimits::default(),
    );
    let decoder: &dyn ModelProtocolDecoder = match snapshot.api_format {
        ModelApiFormat::OpenAiChatCompletions => {
            &aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter
        }
        ModelApiFormat::OpenAiResponses => {
            &aura_model_adapter_openai_responses::OpenAiResponsesAdapter
        }
        ModelApiFormat::AnthropicMessages => {
            &aura_model_adapter_anthropic::AnthropicMessagesAdapter
        }
    };
    let execution = transport.execute(ModelTransportExecution {
        snapshot: &snapshot,
        endpoint_timeouts: &endpoint.timeouts,
        gate: &gate,
        priority: if maintenance {
            ModelRequestPriority::Maintenance
        } else {
            ModelRequestPriority::InteractiveParent
        },
        request_factory: &factory,
        adapter: decoder,
        initial_compatibility_overrides: BTreeSet::new(),
        cancellation: cancel,
        events: &events,
    });
    tokio::pin!(execution);
    // The stream loop's event transaction is a real journal write, and it used to run
    // inline on this async task: a Provider that answered quickly could still park the reactor on
    // the shared journal mutex for as long as another writer held it. It is now one admitted,
    // receipted section on the same bounded Store owner as every other Agent step. A refusal means
    // the event provably was not persisted; an unknown answer means it may still commit under its
    // own permit. The two are neither merged nor retried.
    let stream_step = match stream_run {
        Some(run) => StoreStep::run(run, "agent.provider.stream_event"),
        None => StoreStep::operation("agent.provider.stream_event"),
    };
    // What arrives within `stream::BATCH` of the first event is stored in one transaction; a call
    // that ends stores what was still held first.
    let (mut batch, mut due) = (Vec::new(), None);
    loop {
        tokio::select! {
            result=&mut execution=>{
                store_stream(app, stream_step, batch).await?;
                return Ok(result?);
            }
            event=receive.recv()=>{
                let event = event.ok_or_else(|| anyhow::anyhow!("model.event_channel_closed"))?;
                // A tool call's growing input is only counted; nothing of it is stored.
                if !super::tool_input::observe(&event) {
                    batch.push(event);
                    due = due.or_else(|| Some(tokio::time::Instant::now() + stream::BATCH));
                }
            }
            () = tokio::time::sleep_until(due.unwrap_or_else(tokio::time::Instant::now)),
                if due.is_some() => {
                due = None;
                store_stream(app, stream_step, std::mem::take(&mut batch)).await?;
            }
        }
    }
}

async fn store_stream(
    app: &AgentApplication,
    step: StoreStep,
    events: Vec<aura_model_domain::ModelStreamEventEnvelope>,
) -> anyhow::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    app.store
        .write(step, move |journal| {
            journal.database.unit_of_work(|tx| stream::append_all(tx, &events))
        })
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))
}
