use std::{future::Future, sync::Arc};

use aura_domain_common::{NetworkRequestId, TransportAttemptId};
use aura_model_domain::ModelExecutionSnapshot;
use aura_network_control::{
    CoreHttpClientGateway, GatewayBodyStream, GatewayStreamResponse,
    ProviderTransportGatewayRequest, ProviderTransportRequestAuthority,
};
use aura_security_contract::NetworkCallerKind;

use crate::{ModelTransportError, ProviderRequestPlan};

/// Frozen lifetime for an unconsumed per-attempt Core network grant.
///
/// Consuming the grant before this deadline authorizes the associated HTTP/SSE
/// transaction to continue to its normal terminal state.
pub const CORE_REQUEST_GRANT_TTL_SECONDS: i64 = 30;

pub struct TransportHttpResponse<B> {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: B,
}

pub trait TransportHttpBody: Send {
    fn next_chunk(
        &mut self,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, ModelTransportError>> + Send;
}

impl TransportHttpBody for GatewayBodyStream {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, ModelTransportError> {
        GatewayBodyStream::next_chunk(self).await.map_err(ModelTransportError::from)
    }
}

pub trait ModelNetworkExecutor: Send + Sync {
    type Body: TransportHttpBody;

    fn open(
        &self,
        snapshot: &ModelExecutionSnapshot,
        request_plan: &ProviderRequestPlan,
        attempt_id: TransportAttemptId,
        client_request_id: NetworkRequestId,
    ) -> impl Future<Output = Result<TransportHttpResponse<Self::Body>, ModelTransportError>> + Send;
}

pub trait ModelRequestAuthorizer: Send + Sync {
    fn authorize_model_request(
        &self,
        snapshot: &ModelExecutionSnapshot,
        request_plan: &ProviderRequestPlan,
        attempt_id: TransportAttemptId,
        client_request_id: NetworkRequestId,
    ) -> impl Future<Output = Result<ProviderTransportGatewayRequest, ModelTransportError>> + Send;
}

pub struct CoreGatewayModelNetwork<A> {
    gateway: Arc<CoreHttpClientGateway>,
    authorizer: A,
}

impl<A> CoreGatewayModelNetwork<A> {
    #[must_use]
    pub fn new(gateway: Arc<CoreHttpClientGateway>, authorizer: A) -> Self {
        Self { gateway, authorizer }
    }
}

impl<A> ModelNetworkExecutor for CoreGatewayModelNetwork<A>
where
    A: ModelRequestAuthorizer,
{
    type Body = GatewayBodyStream;

    async fn open(
        &self,
        snapshot: &ModelExecutionSnapshot,
        request_plan: &ProviderRequestPlan,
        attempt_id: TransportAttemptId,
        client_request_id: NetworkRequestId,
    ) -> Result<TransportHttpResponse<Self::Body>, ModelTransportError> {
        validate_provider_request_plan(snapshot, request_plan)?;
        let request = self
            .authorizer
            .authorize_model_request(snapshot, request_plan, attempt_id, client_request_id)
            .await?;
        validate_gateway_binding(snapshot, request_plan, client_request_id, &request)?;
        let GatewayStreamResponse { status, headers, body, .. } =
            self.gateway.execute_provider_transport_stream(request).await?;
        Ok(TransportHttpResponse { status, headers, body })
    }
}

fn validate_provider_request_plan(
    snapshot: &ModelExecutionSnapshot,
    request_plan: &ProviderRequestPlan,
) -> Result<(), ModelTransportError> {
    if request_plan.method != "POST"
        || request_plan.request_url != snapshot.resolved_request_url
        || request_plan.body.is_empty()
    {
        return Err(ModelTransportError::InvalidProviderRequest(
            "method, URL, or body does not match the frozen execution snapshot".to_owned(),
        ));
    }
    if request_plan.headers.iter().any(|(name, _)| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "authorization" | "proxy-authorization" | "x-api-key" | "cookie" | "set-cookie"
        )
    }) {
        return Err(ModelTransportError::InvalidProviderRequest(
            "provider request plan contains a credential-bearing header".to_owned(),
        ));
    }
    Ok(())
}

fn validate_gateway_binding(
    snapshot: &ModelExecutionSnapshot,
    request_plan: &ProviderRequestPlan,
    client_request_id: NetworkRequestId,
    request: &ProviderTransportGatewayRequest,
) -> Result<(), ModelTransportError> {
    if request.grant.logical_request_id != client_request_id
        || request.immutable_request.request_id != client_request_id
        || request.grant.caller != NetworkCallerKind::ModelTransport
        || request.immutable_request.method != request_plan.method
        || request.immutable_request.requested_url != request_plan.request_url
        || request.immutable_request.body != request_plan.body
        || !matches!(request.authority, ProviderTransportRequestAuthority::AgentModel(_))
        || request.grant.purpose.credential_binding.as_ref().map(|binding| {
            (
                binding.credential_id,
                binding.credential_revision,
                &binding.credential_binding_fingerprint,
            )
        }) != snapshot
            .credential_id
            .zip(snapshot.credential_revision)
            .zip(snapshot.credential_binding_fingerprint.as_ref())
            .map(|((credential_id, credential_revision), fingerprint)| {
                (credential_id, credential_revision, fingerprint)
            })
    {
        return Err(ModelTransportError::WrongNetworkBinding);
    }
    Ok(())
}
