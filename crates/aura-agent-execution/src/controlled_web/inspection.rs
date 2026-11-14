use super::*;

// Refusals the web tools share: the code, what happened, and what to do next.
pub(super) const SESSION_REQUIRED: &str = "web.session_required: this call does not belong to a \
    conversation, so Aura did not run it. Nothing was sent.";
pub(super) const AGENT_AUTHORITY_REQUIRED: &str = "web.agent_authority_required: this call was not \
    made by an agent's turn with the access web tools need, so Aura did not run it. Nothing was \
    sent. Call it again from the conversation.";
pub(super) const PREPARATION_CHANGED: &str = "web.preparation_changed: the permission mode, the \
    workspace or the address changed after the call was prepared. Nothing was sent. Call it again.";
pub(super) const MODE_CHANGED_BEFORE_START: &str = "web.mode_changed_before_start: the permission \
    mode or the saved page changed before web.read started. Nothing was read. Call it again.";
const DOCUMENT_LIMIT: &str = "web.document_limit: the saved page is larger than 16 MiB, more than \
    web.read reads. Nothing was read. Fetch a more specific address with web.fetch.";
const DOCUMENT_MISSING: &str = "web.document_missing: there is no saved page with this \
    document_artifact_id in this conversation. Nothing was read. Use a document_artifact_id that \
    web.fetch returned in this conversation, or fetch the page again.";

/// Arguments that do not fit the tool's input. The schema check before the call refuses these
/// first; serde's words about the field follow Core's sentence, bounded.
pub(super) fn input_error(kind: &str, error: &serde_json::Error) -> anyhow::Error {
    let code = match kind {
        "web.search" => "web.search_input",
        "web.read" => "web.read_input",
        _ => "web.fetch_input",
    };
    anyhow::anyhow!(
        "{code}: the arguments do not fit {kind}: {}. The call did not run; correct the arguments \
         and call it again.",
        detail(error)
    )
}

/// Another program's words after Core's sentence: the first line, at most 160 bytes, without
/// control characters or braces.
pub(super) fn detail(text: &dyn std::fmt::Display) -> String {
    let text = text.to_string();
    let mut detail = String::new();
    for c in text.lines().next().unwrap_or_default().trim().chars() {
        if detail.len() + c.len_utf8() > 160 {
            detail.push('…');
            break;
        }
        detail.push(if c.is_control() || c == '{' || c == '}' { ' ' } else { c });
    }
    detail
}

impl ControlledWeb {
    pub(super) fn document(&self, session: SessionId, id: ArtifactId) -> Result<WebDocument> {
        let store = self.journal.lock()?;
        let bytes = match aura_persistence_core::artifact::read(
            store.database.connection(),
            id,
            &format!("web-document:{session}"),
        ) {
            // The page is not this conversation's, or there is none by that id.
            Err(aura_persistence_core::PersistenceCoreError::Invalid("artifact access")) => {
                anyhow::bail!(DOCUMENT_MISSING)
            }
            read => read?,
        };
        ensure!(bytes.len() <= 16 * 1024 * 1024, DOCUMENT_LIMIT);
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
        let input = crate::agent_execution::network::model_action(action);
        Ok(match kind {
            "web.search" => {
                let input: WebSearchInput =
                    serde_json::from_value(input).map_err(|e| input_error(kind, &e))?;
                aura_web_runtime::search_url(&input.query, input.max_results)?
            }
            "web.fetch" => {
                let input: WebFetchInput =
                    serde_json::from_value(input).map_err(|e| input_error(kind, &e))?;
                ensure!(
                    (1..=8 * 1024 * 1024).contains(&input.max_bytes),
                    "web.byte_limit: max_bytes must be between 1 and 8388608 (8 MiB). Nothing was \
                     fetched."
                );
                aura_web_runtime::normalize_url(&input.url)?
            }
            "web.read" => {
                let input: WebReadInput =
                    serde_json::from_value(input).map_err(|e| input_error(kind, &e))?;
                let document = self.document(session, input.document_artifact_id)?;
                aura_web_runtime::excerpt(&document.text, input.offset_chars, input.max_chars)?;
                aura_web_runtime::normalize_url(&document.source_url)?
            }
            "http.request" | "http.download" => {
                aura_net_tools::request_url(action["url"].as_str().ok_or_else(|| {
                    anyhow::anyhow!(
                        "http.url_required: 没有给出 url，请求没有发出。请在 url 里写出完整的 http:// 或 https:// 地址。"
                    )
                })?)?
            }
            _ => anyhow::bail!(
                "web.action_unsupported: the web tools do not run this kind of call. Nothing was sent."
            ),
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
            AGENT_AUTHORITY_REQUIRED
        );
        let session = identity.session_id.ok_or_else(|| anyhow::anyhow!(SESSION_REQUIRED))?;
        let mode = self.persistence.session_permission_state(session)?;

        ensure!(
            self.persistence
                .workspace_session_projection()?
                .sessions
                .iter()
                .any(|v| v.session_id == session && Some(v.workspace_id) == identity.workspace_id),
            "web.workspace_scope: this conversation is no longer in the workspace the call was made \
             for, so Aura did not run it. Nothing was sent. Call it again."
        );
        let url = if package {
            crate::agent_execution::package_tools::http::target(content)?
        } else {
            self.target(session, &content.action_kind, &content.action)?
        };
        if content.action_kind != "web.read" {
            ensure!(
                content.resources.contains(&aura_core_base::web_origin::resource(&url)?),
                "web.origin_binding: the call's address no longer matches the site it was \
                 approved for, so Aura did not run it. Nothing was sent. Call it again."
            );
        }
        Ok(
            serde_json::json!({"schema":"controlled-web-preparation-v1","url":url.as_str(),"mode":mode.mode,"permission_revision":mode.revision}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each shared refusal is one bounded line that starts with its code; arguments that do not
    /// fit name the tool and serde's words about the field.
    #[test]
    fn a_web_refusal_names_its_cause() {
        for (line, code) in [
            (SESSION_REQUIRED, "web.session_required"),
            (AGENT_AUTHORITY_REQUIRED, "web.agent_authority_required"),
            (PREPARATION_CHANGED, "web.preparation_changed"),
            (MODE_CHANGED_BEFORE_START, "web.mode_changed_before_start"),
            (DOCUMENT_LIMIT, "web.document_limit"),
            (DOCUMENT_MISSING, "web.document_missing"),
        ] {
            assert!(line.starts_with(&format!("{code}: ")), "{line}");
            assert!(line.len() <= 512 && !line.contains(['\n', '{', '}']), "{line}");
            assert!(!line.contains("  "), "one space between words: {line}");
        }
        let serde = serde_json::from_value::<WebFetchInput>(serde_json::json!({"max_bytes": 1}))
            .expect_err("no url");
        let line = input_error("web.fetch", &serde).to_string();
        assert!(line.starts_with("web.fetch_input: the arguments do not fit web.fetch: "));
        assert!(line.contains("url") && !line.contains('\n'), "{line}");
        let search = input_error("web.search", &serde).to_string();
        assert!(search.starts_with("web.search_input: "), "{search}");
        let long = detail(&format!("{{x}}{}\nsecond", "中".repeat(100)));
        assert!(long.len() <= 163 && !long.contains(['{', '}']) && !long.contains("second"));
    }
}
