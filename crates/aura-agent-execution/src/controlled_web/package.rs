//! Package HTTP tools on the controlled-web path: the exact request in the approved
//! operation, the user's bound credential re-checked at use, the declared statuses
//! and result contract evaluated on the answer. Redirects are refused.
use super::*;
use crate::agent_execution::package_tools;
use aura_network_control::ordinary_tools::OrdinaryToolRequestGrant;
use aura_package_service::package_http;

impl ControlledWeb {
    pub(super) async fn invoke_package_http(
        &self,
        dispatch: &AuthorizedDispatch,
        service: &Arc<crate::agent_execution::AgentExecution>,
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
                    "agent.package_http.execution_policy",
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
        let work = self.package_http(dispatch, service, call, &mut cancel);
        match tokio::time::timeout(timeout, work).await {
            Ok(finished) => finished,
            Err(_) => InvokeFinish {
                value: Err(anyhow::anyhow!("tool.execution_timeout")),
                cancelled: false,
                stop_confirmed: true,
            },
        }
    }

    async fn package_http(
        &self,
        dispatch: &AuthorizedDispatch,
        service: &Arc<crate::agent_execution::AgentExecution>,
        call: &crate::agent_execution::CallContext,
        cancel: &mut watch::Receiver<bool>,
    ) -> InvokeFinish {
        let value = or_cancel(cancel, async {
            let content = &dispatch.request().operation.content;
            let request = package_tools::http::gateway_request(content)?;
            let checker = Arc::clone(service);
            let checked = content.clone();
            let definition = call.validated.definition.clone();
            // The live install and the bound credential, read on the bounded owner.
            let (usage, secret) = self
                .section(
                    store_step(content.identity.agent_run_id, "agent.package_http.credential"),
                    StoreAccess::Read,
                    move || {
                        let tool = {
                            let store = checker.journal.lock()?;
                            package_tools::installed_tool_for(
                                store.database.connection(),
                                &definition,
                            )?
                        };
                        Ok(match package_tools::http::load_secret(&checker, &checked, &tool)? {
                            Some((usage, secret)) => (Some(usage), Some(secret)),
                            None => (None, None),
                        })
                    },
                )
                .await?;
            let grant = OrdinaryToolRequestGrant::issue(
                dispatch,
                &request,
                usage,
                UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::seconds(30)),
            )?;
            let operation_id = dispatch.request().operation.operation_id;
            let grant =
                self.apply_frozen_proxy(operation_id, content.identity.agent_run_id, grant).await?;
            let start =
                self.transport_start(dispatch, &request, "package_http.request_started").await?;
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
                    super::proxy::explain_transport(error)
                })?;
            self.remember_route(operation_id, &response.routing);
            let ranges = package_tools::http::success_ranges(content)?;
            match package_http::evaluate_contract(
                &call.validated.definition.result_contract,
                &ranges,
                response.response.status,
                &response.response.body,
            ) {
                package_http::PackageHttpEvaluation::Completed { value } => Ok(value),
                package_http::PackageHttpEvaluation::UnexpectedStatus { status } => {
                    Err(anyhow::anyhow!("package.http_status_{status}"))
                }
                package_http::PackageHttpEvaluation::InvalidResult => {
                    Err(anyhow::anyhow!("package.http_result_invalid"))
                }
            }
        })
        .await;
        match value {
            OrCancel::Value(value) => {
                InvokeFinish { value, cancelled: false, stop_confirmed: true }
            }
            OrCancel::Cancelled { stop_confirmed } => InvokeFinish::cancelled(stop_confirmed),
        }
    }
}

/// Whether this operation is a package HTTP call in its exact shape.
pub(super) fn is_package_http(content: &aura_execution_contract::OperationContent) -> bool {
    aura_security_contract::package_http(
        &content.source,
        &content.action_kind,
        &content.capabilities,
        &content.resources,
    )
}
