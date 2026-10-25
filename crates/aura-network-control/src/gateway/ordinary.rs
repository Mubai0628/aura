use super::*;
use crate::ordinary_tools::OrdinaryToolRequestGrant;

pub struct OrdinaryGatewayResponse {
    pub response: GatewayResponse,
    pub final_url: String,
    pub redirects: Vec<String>,
    pub credential_reflection: Option<CredentialReflectionScanEvidenceV1>,
    pub routing: Vec<GatewayHopRouteV1>,
}

pub struct OrdinaryGatewayStream {
    pub response: GatewayStreamResponse,
    pub final_url: String,
    pub redirects: Vec<String>,
    pub routing: Vec<GatewayHopRouteV1>,
}

impl CoreHttpClientGateway {
    /// Runs before Attempt reservation; each redirect is checked again at its actual connect.
    pub async fn check_ordinary_target(&self, url: &str) -> Result<(), NetworkControlError> {
        let (_, _, destination) =
            crate::resolver::normalize_with_options(url, &self.options).await?;
        crate::ordinary_tools::check_destination(&destination)
    }

    pub async fn execute_ordinary(
        &self,
        grant: CoreRequestGrant<OrdinaryToolRequestGrant>,
        request: GatewayRequest,
        credentials: Option<&dyn ToolCredentialResolver>,
        started: impl FnOnce() -> GatewayStartedHook + Send,
    ) -> Result<OrdinaryGatewayResponse, NetworkControlError> {
        let opened =
            self.open_response(grant, request, Some(Box::new(started)), credentials).await?;
        let final_url = opened.final_url.clone();
        let redirects = opened.redirect_chain.clone();
        let routing = opened.routing.clone();
        let (response, credential_reflection) =
            self.collect_response(opened).await.map_err(|error| {
                if routing.iter().any(|hop| {
                    matches!(hop.route, GatewayRouteV1::UserProxy { .. })
                        || hop.proxy_failure.is_some()
                }) && matches!(
                    error,
                    NetworkControlError::Transport(_)
                        | NetworkControlError::Timeout
                        | NetworkControlError::DownloadLimitExceeded
                        | NetworkControlError::ResponseTooLarge
                ) {
                    super::user_proxy::routed_error(error, &routing, false)
                } else {
                    error
                }
            })?;
        Ok(OrdinaryGatewayResponse {
            response,
            final_url,
            redirects,
            credential_reflection,
            routing,
        })
    }

    /// GET download is credential-free. The caller owns bounded consumption and publication;
    /// no response-sized allocation occurs in this API or GatewayBodyStream.
    pub async fn stream_ordinary(
        &self,
        grant: CoreRequestGrant<OrdinaryToolRequestGrant>,
        request: GatewayRequest,
        started: impl FnOnce() -> GatewayStartedHook + Send,
    ) -> Result<OrdinaryGatewayStream, NetworkControlError> {
        if grant.purpose.credential().is_some()
            || request.plan.method != "GET"
            || !request.body.is_empty()
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        let opened = self.open_response(grant, request, Some(Box::new(started)), None).await?;
        let body = GatewayBodyStream::new(
            opened.response,
            &opened.headers,
            opened.rate_permit,
            opened.max_download_bytes,
            opened.max_decompressed_bytes,
        )?;
        Ok(OrdinaryGatewayStream {
            routing: opened.routing,
            final_url: opened.final_url,
            redirects: opened.redirect_chain,
            response: GatewayStreamResponse {
                status: opened.status,
                headers: opened.headers,
                final_origin: opened.final_origin,
                final_destination: opened.final_destination,
                redirect_hops: opened.redirect_hops,
                body,
            },
        })
    }
}

pub(super) fn authentication_header(
    name: &str,
    credential: Option<&aura_permission_domain::ToolCredentialUseV1>,
) -> bool {
    crate::is_authentication_header(name)
        || credential.is_some_and(|c| matches!(&c.placement,aura_permission_domain::ToolCredentialPlacementV1::FixedHeader { header_name } if name.eq_ignore_ascii_case(header_name.as_str())))
}
