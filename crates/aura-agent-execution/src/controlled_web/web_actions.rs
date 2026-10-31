use super::*;

impl ControlledWeb {
    pub(super) async fn invoke(
        &self,
        dispatch: &AuthorizedDispatch,
        session: SessionId,
        mut cancel: watch::Receiver<bool>,
    ) -> InvokeFinish {
        if *cancel.borrow() {
            return InvokeFinish::cancelled(true);
        }
        if dispatch.request().operation.content.action_kind == "http.request" {
            return self.invoke_http(dispatch, &mut cancel).await;
        }
        let request = dispatch.request().clone();
        let request = &request;
        let run = request
            .operation
            .content
            .identity
            .agent_run_id
            .ok_or_else(|| anyhow::anyhow!("web.agent_authority_required"));
        let run = match run {
            Ok(run) => run,
            Err(error) => {
                return InvokeFinish { value: Err(error), cancelled: false, stop_confirmed: true };
            }
        };
        let content = &request.operation.content;
        // A call planned for DeepSeek's search or fetch.
        match web_service::frozen(&content.action) {
            Ok(Some(service)) => {
                return self.invoke_deepseek(dispatch, run, service, &mut cancel).await;
            }
            Ok(None) => {}
            Err(error) => {
                return InvokeFinish { value: Err(error), cancelled: false, stop_confirmed: true };
            }
        }
        if content.action_kind == "web.read" {
            // This session's own short start lease, released with the section below.
            let _start = self.persistence.permission_start_lease(session).await;
            let web = self.clone();
            let action = crate::agent_execution::network::model_action(&content.action);
            let operation = request.operation.clone();
            let preparation = request.plan.prepared.preparation.clone();
            // Boundary: the start revalidation and the stored-document read are one
            // synchronous section (permission authority, then journal), run whole on the
            // bounded owner.
            let value = self
                .section(
                    StoreStep::run(run, "agent.web.read_start"),
                    StoreAccess::Read,
                    move || -> Result<serde_json::Value> {
                        web.persistence.with_session_permission_authority(
                            session,
                            |_| -> Result<()> {
                                ensure!(
                                    web.inspect(&operation)? == preparation,
                                    "web.mode_changed_before_start"
                                );
                                aura_persistence_v2::agent::checkpoint::verify_start_operation(
                                    web.journal.lock()?.database.connection(),
                                    operation.operation_id,
                                )?;
                                web.activities.mark_started(operation.operation_id)
                            },
                        )??;
                        let input: WebReadInput = serde_json::from_value(action)?;
                        let document = web.document(session, input.document_artifact_id)?;
                        let (text, next) = aura_web_runtime::excerpt(
                            &document.text,
                            input.offset_chars,
                            input.max_chars,
                        )?;
                        Ok(serde_json::to_value(WebReadResult {
                            document_artifact_id: input.document_artifact_id,
                            source_url: document.source_url,
                            text,
                            next_offset_chars: next,
                        })?)
                    },
                )
                .await;
            return InvokeFinish { value, cancelled: false, stop_confirmed: true };
        }
        if content.action_kind == "web.fetch"
            && serde_json::from_value::<WebFetchInput>(
                crate::agent_execution::network::model_action(&content.action),
            )
            .is_err()
        {
            return InvokeFinish {
                value: Err(anyhow::anyhow!("web.fetch_input")),
                cancelled: false,
                stop_confirmed: true,
            };
        }
        let url = match self.target(session, &content.action_kind, &content.action) {
            Ok(url) => url,
            Err(error) => {
                return InvokeFinish { value: Err(error), cancelled: false, stop_confirmed: true };
            }
        };
        let max_bytes = if content.action_kind == "web.fetch" {
            match serde_json::from_value::<WebFetchInput>(
                crate::agent_execution::network::model_action(&content.action),
            ) {
                Ok(input) => input.max_bytes,
                Err(error) => {
                    return InvokeFinish {
                        value: Err(error.into()),
                        cancelled: false,
                        stop_confirmed: true,
                    };
                }
            }
        } else {
            2 * 1024 * 1024
        };
        let page_bytes = match or_cancel(
            &mut cancel,
            self.gateway_get(dispatch, &url, max_bytes, "web.request_started"),
        )
        .await
        {
            OrCancel::Cancelled { stop_confirmed } => {
                return InvokeFinish::cancelled(stop_confirmed);
            }
            OrCancel::Value(Err(error)) => {
                return InvokeFinish { value: Err(error), cancelled: false, stop_confirmed: true };
            }
            OrCancel::Value(Ok(value)) => value,
        };
        let (status, media, body) = page_bytes;
        if content.action_kind == "web.search" {
            let web = self.clone();
            let action = crate::agent_execution::network::model_action(&content.action);
            // Boundary: the search evidence artifact is one bounded write section.
            let value = self
                .section(
                    StoreStep::run(run, "agent.web.search_evidence"),
                    StoreAccess::Write,
                    move || -> Result<serde_json::Value> {
                        let input: WebSearchInput = serde_json::from_value(action)?;
                        let hits = aura_web_runtime::extract_search(
                            &url,
                            status,
                            &media,
                            &body,
                            input.max_results,
                        )?;
                        let id = ArtifactId::new();
                        let at = UtcTimestamp::now();
                        let result = WebSearchResult {
                            query: input.query,
                            surface: aura_web_runtime::SEARCH_SURFACE.into(),
                            hits,
                            retrieved_at: at,
                            live: true,
                            evidence_artifact_id: id,
                        };
                        let bytes = serde_json::to_vec(&result)?;
                        web.journal.lock()?.database.unit_of_work(|tx| {
                            aura_persistence_core::artifact::put(
                                tx,
                                id,
                                &format!("web-search:{session}"),
                                "application/vnd.aura.web-search+json",
                                &bytes,
                                at,
                            )
                        })?;
                        Ok(serde_json::to_value(result)?)
                    },
                )
                .await;
            return InvokeFinish { value, cancelled: false, stop_confirmed: true };
        }
        let input: WebFetchInput = match serde_json::from_value(
            crate::agent_execution::network::model_action(&content.action),
        ) {
            Ok(input) => input,
            Err(error) => {
                return InvokeFinish {
                    value: Err(error.into()),
                    cancelled: false,
                    stop_confirmed: true,
                };
            }
        };
        let page = FetchedPage {
            dispatch,
            url: &url,
            status,
            content_type: &media,
            body: &body,
            max_bytes,
        };
        match self.fetched_document(&page, input.mode, &mut cancel).await {
            OrCancel::Cancelled { stop_confirmed } => InvokeFinish::cancelled(stop_confirmed),
            OrCancel::Value(RenderedDocument { document, stop_confirmed }) => {
                // Boundary: the fetched document's own artifact publish is one
                // bounded write section, including the excerpt computed from it.
                let web = self.clone();
                let value = self
                    .section(
                        StoreStep::run(run, "agent.web.document"),
                        StoreAccess::Write,
                        move || -> Result<serde_json::Value> {
                            let document = document?;
                            let id = ArtifactId::new();
                            let bytes = serde_json::to_vec(&document)?;
                            web.journal.lock()?.database.unit_of_work(|tx| {
                                aura_persistence_core::artifact::put(
                                    tx,
                                    id,
                                    &format!("web-document:{session}"),
                                    DOCUMENT_MEDIA_TYPE,
                                    &bytes,
                                    document.retrieved_at,
                                )
                            })?;
                            let (excerpt, _) = aura_web_runtime::excerpt(&document.text, 0, 12000)?;
                            Ok(serde_json::to_value(WebFetchResult {
                                document_artifact_id: id,
                                source_url: document.source_url,
                                title: document.title,
                                excerpt,
                                total_chars: document.text.chars().count() as u32,
                                rendered: document.rendered,
                            })?)
                        },
                    )
                    .await;
                InvokeFinish { value, cancelled: false, stop_confirmed }
            }
        }
    }
}
