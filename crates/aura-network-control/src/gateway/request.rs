use super::*;

impl CoreHttpClientGateway {
    pub(super) async fn open_response<P>(
        &self,
        grant: CoreRequestGrant<P>,
        mut request: GatewayRequest,
        mut on_started: Option<Box<dyn FnOnce() -> GatewayStartedHook + Send + '_>>,
        tool_credentials: Option<&dyn ToolCredentialResolver>,
    ) -> Result<OpenedGatewayResponse, NetworkControlError>
    where
        P: GatewayGrantPurpose,
    {
        self.consume_core_request_grant(&grant)?;
        validate_core_request_binding(&grant, &request.plan, true)?;
        grant.purpose.validate_request(&request)?;
        if u64::try_from(request.body.len()).unwrap_or(u64::MAX)
            > grant.byte_limits.max_upload_bytes
        {
            return Err(NetworkControlError::UploadLimitExceeded);
        }
        self.scanner.scan_http_request(
            &request,
            !grant.purpose.is_general_tool() && !grant.purpose.carries_own_credentials(),
        )?;

        let requested_tool_credential = grant.purpose.tool_credential_binding().cloned();
        let tool_success_status_ranges =
            grant.purpose.tool_success_status_ranges().map(<[_]>::to_vec);
        let mut credential_reflection = None;
        let ordinary = grant.purpose.is_ordinary_tool();
        let mut credential_forwarding = true;
        let mut redirect_chain = Vec::new();
        let original_method = request.plan.method.clone();
        let mut redirect_hops = 0_u8;
        let frozen_proxy = grant.purpose.frozen_user_proxy().cloned();
        let mut routing = Vec::new();
        let mut clients: BTreeMap<CoreClientCacheKey, HopClient> = BTreeMap::new();
        loop {
            validate_core_request_binding(&grant, &request.plan, redirect_hops == 0)?;
            let authorized = if ordinary {
                crate::general_tool::authorize_for_caller(
                    request.plan.clone(),
                    &self.options,
                    grant.caller,
                )
                .await?
            } else if grant.purpose.is_controlled_web() {
                // The typed grant has already bound this exact credential-free
                // GET. Source classes remain facts, never relabelled PublicQuery.
                crate::controlled_web::authorize(request.plan.clone(), self.options.dns_timeout)
                    .await?
            } else if grant.purpose.is_general_tool() {
                crate::general_tool::authorize(request.plan.clone(), &self.options).await?
            } else {
                self.authorize_with_context(
                    request.plan.clone(),
                    grant.purpose.allows_ui_configuration_private_destination(),
                    grant.purpose.trusted_endpoint_content(),
                )
                .await?
            };
            let destination = authorized.destination.clone();
            if ordinary {
                crate::ordinary_tools::check_destination(&destination)?;
            }
            validate_core_hop(&grant, &authorized)?;
            let proxy = super::user_proxy::select_proxy(
                ordinary || grant.purpose.uses_user_proxy(),
                frozen_proxy.as_ref(),
                &destination,
            );
            if let Some(proxy) = proxy {
                super::user_proxy::check_proxy(proxy)?;
            }
            let origin = NormalizedOrigin {
                scheme: destination.scheme.clone(),
                host: destination.host.clone(),
                port: destination.port,
            };
            let _rate_permit = self
                .limiter
                .acquire(&origin, u64::try_from(request.body.len()).unwrap_or(u64::MAX))?;
            let client_key = CoreClientCacheKey::routed(&destination, proxy);
            let client = if let Some(client) = clients.get(&client_key) {
                client.clone()
            } else {
                let client =
                    HopClient::new(&destination, &request, &self.options, ordinary, proxy)?;
                clients.insert(client_key, client.clone());
                client
            };
            let method = Method::from_bytes(request.plan.method.as_bytes())
                .map_err(|_| NetworkControlError::InvalidMethod)?;
            let mut headers = HeaderMap::new();
            for (name, value) in &request.headers {
                let name =
                    HeaderName::from_str(name).map_err(|_| NetworkControlError::InvalidMethod)?;
                let value = HeaderValue::from_str(value).map_err(|_| {
                    NetworkControlError::SecretDetected {
                        location: "request.header".to_owned(),
                        kind: "invalid_header_value".to_owned(),
                    }
                })?;
                headers.append(name, value);
            }
            self.inject_credentials(&request.plan, &origin, &mut headers)?;
            self.inject_provider_credential(
                grant.purpose.provider_credential_binding(),
                &origin,
                &mut headers,
            )?;
            self.inject_tool_credential(
                requested_tool_credential.as_ref().filter(|_| !ordinary || credential_forwarding),
                tool_credentials,
                &origin,
                &mut headers,
                &mut credential_reflection,
            )?;
            let hop_started = tokio::time::Instant::now();
            let mut route = GatewayHopRouteV1 {
                origin: origin.clone(),
                route: GatewayRouteV1::from_proxy(proxy),
                proxy_failure: None,
            };
            let send = client.send(request.clone(), method.clone(), headers.clone());
            tokio::pin!(send);
            // Poll the transport once so DNS, connection and request preparation happen,
            // then run the awaitable hook - the caller's own bounded section - and only
            // then carry the same, still-pinned request future to its response. The hook
            // can never replace the request's result or its accounting.
            let begun =
                std::future::poll_fn(|cx| match std::future::Future::poll(send.as_mut(), cx) {
                    std::task::Poll::Ready(result) => std::task::Poll::Ready(Some(result)),
                    std::task::Poll::Pending => std::task::Poll::Ready(None),
                })
                .await;
            if let Some(started) = on_started.take() {
                started().await?;
            }
            let result = match begun {
                Some(result) => result,
                None => send.await,
            };
            let response = match result {
                Ok(response) => response,
                Err(super::user_proxy::HopSendError {
                    error: NetworkControlError::UserProxyFailure(failure),
                    request_not_started: true,
                }) if proxy.is_some() => {
                    // Owner clarification: an unavailable proxy behaves as disabled. Only a
                    // proved pre-HTTP failure permits this direct attempt; never replay Unknown.
                    route.route = GatewayRouteV1::Direct;
                    route.proxy_failure = Some(*failure);
                    crate::ordinary_tools::check_destination(&destination)?;
                    validate_core_hop(&grant, &authorized)?;
                    let direct_key = CoreClientCacheKey::routed(&destination, None);
                    let direct = if let Some(client) = clients.get(&direct_key) {
                        client.clone()
                    } else {
                        let client =
                            HopClient::new(&destination, &request, &self.options, ordinary, None)?;
                        clients.insert(direct_key, client.clone());
                        client
                    };
                    let elapsed_ms =
                        u64::try_from(hop_started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    let remaining = request.plan.timeout_ms.saturating_sub(elapsed_ms);
                    if remaining == 0 {
                        routing.push(route);
                        return Err(super::user_proxy::routed_error(
                            NetworkControlError::Timeout,
                            &routing,
                            redirect_hops == 0,
                        ));
                    }
                    match tokio::time::timeout(
                        Duration::from_millis(remaining),
                        direct.send(request.clone(), method, headers),
                    )
                    .await
                    {
                        Ok(Ok(response)) => response,
                        Ok(Err(error)) => {
                            routing.push(route);
                            return Err(super::user_proxy::routed_error(
                                error.error,
                                &routing,
                                redirect_hops == 0 && error.request_not_started,
                            ));
                        }
                        Err(_) => {
                            routing.push(route);
                            return Err(super::user_proxy::routed_error(
                                NetworkControlError::Timeout,
                                &routing,
                                false,
                            ));
                        }
                    }
                }
                Err(error) => {
                    routing.push(route);
                    if ((ordinary || grant.purpose.reports_unsent_failures())
                        && error.request_not_started)
                        || routing.iter().any(|hop| {
                            matches!(hop.route, GatewayRouteV1::UserProxy { .. })
                                || hop.proxy_failure.is_some()
                        })
                    {
                        return Err(super::user_proxy::routed_error(
                            error.error,
                            &routing,
                            redirect_hops == 0 && error.request_not_started,
                        ));
                    }
                    return Err(error.error);
                }
            };
            routing.push(route);
            let status = response.status();
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned);

            let follow = if ordinary {
                matches!(original_method.as_str(), "GET" | "HEAD")
                    && matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
            } else {
                status.is_redirection()
            };
            if follow {
                if let Some(scanner) = credential_reflection.as_mut() {
                    scanner.scan_headers(
                        response.headers(),
                        CredentialReflectionSurfaceV1::RedirectHeaderValue,
                    )?;
                }
                let Some(location) = location else {
                    return Err(NetworkControlError::RedirectBlocked);
                };
                if !request.body.is_empty() || !matches!(original_method.as_str(), "GET" | "HEAD") {
                    return Err(NetworkControlError::RedirectBlocked);
                }
                let max_hops = match request.plan.redirect_policy {
                    RedirectPolicy::Deny => return Err(NetworkControlError::RedirectBlocked),
                    RedirectPolicy::SameOrigin { max_hops }
                    | RedirectPolicy::ReauthorizeEachHop { max_hops } => max_hops,
                };
                redirect_hops = redirect_hops.saturating_add(1);
                if redirect_hops > max_hops {
                    return Err(NetworkControlError::RedirectLimitExceeded);
                }
                let current = Url::parse(&request.plan.requested_url)
                    .map_err(|_| NetworkControlError::InvalidUrl)?;
                let next = current.join(&location).map_err(|_| NetworkControlError::InvalidUrl)?;
                if !next.username().is_empty()
                    || next.password().is_some()
                    || next.fragment().is_some()
                {
                    return Err(NetworkControlError::UrlCredentialsForbidden);
                }
                if ordinary && current.origin() != next.origin() {
                    credential_forwarding = false;
                    request.headers.retain(|(name, _)| {
                        !ordinary::authentication_header(name, requested_tool_credential.as_ref())
                    });
                }
                if matches!(request.plan.redirect_policy, RedirectPolicy::SameOrigin { .. }) {
                    let next_port =
                        next.port_or_known_default().ok_or(NetworkControlError::InvalidUrl)?;
                    if next.scheme() != origin.scheme
                        || next.host_str().is_none_or(|host| {
                            !host.trim_end_matches('.').eq_ignore_ascii_case(&origin.host)
                        })
                        || next_port != origin.port
                    {
                        return Err(NetworkControlError::RedirectBlocked);
                    }
                }
                redirect_chain.push(crate::ordinary_tools::safe_url(next.as_str()));
                request.plan.requested_url = next.into();
                request.plan.destination = None;
                continue;
            }

            let mut response_headers = sanitized_response_headers(response.headers());
            if grant.purpose.reads_bearer_challenge() {
                keep_bearer_challenge(&mut response_headers, response.headers());
            }
            if response
                .content_length()
                .is_some_and(|length| length > request.plan.byte_limits.max_download_bytes)
            {
                if routing.iter().any(|hop| {
                    matches!(hop.route, GatewayRouteV1::UserProxy { .. })
                        || hop.proxy_failure.is_some()
                }) {
                    return Err(super::user_proxy::routed_error(
                        NetworkControlError::ResponseTooLarge,
                        &routing,
                        false,
                    ));
                }
                return Err(NetworkControlError::ResponseTooLarge);
            }
            return Ok(OpenedGatewayResponse {
                final_url: crate::ordinary_tools::safe_url(&request.plan.requested_url),
                routing,
                redirect_chain,
                status: status.as_u16(),
                headers: response_headers,
                final_origin: origin,
                final_destination: destination,
                redirect_hops,
                response,
                rate_permit: _rate_permit,
                max_download_bytes: request.plan.byte_limits.max_download_bytes,
                max_decompressed_bytes: request.plan.byte_limits.max_decompressed_bytes,
                credential_reflection,
                tool_success_status_ranges,
            });
        }
    }
}

/// An MCP server's `WWW-Authenticate: Bearer …` challenge names its sign-in metadata and the
/// scope to ask for (RFC 6750, RFC 9728); nothing in it is secret, so it is kept for the MCP
/// grant. Any other challenge stays redacted.
fn keep_bearer_challenge(sanitized: &mut [(String, String)], headers: &reqwest::header::HeaderMap) {
    let challenges: Vec<&str> = headers
        .get_all("www-authenticate")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect();
    let mut found = challenges.into_iter();
    for (_, value) in sanitized.iter_mut().filter(|(name, _)| name == "www-authenticate") {
        if let Some(real) = found.next()
            && real.get(..7).is_some_and(|scheme| scheme.eq_ignore_ascii_case("bearer "))
            && real.len() <= 2048
        {
            *value = real.to_owned();
        }
    }
}
