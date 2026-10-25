use super::*;

impl CoreHttpClientGateway {
    /// Executes one P07 Declarative HTTP request only after the Core-owned
    /// Authority, attempt-bound one-shot grant and exact request agree.
    pub async fn execute_tool_http(
        &self,
        request: ToolHttpGatewayRequestV1,
    ) -> Result<ToolHttpGatewayResponseV1, NetworkControlError> {
        self.execute_tool_http_with_credentials(request, None).await
    }

    /// A request-local resolver retains the caller's checked tool/scope binding.
    /// Reuses this Gateway's limiter and one-shot grant owner; never clones a new Gateway.
    pub async fn execute_tool_http_with_credentials(
        &self,
        request: ToolHttpGatewayRequestV1,
        credentials: Option<&dyn ToolCredentialResolver>,
    ) -> Result<ToolHttpGatewayResponseV1, NetworkControlError> {
        request.authority.validate_grant(&request.grant)?;
        let opened = self.open_response(request.grant, request.request, None, credentials).await?;
        let (response, credential_reflection) = self.collect_response(opened).await?;
        let retry_after = parse_gateway_retry_after(&response.headers);
        Ok(ToolHttpGatewayResponseV1 { response, credential_reflection, retry_after })
    }

    pub async fn execute<P>(
        &self,
        grant: CoreRequestGrant<P>,
        request: GatewayRequest,
    ) -> Result<GatewayResponse, NetworkControlError>
    where
        P: GatewayGrantPurpose,
    {
        let opened = self.open_response(grant, request, None, None).await?;
        self.collect_response(opened).await.map(|(response, _)| response)
    }

    /// Like [`Self::execute`], but the caller reads the body as it arrives (bounded by the
    /// grant's byte limits): a Streamable HTTP MCP server answers with server-sent events.
    pub async fn stream<P>(
        &self,
        grant: CoreRequestGrant<P>,
        request: GatewayRequest,
    ) -> Result<GatewayStreamResponse, NetworkControlError>
    where
        P: GatewayGrantPurpose,
    {
        self.stream_with_credentials(grant, request, None).await
    }

    /// [`Self::stream`] for a grant that binds a credential: the request-local resolver
    /// supplies it after the secret scan, and the response headers and every body chunk are
    /// scanned for it before the caller sees them.
    pub async fn stream_with_credentials<P>(
        &self,
        grant: CoreRequestGrant<P>,
        request: GatewayRequest,
        credentials: Option<&dyn ToolCredentialResolver>,
    ) -> Result<GatewayStreamResponse, NetworkControlError>
    where
        P: GatewayGrantPurpose,
    {
        let mut opened = self.open_response(grant, request, None, credentials).await?;
        let final_answer = (200..300).contains(&opened.status);
        if let Some(scanner) = opened.credential_reflection.as_mut() {
            scanner.scan_headers(
                opened.response.headers(),
                if final_answer {
                    CredentialReflectionSurfaceV1::FinalHeaderValue
                } else {
                    CredentialReflectionSurfaceV1::ErrorHeaderValue
                },
            )?;
        }
        let mut body = GatewayBodyStream::new(
            opened.response,
            &opened.headers,
            opened.rate_permit,
            opened.max_download_bytes,
            opened.max_decompressed_bytes,
        )?;
        if let Some(scanner) = opened.credential_reflection.take() {
            body = body.scanned(
                scanner,
                if final_answer {
                    CredentialReflectionSurfaceV1::FinalResponseBody
                } else {
                    CredentialReflectionSurfaceV1::ErrorResponseBody
                },
            );
        }
        Ok(GatewayStreamResponse {
            status: opened.status,
            headers: opened.headers,
            final_origin: opened.final_origin,
            final_destination: opened.final_destination,
            redirect_hops: opened.redirect_hops,
            body,
        })
    }

    /// The callback runs after the first transport poll, after DNS and request
    /// preparation, but before yielding control. Core uses it to release its
    /// start lease and to record its own request-start line; response streaming
    /// remains free to finish under that grant.
    ///
    /// The callback is **awaitable**. Core's hook writes a journal line, and that
    /// write is a bounded section on Core's own owner: a synchronous hook would have had to
    /// run the journal lock and its SQL on the reactor thread polling this request. The
    /// transport future is simply held across the hook's await; nothing about the request,
    /// its grant or its byte accounting moves.
    pub async fn execute_started<P>(
        &self,
        grant: CoreRequestGrant<P>,
        request: GatewayRequest,
        on_started: impl FnOnce() -> GatewayStartedHook + Send,
    ) -> Result<GatewayResponse, NetworkControlError>
    where
        P: GatewayGrantPurpose,
    {
        let opened = self.open_response(grant, request, Some(Box::new(on_started)), None).await?;
        self.collect_response(opened).await.map(|(response, _)| response)
    }

    pub(super) async fn collect_response(
        &self,
        opened: OpenedGatewayResponse,
    ) -> Result<(GatewayResponse, Option<CredentialReflectionScanEvidenceV1>), NetworkControlError>
    {
        let OpenedGatewayResponse {
            status,
            headers,
            final_origin,
            final_destination,
            redirect_hops,
            response,
            rate_permit,
            max_download_bytes,
            max_decompressed_bytes,
            mut credential_reflection,
            tool_success_status_ranges,
            ..
        } = opened;
        let successful = tool_success_status_ranges.as_ref().is_none_or(|ranges| {
            ranges.iter().any(|(start, end)| (*start..=*end).contains(&status))
        });
        if let Some(scanner) = credential_reflection.as_mut() {
            scanner.scan_headers(
                response.headers(),
                if successful {
                    CredentialReflectionSurfaceV1::FinalHeaderValue
                } else {
                    CredentialReflectionSurfaceV1::ErrorHeaderValue
                },
            )?;
        }
        let mut body = match GatewayBodyStream::new(
            response,
            &headers,
            rate_permit,
            max_download_bytes,
            max_decompressed_bytes,
        ) {
            Ok(body) => body,
            Err(error) => {
                return Err(credential_reflection
                    .as_ref()
                    .map_or(error, CredentialReflectionScannerV1::incomplete_error));
            }
        };
        let mut collected = Vec::new();
        loop {
            match body.next_chunk().await {
                Ok(Some(chunk)) => collected.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(error) => {
                    return Err(credential_reflection
                        .as_ref()
                        .map_or(error, CredentialReflectionScannerV1::incomplete_error));
                }
            }
        }
        let credential_reflection = match credential_reflection.as_mut() {
            Some(scanner) => {
                if let Err(error) = scanner.scan_body(
                    &collected,
                    if successful {
                        CredentialReflectionSurfaceV1::FinalResponseBody
                    } else {
                        CredentialReflectionSurfaceV1::ErrorResponseBody
                    },
                ) {
                    return Err(Self::mark_reflection_response_terminal_known(error));
                }
                Some(
                    scanner
                        .passed_evidence()
                        .map_err(Self::mark_reflection_response_terminal_known)?,
                )
            }
            None => None,
        };
        Ok((
            GatewayResponse {
                status,
                headers,
                body: collected,
                final_origin,
                final_destination,
                redirect_hops,
            },
            credential_reflection,
        ))
    }

    fn mark_reflection_response_terminal_known(error: NetworkControlError) -> NetworkControlError {
        match error {
            NetworkControlError::CredentialReflectionScanIncomplete { evidence, .. } => {
                NetworkControlError::CredentialReflectionScanIncomplete {
                    evidence,
                    response_terminal_known: true,
                }
            }
            other => other,
        }
    }
}
