use super::*;
mod stops;
use crate::agent_execution::{AgentExecution, CallContext, network};
use aura_network_control::ordinary_tools::OrdinaryToolRequestGrant;
use std::sync::Mutex;

impl ControlledWeb {
    pub(super) async fn invoke_download(
        &self,
        dispatch: &AuthorizedDispatch,
        service: &Arc<AgentExecution>,
        call: &CallContext,
        mut cancel: watch::Receiver<bool>,
    ) -> InvokeFinish {
        let result = self.download(dispatch, service, call, &mut cancel).await;
        let cancelled = result.as_ref().err().is_some_and(|e| e.to_string() == "http.cancelled");
        let stop_confirmed = stops::worker_confirmed(result.as_ref().err());
        InvokeFinish { value: result, cancelled, stop_confirmed }
    }
    async fn download(
        &self,
        dispatch: &AuthorizedDispatch,
        service: &Arc<AgentExecution>,
        call: &CallContext,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<serde_json::Value> {
        ensure!(!*cancel.borrow(), "http.cancelled");
        let content = &dispatch.request().operation.content;
        let call_id = call.validated.call.tool_call_id;
        let target = network::download_target(content)?;
        let begin = Arc::clone(service);
        let checked = call.clone();
        let staged = service
            .download_file_section(
                call_id,
                store_step(content.identity.agent_run_id, "agent.http.download_begin"),
                move || {
                    begin.verify_source(&checked)?;
                    Ok(begin.fs.begin_download(
                        target,
                        checked.snapshot.workspace_id,
                        checked.snapshot.session_id,
                    )?)
                },
            )
            .await?;
        let staging = Arc::new(Mutex::new(Some(staged)));
        let transfer = async {
            let request = network::gateway_request(content)?;
            let grant = OrdinaryToolRequestGrant::issue(dispatch,&request,None,
                UtcTimestamp::from_value(UtcTimestamp::now().value()+time::Duration::minutes(5)))?;
            let grant = self.apply_frozen_proxy(dispatch.request().operation.operation_id, content.identity.agent_run_id, grant).await?;
            let start = self.transport_start(dispatch,&request,"http.download_started").await?;
            let operation_id = dispatch.request().operation.operation_id;
            let mut stream = tokio::select! { biased;
                _ = cancel.changed() => anyhow::bail!("http.cancelled"),
                result = self.gateway.stream_ordinary(grant,request,start) => result.map_err(|error| {
                    if let aura_network_control::NetworkControlError::RoutedTransport { routing, .. } = &error {
                        self.remember_route(operation_id, routing);
                    }
                    super::proxy::explain_transport(error)
                })?,
            };
            self.remember_route(operation_id, &stream.routing);
            ensure!((200..300).contains(&stream.response.status),"http.download_status:{}",stream.response.status);
            loop {
                ensure!(!*cancel.borrow(),"http.cancelled");
                let chunk = tokio::select! { biased;
                    _ = cancel.changed() => anyhow::bail!("http.cancelled"),
                    result = stream.response.body.next_chunk() => result.map_err(|error|anyhow::anyhow!(aura_network_control::ordinary_tools::safe_transport_code(&error)))?,
                };
                let Some(chunk) = chunk else {break};
                let writer = Arc::clone(&staging);
                // Do not acknowledge cancellation while a filesystem worker still owns a write.
                service.download_file_section(call_id,store_step(content.identity.agent_run_id,"agent.http.download_chunk"),move || {
                    writer.lock().map_err(|_|anyhow::anyhow!("http.staging_poisoned"))?.as_mut()
                        .ok_or_else(||anyhow::anyhow!("http.staging_finished"))?.append(&chunk)?;
                    Ok(())
                }).await?;
            }
            ensure!(!*cancel.borrow(),"http.cancelled");
            let writer = Arc::clone(service);
            let checked = call.clone();
            let holder = Arc::clone(&staging);
            let lease = service.persistence.permission_start_lease(call.snapshot.session_id).await;
            let published = service.download_file_section(call_id,store_step(content.identity.agent_run_id,"agent.http.download_publish"),move || {
                writer.persistence.with_session_permission_authority(checked.snapshot.session_id,|mode| -> Result<_> {
                    ensure!(writer.operation_permitted_now(&checked.operation,&checked.snapshot,mode.mode,mode.revision)?,"agent.permission_withdrawn");
                    writer.verify_source(&checked)?;
                    // The initial dispatch lease can expire during a large transfer. Publication
                    // needs the live capability check above, including approval expiry/revocation,
                    // rather than extending or replaying that already-consumed start lease.
                    let staged = holder.lock().map_err(|_|anyhow::anyhow!("http.staging_poisoned"))?.take().ok_or_else(||anyhow::anyhow!("http.staging_finished"))?;
                    let published = writer.fs.finish_download(staged,checked.snapshot.workspace_id,checked.operation.content.identity.correlation_id)
                        .map_err(|error|match error {
                            aura_fs_tools::FsToolError::InvalidRequest(ref reason) if reason == "http.never_published" => anyhow::anyhow!("http.never_published"),
                            aura_fs_tools::FsToolError::ExternalPublishRolledBack {..} => anyhow::anyhow!("http.publication_rolled_back"),
                            _ => anyhow::anyhow!("http.publication_unconfirmed"),
                        })?;
                    crate::agent_execution::files::publication::follow_publications(&writer,&checked,&published.receipts).map_err(|_|anyhow::anyhow!("http.publication_unconfirmed"))?;
                    drop(lease);
                    Ok(published)
                })?
            }).await?;
            Ok(serde_json::to_value(HttpDownloadResult {status:stream.response.status,path:content.action["path"].as_str().unwrap_or_default().into(),bytes_written:published.bytes_written,
                hash:published.hash,content_type:stream.response.headers.iter().find(|(k,_)|k.eq_ignore_ascii_case("content-type")).map(|(_,v)|v.clone()),
                final_url:stream.final_url,overwritten:published.overwritten,published:true})?)
        }.await;
        let cleanup = Arc::clone(&staging);
        service
            .download_file_section(
                call_id,
                store_step(content.identity.agent_run_id, "agent.http.download_cleanup"),
                move || {
                    cleanup.lock().map_err(|_| anyhow::anyhow!("http.staging_poisoned"))?.take();
                    Ok(())
                },
            )
            .await?;
        transfer
    }
}
