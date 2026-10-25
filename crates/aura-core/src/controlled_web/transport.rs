use super::*;
use aura_network_control::ordinary_tools::{OrdinaryToolRequestGrant, safe_url};

impl ControlledWeb {
    pub(super) async fn gateway_get(
        &self,
        dispatch: &AuthorizedDispatch,
        url: &url::Url,
        max_bytes: u32,
        event: &str,
    ) -> Result<(u16, String, Vec<u8>)> {
        let parent = crate::agent_execution::network::gateway_request(
            &dispatch.request().operation.content,
        )?;
        let request = aura_net_tools::web_request(
            url.as_str(),
            parent.plan.identity.clone(),
            u64::from(max_bytes),
        )?;
        let expiry =
            UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::seconds(30));
        let grant = if request.plan.requested_url == parent.plan.requested_url {
            OrdinaryToolRequestGrant::issue(dispatch, &request, None, expiry)?
        } else {
            OrdinaryToolRequestGrant::subresource(dispatch, &parent, &request, expiry)?
        };
        let grant = self
            .apply_frozen_proxy(
                dispatch.request().operation.operation_id,
                dispatch.request().operation.content.identity.agent_run_id,
                grant,
            )
            .await?;
        let start = self.transport_start(dispatch, &request, event).await?;
        let operation_id = dispatch.request().operation.operation_id;
        let opened =
            self.gateway.execute_ordinary(grant, request, None, start).await.map_err(|error| {
                if let aura_network_control::NetworkControlError::RoutedTransport {
                    routing, ..
                } = &error
                {
                    self.remember_route(operation_id, routing);
                }
                super::proxy::explain_transport(error)
            })?;
        self.remember_route(operation_id, &opened.routing);
        let response = opened.response;
        let media = response
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map_or("", |(_, v)| v.as_str())
            .to_owned();
        Ok((response.status, media, response.body))
    }

    pub(super) async fn transport_start(
        &self,
        dispatch: &AuthorizedDispatch,
        request: &GatewayRequest,
        event: &str,
    ) -> Result<impl FnOnce() -> aura_network_control::GatewayStartedHook + Send + 'static> {
        let content = &dispatch.request().operation.content;
        let session =
            content.identity.session_id.ok_or_else(|| anyhow::anyhow!("web.session_required"))?;
        let operation_id = dispatch.request().operation.operation_id;
        let run = content.identity.agent_run_id;
        let launch = if self.activities.started_mode(operation_id, session)?.is_none() {
            Some(self.persistence.permission_start_lease(session).await)
        } else {
            None
        };
        let web = self.clone();
        let checked = dispatch.request().clone();
        let event = event.to_owned();
        let url = safe_url(&request.plan.requested_url);
        let method = request.plan.method.clone();
        let request_id = request.plan.request_id;
        self.section(store_step(run,"agent.web.request_journal"),StoreAccess::Write,move || {
            web.journal.lock()?.database.unit_of_work(|tx| {
                aura_persistence_v2::agent::checkpoint::verify_start_operation(tx,operation_id)?;
                aura_persistence_v2::execution::dispatch_authority(tx,&checked,UtcTimestamp::now())?;
                aura_persistence_core::operation::journal(tx,operation_id,&event,
                    &serde_json::json!({"request_id":request_id,"url":url,"method":method,"source_classification":"not_rewritten"}),UtcTimestamp::now())
            })?;
            Ok(())
        }).await?;
        let web = self.clone();
        Ok(move || -> aura_network_control::GatewayStartedHook {
            Box::pin(async move {
                web.activities.mark_started(operation_id).map_err(|_| {
                    aura_network_control::NetworkControlError::CoreGrantBindingMismatch
                })?;
                let writer = web.clone();
                let stored = web
                    .section(
                        store_step(run, "agent.web.transport_started"),
                        StoreAccess::Write,
                        move || {
                            writer.journal.lock()?.database.unit_of_work(|tx| {
                                aura_persistence_core::operation::journal(
                                    tx,
                                    operation_id,
                                    "web.transport_started",
                                    &serde_json::json!({"session_id":session}),
                                    UtcTimestamp::now(),
                                )
                            })?;
                            Ok(())
                        },
                    )
                    .await;
                drop(launch);
                stored.map_err(|_| {
                    aura_network_control::NetworkControlError::CoreGrantBindingMismatch
                })
            })
        })
    }
}
