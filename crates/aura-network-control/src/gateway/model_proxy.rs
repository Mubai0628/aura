//! Model requests leave through this gateway like every other Core request, and follow the way
//! out the user's system takes: the proxy switched on in Aura's settings, else the one the system
//! names, read again for every hop. Only an https request to a public address goes through it,
//! with the host name, so the proxy's own rules choose between its direct and proxied routes; a
//! proxy that cannot start the request leaves it to a direct attempt, as for tools.
use super::*;
use aura_tool_contract::ToolNetworkProxyV1;
use std::pin::Pin;

/// Where a model request's proxy comes from.
pub trait ModelProxySource: Send + Sync {
    fn model_proxy(&self) -> Option<ToolNetworkProxyV1>;
}

impl<F> ModelProxySource for F
where
    F: Fn() -> Option<ToolNetworkProxyV1> + Send + Sync,
{
    fn model_proxy(&self) -> Option<ToolNetworkProxyV1> {
        self()
    }
}

/// A model request without a total limit (a streamed answer) keeps a proxied connection at most
/// this long; its own idle limits end it sooner.
const UNLIMITED_MODEL_REQUEST: Duration = Duration::from_secs(24 * 60 * 60);

type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

impl CoreHttpClientGateway {
    #[must_use]
    pub fn with_model_proxy(mut self, source: Arc<dyn ModelProxySource>) -> Self {
        self.model_proxy = Some(source);
        self
    }

    /// One hop of a model request through the proxy the source names now, when it applies to
    /// `destination`; over the pinned `direct` client when the proxy proves it never started the
    /// request. None leaves the hop to the caller's own direct send.
    ///
    /// Model requests run deep inside a turn's futures, close to a thread's stack limit in debug
    /// builds, and each of those futures embeds the request's. So the caller holds nothing new
    /// across an await, and a plain function boxes the proxied hop: its future is built in this
    /// short frame, never in the caller's.
    pub(super) fn proxied_model_hop<'a>(
        &'a self,
        destination: &ResolvedNetworkDestination,
        request: &'a ProviderTransportImmutableRequest,
        method: &Method,
        headers: &HeaderMap,
        direct: &'a reqwest::Client,
    ) -> Option<Boxed<'a, Result<reqwest::Response, NetworkControlError>>> {
        let proxy = self.model_proxy.as_ref()?.model_proxy()?;
        super::user_proxy::select_proxy(true, Some(&proxy), destination)?;
        Some(Box::pin(self.model_hop(proxy, request, method.clone(), headers.clone(), direct)))
    }

    async fn model_hop(
        &self,
        proxy: ToolNetworkProxyV1,
        request: &ProviderTransportImmutableRequest,
        method: Method,
        headers: HeaderMap,
        direct: &reqwest::Client,
    ) -> Result<reqwest::Response, NetworkControlError> {
        let begun = tokio::time::Instant::now();
        super::user_proxy::check_proxy(&proxy)?;
        let total = request.timeout_ms.map_or(UNLIMITED_MODEL_REQUEST, Duration::from_millis);
        let client = super::user_proxy::ProxyClient::new(proxy, &self.options)?;
        let hop = super::user_proxy::ProxiedHop {
            url: &request.requested_url,
            method: method.clone(),
            headers: headers.clone(),
            body: request.body.clone(),
            connect: Duration::from_millis(request.connect_timeout_ms.clamp(1, 5_000)),
            deadline: begun + total,
        };
        match proxied(&client, hop).await {
            Ok(response) => return Ok(response),
            // Only a proxy that proved the request never started leaves it to a direct attempt;
            // a request that may have reached the server is never sent again.
            Err(super::user_proxy::HopSendError {
                error: NetworkControlError::UserProxyFailure(_),
                request_not_started: true,
            }) => {}
            Err(failure) => return Err(failure.error),
        }
        let send = direct
            .request(method, &request.requested_url)
            .headers(headers)
            .body(request.body.clone())
            .send();
        let sent = match request.timeout_ms {
            // The direct attempt keeps within what is left of the request's limit.
            Some(ms) => {
                let left = Duration::from_millis(ms).saturating_sub(begun.elapsed());
                if left.is_zero() {
                    return Err(NetworkControlError::Timeout);
                }
                tokio::time::timeout(left, send).await.map_err(|_| NetworkControlError::Timeout)?
            }
            None => send.await,
        };
        sent.map_err(map_reqwest_error)
    }
}

/// The proxy handshake and exchange, boxed by a plain function for the same reason.
fn proxied<'a>(
    client: &'a super::user_proxy::ProxyClient,
    hop: super::user_proxy::ProxiedHop<'a>,
) -> Boxed<'a, Result<reqwest::Response, super::user_proxy::HopSendError>> {
    Box::pin(client.send_hop(hop))
}
