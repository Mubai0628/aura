use super::*;

impl CommittedAuthorizedToolHttpAttemptV1 {
    /// Persists the Running boundary immediately before the in-process P04
    /// Gateway may consume the one-shot grant. Any Gateway error after this
    /// point is conservatively represented as an unknown dispatch outcome;
    /// raw transport diagnostics never enter ordinary Tool evidence.
    pub async fn dispatch(
        self,
        persistence: &PersistenceCoordinator,
        sync: &crate::sync_boundary::SyncBoundary,
        gateway: &CoreHttpClientGateway,
    ) -> Result<DispatchedAuthorizedToolAttemptV1, ToolAttemptDispatchErrorV1> {
        let Self {
            tool_batch_id,
            mut attempt,
            gateway_request,
            success_status_ranges,
            mut projection_state,
            persistence: pre_dispatch_persistence,
            projection_publish_error,
            scheduler_permit,
            result_contract,
            execution_semantics,
            timeout_ms,
            max_retries,
            dispatch_prepared_event_id,
            correlation_id,
            result_context,
        } = self;

        // Read the exact prepared binding on the same bounded owner as other credential I/O.
        // A caller timeout retains that owner's permit until the read actually settles.
        let credential = gateway_request.grant.purpose.credential().cloned();
        let local_secret = if let Some(binding) = credential {
            let owner = persistence.clone();
            let definition = result_context.definition_ref.clone();
            let destination = gateway_request
                .request
                .plan
                .destination
                .as_ref()
                .ok_or(ToolAttemptDispatchErrorV1::CredentialUnavailable)?;
            let origin = aura_permission_domain::NormalizedOrigin {
                scheme: destination.scheme.clone(),
                host: destination.host.clone(),
                port: destination.port,
            };
            match sync
                .run(
                    crate::sync_boundary::SyncSection::CredentialSecret,
                    crate::sync_boundary::SyncSubject::new()
                        .method("tool.http.credential")
                        .request(attempt.tool_execution_attempt_id),
                    move || {
                        crate::tool_credential_service::execution::HttpSecret::load(
                            &owner,
                            &definition,
                            tool_batch_id,
                            binding,
                            origin,
                        )
                    },
                )
                .await
            {
                crate::blocking_pool::BlockingAdmission::Completed(Ok(secret)) => Some(secret),
                _ => return Err(ToolAttemptDispatchErrorV1::CredentialUnavailable),
            }
        } else {
            None
        };
        let updated_at = UtcTimestamp::now();
        let mut running_attempt = attempt.clone();
        running_attempt.status = ToolExecutionAttemptStatusV1::Running;
        running_attempt.side_effect_state = SideEffectStateV1::Unknown;
        let mut running_projection_state = projection_state.clone();
        let attempt_projection = running_projection_state.runtime_started(&running_attempt)?;
        let runtime_started_persistence = persistence
            .commit_tool_execution_attempt_runtime_started(
                ToolExecutionAttemptRuntimeStartedRequest {
                    attempt: running_attempt.clone(),
                    attempt_projection,
                    updated_at,
                },
            )?;
        let runtime_started_publish_error = persistence
            .publish_tool_projection_changes(&runtime_started_persistence.projection_changes)
            .err();
        attempt = running_attempt;
        projection_state = running_projection_state;

        let evidence = match gateway
            .execute_tool_http_with_credentials(
                gateway_request,
                local_secret
                    .as_ref()
                    .map(|s| s as &dyn aura_network_control::ToolCredentialResolver),
            )
            .await
        {
            Ok(response) => http_response_evidence(response, success_status_ranges),
            Err(aura_network_control::NetworkControlError::CredentialReflectionDetected {
                evidence,
            }) => ToolHttpDispatchEvidenceV1 {
                candidate:
                    aura_tool_runtime::ToolRuntimeCandidateV1::DeterministicExternalFailure {
                        error: safe_network_gateway_error(
                            "network.credential_reflection_detected",
                            "The tool HTTP response was blocked because it reflected credential material.",
                            correlation_id,
                        ),
                    },
                response_header_names: Vec::new(),
                final_origin: None,
                final_destination: None,
                redirect_hops: None,
                gateway_error_code: Some("network.credential_reflection_detected"),
                credential_reflection: Some(*evidence),
                retry_source: None,
            },
            Err(
                aura_network_control::NetworkControlError::CredentialReflectionScanIncomplete {
                    evidence,
                    response_terminal_known,
                },
            ) => ToolHttpDispatchEvidenceV1 {
                candidate: if response_terminal_known {
                    aura_tool_runtime::ToolRuntimeCandidateV1::DeterministicExternalFailure {
                        error: safe_network_gateway_error(
                            "network.credential_reflection_scan_incomplete",
                            "The tool HTTP response was blocked because credential reflection scanning could not be completed.",
                            correlation_id,
                        ),
                    }
                } else {
                    aura_tool_runtime::ToolRuntimeCandidateV1::DispatchOutcomeUnknown
                },
                response_header_names: Vec::new(),
                final_origin: None,
                final_destination: None,
                redirect_hops: None,
                gateway_error_code: Some("network.credential_reflection_scan_incomplete"),
                credential_reflection: Some(*evidence),
                retry_source: None,
            },
            Err(error) => transient_gateway_failure_evidence(&error, correlation_id)
                .unwrap_or_else(|| ToolHttpDispatchEvidenceV1 {
                    candidate: aura_tool_runtime::ToolRuntimeCandidateV1::DispatchOutcomeUnknown,
                    response_header_names: Vec::new(),
                    final_origin: None,
                    final_destination: None,
                    redirect_hops: None,
                    gateway_error_code: Some(safe_network_gateway_error_code(&error)),
                    credential_reflection: None,
                    retry_source: None,
                }),
        };
        let mut projection_publish_errors =
            projection_publish_error.into_iter().collect::<Vec<_>>();
        projection_publish_errors.extend(runtime_started_publish_error);
        Ok(DispatchedAuthorizedToolAttemptV1 {
            tool_batch_id,
            attempt,
            projection_state,
            evidence: ToolRuntimeDispatchEvidenceV1::DeclarativeHttp(evidence),
            pre_dispatch_persistence,
            runtime_started_persistence: Some(runtime_started_persistence),
            progress_persistence: Vec::new(),
            projection_publish_errors,
            runtime_started_error: None,
            scheduler_permit,
            result_contract,
            execution_semantics,
            timeout_ms,
            max_retries,
            dispatch_prepared_event_id,
            correlation_id,
            result_context,
        })
    }
}

pub(super) fn http_response_evidence(
    response: ToolHttpGatewayResponseV1,
    success_status_ranges: Vec<(u16, u16)>,
) -> ToolHttpDispatchEvidenceV1 {
    let ToolHttpGatewayResponseV1 { response, credential_reflection, retry_after } = response;
    let GatewayResponse { status, headers, body, final_origin, final_destination, redirect_hops } =
        response;
    let mut response_header_names =
        headers.into_iter().map(|(name, _)| name.to_ascii_lowercase()).collect::<Vec<_>>();
    response_header_names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    response_header_names.dedup();
    let retry_source = http_retry_source(status, retry_after);
    ToolHttpDispatchEvidenceV1 {
        candidate: aura_tool_runtime::ToolRuntimeCandidateV1::HttpCompleted {
            status,
            success_status_ranges,
            response_body: body,
        },
        response_header_names,
        final_origin: Some(final_origin),
        final_destination: Some(final_destination),
        redirect_hops: Some(redirect_hops),
        gateway_error_code: None,
        credential_reflection,
        retry_source,
    }
}

pub(super) const fn http_retry_source(
    status: u16,
    retry_after: Option<GatewayRetryAfterV1>,
) -> Option<ToolCoreRetrySourceV1> {
    match status {
        408 => Some(ToolCoreRetrySourceV1::Http408 { retry_after }),
        425 => Some(ToolCoreRetrySourceV1::Http425 { retry_after }),
        429 => Some(ToolCoreRetrySourceV1::Http429 { retry_after }),
        502 => Some(ToolCoreRetrySourceV1::Http502 { retry_after }),
        503 => Some(ToolCoreRetrySourceV1::Http503 { retry_after }),
        504 => Some(ToolCoreRetrySourceV1::Http504 { retry_after }),
        _ => None,
    }
}

pub(super) fn transient_gateway_failure_evidence(
    error: &aura_network_control::NetworkControlError,
    correlation_id: CorrelationId,
) -> Option<ToolHttpDispatchEvidenceV1> {
    let (retry_source, code, message) = match error {
        aura_network_control::NetworkControlError::RateLimited => (
            ToolCoreRetrySourceV1::GatewayRateLimited,
            "network.rate_limited",
            "The tool HTTP request was rate limited by the network Gateway.",
        ),
        aura_network_control::NetworkControlError::Timeout => (
            ToolCoreRetrySourceV1::GatewayTimeout,
            "network.timeout",
            "The tool HTTP request timed out in the network Gateway.",
        ),
        aura_network_control::NetworkControlError::ProxyInterrupted => (
            ToolCoreRetrySourceV1::GatewayProxyInterrupted,
            "network.proxy_interrupted",
            "The tool HTTP request was interrupted by the network Gateway.",
        ),
        _ => return None,
    };
    Some(ToolHttpDispatchEvidenceV1 {
        candidate: aura_tool_runtime::ToolRuntimeCandidateV1::DeterministicExternalFailure {
            error: ErrorEnvelope::new(
                ErrorCode::new(code).expect("closed network retry error code"),
                ErrorCategory::Transport,
                message,
                false,
                correlation_id,
                None,
            ),
        },
        response_header_names: Vec::new(),
        final_origin: None,
        final_destination: None,
        redirect_hops: None,
        gateway_error_code: Some(code),
        credential_reflection: None,
        retry_source: Some(retry_source),
    })
}

/// Reduces every Gateway failure to a stable, non-secret diagnostic code.
/// Dynamic transport, filesystem, URL, credential and persistence details are
/// deliberately discarded; this is internal Attempt evidence, not a retry
/// declaration or a public Runtime error contract.
pub(super) fn safe_network_gateway_error(
    code: &'static str,
    message: &'static str,
    correlation_id: CorrelationId,
) -> ErrorEnvelope {
    ErrorEnvelope::new(
        ErrorCode::new(code).expect("frozen network error code"),
        ErrorCategory::Protocol,
        message,
        false,
        correlation_id,
        None,
    )
}
