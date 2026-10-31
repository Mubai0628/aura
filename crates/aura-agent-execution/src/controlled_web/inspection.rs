use super::*;

impl ControlledWeb {
    pub(super) fn document(&self, session: SessionId, id: ArtifactId) -> Result<WebDocument> {
        let store = self.journal.lock()?;
        let bytes = aura_persistence_core::artifact::read(
            store.database.connection(),
            id,
            &format!("web-document:{session}"),
        )?;
        ensure!(bytes.len() <= 16 * 1024 * 1024, "web.document_limit");
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub fn target(
        &self,
        session: SessionId,
        kind: &str,
        action: &serde_json::Value,
    ) -> Result<url::Url> {
        // A call planned for DeepSeek goes to its search or fetch address.
        if matches!(kind, "web.search" | "web.fetch")
            && let Some(service) = web_service::frozen(action)?
        {
            return web_service::service_url(&service, kind);
        }
        Ok(match kind {
            "web.search" => {
                let input: WebSearchInput =
                    serde_json::from_value(crate::agent_execution::network::model_action(action))?;
                aura_web_runtime::search_url(&input.query, input.max_results)?
            }
            "web.fetch" => {
                let input: WebFetchInput =
                    serde_json::from_value(crate::agent_execution::network::model_action(action))?;
                ensure!((1..=8 * 1024 * 1024).contains(&input.max_bytes), "web.byte_limit");
                aura_web_runtime::normalize_url(&input.url)?
            }
            "web.read" => {
                let input: WebReadInput =
                    serde_json::from_value(crate::agent_execution::network::model_action(action))?;
                let document = self.document(session, input.document_artifact_id)?;
                aura_web_runtime::excerpt(&document.text, input.offset_chars, input.max_chars)?;
                aura_web_runtime::normalize_url(&document.source_url)?
            }
            "http.request" | "http.download" => aura_net_tools::request_url(
                action["url"].as_str().ok_or_else(|| anyhow::anyhow!("http.url_required"))?,
            )?,
            _ => anyhow::bail!("web.action_unsupported"),
        })
    }
    pub fn inspect(&self, operation: &SecuredOperation) -> Result<serde_json::Value> {
        operation.validate()?;
        let content = &operation.content;
        let identity = &content.identity;
        let package = super::package::is_package_http(content);
        ensure!(
            matches!(identity.actor, InvocationActor::ParentAgent | InvocationActor::ChildAgent)
                && (matches!(content.source, SecuritySourceRef::BuiltInTool { .. }) || package)
                && identity.agent_run_id.is_some()
                && identity.turn_task_id.is_some()
                && ((content.action_kind == "web.read"
                    && content.capabilities == [ResolvedCapability::AgentTaskRead]
                    && content.network == OperationNetwork::None)
                    || (content.capabilities.contains(&ResolvedCapability::OrdinaryNetwork)
                        && content.network == OperationNetwork::ExactOrigins)),
            "web.agent_authority_required"
        );
        let session = identity.session_id.ok_or_else(|| anyhow::anyhow!("web.session_required"))?;
        let mode = self.persistence.session_permission_state(session)?;

        ensure!(
            self.persistence
                .workspace_session_projection()?
                .sessions
                .iter()
                .any(|v| v.session_id == session && Some(v.workspace_id) == identity.workspace_id),
            "web.workspace_scope"
        );
        let url = if package {
            crate::agent_execution::package_tools::http::target(content)?
        } else {
            self.target(session, &content.action_kind, &content.action)?
        };
        if content.action_kind != "web.read" {
            ensure!(
                content.resources.contains(&aura_core_base::web_origin::resource(&url)?),
                "web.origin_binding"
            );
        }
        Ok(
            serde_json::json!({"schema":"controlled-web-preparation-v1","url":url.as_str(),"mode":mode.mode,"permission_revision":mode.revision}),
        )
    }
}
