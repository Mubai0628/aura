use super::*;
use aura_network_control::ordinary_tools::OrdinaryToolRequestGrant;

impl ControlledWeb {
    /// Use the admitted, immutable execution policy rather than a later settings read.
    pub(super) async fn invoke_http_with_policy(
        &self,
        dispatch: &AuthorizedDispatch,
        call: &crate::agent_execution::CallContext,
        mut cancel: watch::Receiver<bool>,
    ) -> InvokeFinish {
        let web = self.clone();
        let batch = call.batch.tool_batch_id;
        let id = call.validated.call.tool_call_id;
        let timeout = self
            .section(
                store_step(
                    dispatch.request().operation.content.identity.agent_run_id,
                    "agent.http.execution_policy",
                ),
                StoreAccess::Read,
                move || {
                    let store = web.journal.lock()?;
                    let batch = aura_persistence_v2::tool::execution::batch(
                        store.database.connection(),
                        batch,
                    )?;
                    let plan = batch
                        .plan
                        .calls
                        .iter()
                        .find(|plan| plan.tool_call_id == id)
                        .ok_or_else(|| anyhow::anyhow!("agent.plan_call_missing"))?;
                    Ok(Duration::from_millis(plan.timeout_ms))
                },
            )
            .await;
        let timeout = match timeout {
            Ok(timeout) => timeout,
            Err(error) => {
                return InvokeFinish { value: Err(error), cancelled: false, stop_confirmed: true };
            }
        };
        match tokio::time::timeout(timeout, self.invoke_http(dispatch, &mut cancel)).await {
            Ok(finished) => finished,
            Err(_) => InvokeFinish {
                value: Err(anyhow::anyhow!("tool.execution_timeout")),
                cancelled: false,
                stop_confirmed: true,
            },
        }
    }

    pub(super) async fn invoke_http(
        &self,
        dispatch: &AuthorizedDispatch,
        cancel: &mut watch::Receiver<bool>,
    ) -> InvokeFinish {
        let value = or_cancel(cancel, async {
            let content = &dispatch.request().operation.content;
            let request = crate::agent_execution::network::gateway_request(content)?;
            let web = self.clone();
            let checked = content.clone();
            let (usage, secret) = self
                .section(
                    store_step(content.identity.agent_run_id, "agent.http.credential"),
                    StoreAccess::Read,
                    move || {
                        let usage = crate::agent_execution::network::credential(
                            &web.persistence,
                            &checked.action,
                            &checked.identity,
                        )?
                        .map(|(usage, _, _)| usage);
                        let secret = crate::agent_execution::network::load_secret(
                            &web.persistence,
                            &checked,
                        )?;
                        Ok((usage, secret))
                    },
                )
                .await?;
            let grant = OrdinaryToolRequestGrant::issue(
                dispatch,
                &request,
                usage,
                UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::seconds(30)),
            )?;
            let grant = self
                .apply_frozen_proxy(
                    dispatch.request().operation.operation_id,
                    content.identity.agent_run_id,
                    grant,
                )
                .await?;
            let start = self.transport_start(dispatch, &request, "http.request_started").await?;
            let started = std::time::Instant::now();
            let operation_id = dispatch.request().operation.operation_id;
            let target = url::Url::parse(&request.plan.requested_url).ok();
            let response = self
                .gateway
                .execute_ordinary(
                    grant,
                    request,
                    secret.as_ref().map(|s| s as &dyn aura_network_control::ToolCredentialResolver),
                    start,
                )
                .await
                .map_err(|error| {
                    if let aura_network_control::NetworkControlError::RoutedTransport {
                        routing,
                        ..
                    } = &error
                    {
                        self.remember_route(operation_id, routing);
                    }
                    match &target {
                        Some(url) => super::proxy::explain_transport_to(error, url),
                        None => safe_transport(error),
                    }
                })?;
            self.remember_route(operation_id, &response.routing);
            Ok(serde_json::to_value(aura_net_tools::http_result(
                response,
                started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            ))?)
        })
        .await;
        match value {
            OrCancel::Value(value) => {
                InvokeFinish { value, cancelled: false, stop_confirmed: true }
            }
            OrCancel::Cancelled { stop_confirmed } => InvokeFinish::cancelled(stop_confirmed),
        }
    }
    /// Before the user has answered, only targets that need no DNS query are checked: resolving
    /// a name already sends it to DNS, and the name itself can carry data.
    pub(crate) async fn preflight_network_before_answer(
        &self,
        operation: &SecuredOperation,
    ) -> Result<()> {
        if operation.content.network == OperationNetwork::ExactOrigins {
            let request = crate::agent_execution::network::gateway_request(&operation.content)?;
            if resolves_without_dns(&request.plan.requested_url) {
                return self.preflight_network(operation).await;
            }
        }
        Ok(())
    }
    pub(crate) async fn preflight_network(&self, operation: &SecuredOperation) -> Result<()> {
        if operation.content.network == OperationNetwork::ExactOrigins {
            let request = crate::agent_execution::network::gateway_request(&operation.content)?;
            self.gateway
                .check_ordinary_target(&request.plan.requested_url)
                .await
                .map_err(safe_transport)?;
        }
        Ok(())
    }
}

fn resolves_without_dns(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| match url.host() {
        Some(url::Host::Domain(name)) => {
            name.trim_end_matches('.').eq_ignore_ascii_case("localhost")
        }
        Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) => true,
        None => false,
    })
}

fn safe_transport(error: aura_network_control::NetworkControlError) -> anyhow::Error {
    super::proxy::explain_transport(error)
}
