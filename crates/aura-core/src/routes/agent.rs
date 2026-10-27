//! Entry requires the authenticated Main channel; Agent identity is allocated
//! by Core only after the user submission transaction has committed.
use crate::*;
pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    ledger: &Arc<Mutex<RequestLedger>>,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    let result: Result<serde_json::Value> = async {
        let StartupMode::Ready(services) = &state.startup_mode else {
            anyhow::bail!("core.recovery_required");
        };
        services.commands()?.set_terminal_publisher(queue.clone())?;
        // The browser host is Main; its commands go out on this connection.
        if let Ok(agent) = services.agent() {
            agent.browser().set_publisher(queue.clone());
            agent.media().set_publisher(queue.clone());
        }
        if matches!(request.method.as_str(), "media.host.read" | "media.host.result") {
            return super::media::dispatch(
                services.agent()?.as_ref(),
                &request.method,
                request.params.clone(),
            )
            .await;
        }
        // Main's answer to a browser command. Only Main reaches this route.
        if request.method == "browser.host.result" {
            let result: aura_browser_domain::BrowserHostResultV1 =
                serde_json::from_value(request.params.clone())?;
            let accepted = services.agent()?.browser().complete(result);
            return Ok(serde_json::to_value(aura_browser_domain::BrowserHostResultAck {
                accepted,
            })?);
        }
        // A screenshot of this conversation's browser calls, for the conversation to show.
        if request.method == "browser.screenshot.read" {
            let input: aura_browser_domain::BrowserScreenshotReadRequest =
                serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(services.agent()?.screenshot(input).await?)?);
        }
        // A message's attachments read again for the conversation's 文件 tab.
        if request.method == "agent.attachments.text" {
            let input: aura_agent_domain::AttachmentTextRequest =
                serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(services.agent()?.attachment_text(input).await?)?);
        }
        if request.method == "agent.attachments.locate" {
            let input: aura_agent_domain::AttachmentLocateRequest =
                serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(services.agent()?.locate_attachment(input)?)?);
        }
        // An entry of an attached zip, for the 文件 tab.
        if request.method == "agent.attachments.entry" {
            let input: aura_agent_domain::AttachmentEntryRequest =
                serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(services.agent()?.attachment_entry(input).await?)?);
        }
        // A workspace's file index walked again after it failed to build.
        if request.method == "agent.workspace.rebuild_index" {
            let input: aura_agent_domain::WorkspaceIndexRebuildRequest =
                serde_json::from_value(request.params.clone())?;
            let workspace = Arc::clone(&services.p03()?.workspace);
            let id = input.workspace_id;
            let started =
                tokio::task::spawn_blocking(move || workspace.rebuild_index(id)).await??;
            return Ok(serde_json::to_value(aura_agent_domain::WorkspaceIndexRebuildStarted {
                workspace_id: id,
                started,
            })?);
        }
        // A file outside the workspace a message names, for the 文件 tab.
        if request.method == "agent.files.mentioned" {
            let input: aura_agent_domain::MentionedFileRequest =
                serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(services.agent()?.mentioned_file(input).await?)?);
        }
        // This gate is deliberately **not** at the top of the
        // function any more.
        //
        // It used to run first and unconditionally, so a damaged tool policy also
        // refused `agent.status`, `agent.cancel`, `agent.shell.stop` and the activity
        // query — the very paths the user's confirmed rule requires to stay reachable
        // while a configuration is broken ("保留查看、停止与配置修复"). Adding every
        // method to another global allow-list would be the same mistake in the other
        // direction, so the split is made by **real dependency** below: a route that
        // reads live state, settles already-admitted work or stops it does not need
        // the configuration authority, and only a route that *starts* something does.
        //
        // The capability answer is read from the runtime's own degradation registry,
        // which the recovery entry updates, so a repaired configuration becomes usable
        // without a restart.
        if request.method.starts_with("terminal.user.") {
            if request.method == "terminal.user.open" {
                return services
                    .commands()?
                    .open_user_terminal(
                        state.options.runtime_instance_id,
                        queue.clone(),
                        request,
                        &state.terminal_openings,
                        ledger,
                    )
                    .await;
            }
            return services
                .commands()?
                .terminal_request(state.options.runtime_instance_id, queue.clone(), request)
                .await;
        }
        // 设置 → 环境, the interface settings. They start nothing and need no Agent service.
        if request.method.starts_with("ui.settings.") {
            let settings = services._settings.clone();
            let (method, params) = (request.method.clone(), request.params.clone());
            let command = request.command_invocation_id;
            return tokio::task::spawn_blocking(move || {
                super::ui_settings::dispatch(&settings, &method, params, command)
            })
            .await
            .map_err(|error| anyhow::anyhow!("ui_settings.interrupted: {error}"))?;
        }
        // 设置 → 网络与浏览器. Like the interface settings they start nothing.
        if request.method.starts_with("browser.settings.") {
            let settings = services._settings.clone();
            let (method, params) = (request.method.clone(), request.params.clone());
            let command = request.command_invocation_id;
            return tokio::task::spawn_blocking(move || {
                super::browser_settings::dispatch(&settings, &method, params, command)
            })
            .await
            .map_err(|error| anyhow::anyhow!("browser_settings.interrupted: {error}"))?;
        }
        // 设置 → 网络与浏览器 → Chrome. It starts nothing a model can reach.
        if request.method.starts_with("chrome.") {
            let command = request.command_invocation_id;
            return super::chrome::dispatch(
                &services.chrome,
                &request.method,
                request.params.clone(),
                command,
            )
            .await;
        }
        // A new conversation from one finished turn of this one.
        if request.method == "agent.session.branch" {
            return crate::session_branch::dispatch(services, request).await;
        }
        let agent = services.agent()?;
        if request.method == "agent.process.list" || request.method == "agent.process.stop" {
            let session: aura_windows_user_contract::ManagedProcessRequestV1 =
                serde_json::from_value(
                    serde_json::json!({"session_id":request.params["session_id"]}),
                )?;
            anyhow::ensure!(
                services
                    .persistence
                    .workspace_session_projection()?
                    .sessions
                    .iter()
                    .any(|v| v.session_id == session.session_id),
                "agent.session_unknown"
            );
            return if request.method == "agent.process.list" {
                let input: aura_windows_user_contract::ManagedProcessRequestV1 =
                    serde_json::from_value(request.params.clone())?;
                Ok(serde_json::to_value(
                    services.commands()?.process_list(input.session_id).await?,
                )?)
            } else {
                Ok(serde_json::to_value(
                    services
                        .commands()?
                        .process_stop(serde_json::from_value(request.params.clone())?)
                        .await?,
                )?)
            };
        }
        if request.method == "agent.tools.settings" {
            let _: aura_tool_contract::AgentToolSettingsRequest =
                serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(agent.tool_settings()?)?);
        }
        if request.method == "agent.tools.network_proxy.test" {
            return Ok(serde_json::to_value(
                agent.test_network_proxy(serde_json::from_value(request.params.clone())?).await?,
            )?);
        }
        if request.method == "agent.tools.configure" {
            let command = request
                .command_invocation_id
                .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?;
            return Ok(serde_json::to_value(
                agent
                    .update_tool_settings(command, serde_json::from_value(request.params.clone())?)
                    .await?,
            )?);
        }
        // Settings → Hook. Nothing here runs a Hook; package reads run off the reactor.
        if request.method.starts_with("agent.hooks.") {
            if aura_ipc::routes::side_effecting(&request.method) {
                anyhow::ensure!(
                    request.command_invocation_id.is_some(),
                    "ipc.command_invocation_id_required"
                );
            }
            let context = crate::hooks::HookContext::of(services);
            let (method, params) = (request.method.clone(), request.params.clone());
            return tokio::task::spawn_blocking(move || context.dispatch(&method, params))
                .await
                .map_err(|error| anyhow::anyhow!("hooks.interrupted: {error}"))?;
        }
        // Settings → MCP. Nothing here calls a server's tool; refresh and trust start a
        // server to list its tools, off the reactor.
        if request.method.starts_with("agent.mcp.") {
            if aura_ipc::routes::side_effecting(&request.method) {
                anyhow::ensure!(
                    request.command_invocation_id.is_some(),
                    "ipc.command_invocation_id_required"
                );
            }
            let context = crate::mcp::McpContext::new(Arc::clone(&services.mcp));
            let (method, params) = (request.method.clone(), request.params.clone());
            return tokio::task::spawn_blocking(move || context.dispatch(&method, params))
                .await
                .map_err(|error| anyhow::anyhow!("mcp.interrupted: {error}"))?;
        }
        // Settings → Skill. Reading folders and importing a copy run off the reactor.
        if request.method.starts_with("agent.skills.") {
            if aura_ipc::routes::side_effecting(&request.method) {
                anyhow::ensure!(
                    request.command_invocation_id.is_some(),
                    "ipc.command_invocation_id_required"
                );
            }
            let context = crate::skills::SkillContext::new(Arc::clone(&services.skills));
            let (method, params) = (request.method.clone(), request.params.clone());
            return tokio::task::spawn_blocking(move || context.dispatch(&method, params))
                .await
                .map_err(|error| anyhow::anyhow!("skills.interrupted: {error}"))?;
        }
        // Settings → 插件. Reading plugin folders and importing a copy run off the reactor;
        // nothing here runs anything a plugin brings.
        if request.method.starts_with("agent.plugins.") {
            if aura_ipc::routes::side_effecting(&request.method) {
                anyhow::ensure!(
                    request.command_invocation_id.is_some(),
                    "ipc.command_invocation_id_required"
                );
            }
            let context = crate::plugins::PluginContext::new(Arc::clone(&services.plugins));
            let (method, params) = (request.method.clone(), request.params.clone());
            return tokio::task::spawn_blocking(move || context.dispatch(&method, params))
                .await
                .map_err(|error| anyhow::anyhow!("plugins.interrupted: {error}"))?;
        }
        // The user's command menu and settings. Nothing here starts a task; the
        // directory reads run off the reactor.
        if request.method.starts_with("agent.commands.") {
            if aura_ipc::routes::side_effecting(&request.method) {
                anyhow::ensure!(
                    request.command_invocation_id.is_some(),
                    "ipc.command_invocation_id_required"
                );
            }
            let context = crate::slash_commands::CommandContext::of(services);
            let (method, params) = (request.method.clone(), request.params.clone());
            return tokio::task::spawn_blocking(move || context.dispatch(&method, params))
                .await
                .map_err(|error| anyhow::anyhow!("commands.interrupted: {error}"))?;
        }
        if matches!(
            request.method.as_str(),
            "agent.shell.query" | "agent.shell.stop" | "agent.tool_activity"
        ) {
            return super::shell::dispatch(services, request).await;
        }
        // The run-information query. It answers from the process's own index and
        // touches nothing else, so a failure here is a failed query and never a failed tool.
        if request.method == "agent.activity.query" {
            return super::activity::dispatch(services, request).await;
        }
        if request.method == "agent.approval.get" {
            return Ok(serde_json::to_value(
                agent.approval(serde_json::from_value(request.params.clone())?).await?,
            )?);
        }
        if request.method == "agent.history" {
            return Ok(serde_json::to_value(
                agent.history(serde_json::from_value(request.params.clone())?).await?,
            )?);
        }
        if request.method == "agent.compaction.status" {
            return Ok(serde_json::to_value(
                agent.compaction_status(serde_json::from_value(request.params.clone())?).await?,
            )?);
        }
        // Everything above this line is *read*, *stop* or
        // *settle* — it is answered from live state and needs no configuration
        // authority, so a damaged tool policy or an unresolved bundle cannot take the
        // user's status, stop or diagnostic paths away.
        //
        // Everything below **starts or advances Agent work**, which does read the
        // configuration authority. The gate is placed here, at the boundary, rather
        // than at the top of the function: that is what makes the split follow real
        // dependency instead of an allow-list that would rot.
        if !services.agent_capability_available() {
            // The refusal names the real blocking capability rather than a generic
            // message, and a refusal can never happen without a reason.
            let reason = services
                .agent_work_blocker()
                .map_or_else(|| "core.config_unavailable".to_owned(), |(_, reason)| reason);
            anyhow::bail!(reason);
        }
        if request.method == "agent.context.remediation" {
            return Ok(serde_json::to_value(
                agent.context_remediation(serde_json::from_value(request.params.clone())?).await?,
            )?);
        }
        if request.method == "agent.context.recheck" {
            return Ok(serde_json::to_value(
                agent
                    .recheck_context(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?,
            )?);
        }
        if request.method == "agent.compaction.request" {
            return Ok(serde_json::to_value(
                agent
                    .request_compaction(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?,
            )?);
        }
        if request.method == "agent.context.window.resolve" {
            return Ok(serde_json::to_value(
                agent
                    .resolve_context_window(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?,
            )?);
        }
        if request.method == "agent.message.text" {
            return Ok(serde_json::to_value(
                agent.message_text(serde_json::from_value(request.params.clone())?).await?,
            )?);
        }
        if request.method == "agent.inspector" {
            return Ok(serde_json::to_value(
                agent.inspector(serde_json::from_value(request.params.clone())?).await?,
            )?);
        }
        if request.method == "agent.query.select_file" {
            return Ok(serde_json::to_value(
                agent
                    .query_file(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?,
            )?);
        }
        if request.method == "agent.sessions.activity" {
            let _: aura_agent_domain::AgentSessionsActivityRequest =
                serde_json::from_value(request.params.clone())?;
            return Ok(serde_json::to_value(agent.sessions_activity().await?)?);
        }
        if request.method == "agent.result" {
            return Ok(serde_json::to_value(
                agent.result(serde_json::from_value(request.params.clone())?).await?,
            )?);
        }
        if matches!(request.method.as_str(), "agent.inbox" | "agent.submit" | "agent.inbox.mutate")
        {
            let value = match request.method.as_str() {
                "agent.inbox" => {
                    agent.inbox(serde_json::from_value(request.params.clone())?).await?
                }
                "agent.submit" => {
                    agent
                        .submit(
                            request.command_invocation_id.ok_or_else(|| {
                                anyhow::anyhow!("ipc.command_invocation_id_required")
                            })?,
                            serde_json::from_value(request.params.clone())?,
                        )
                        .await?
                }
                _ => {
                    agent
                        .mutate_inbox(
                            request.command_invocation_id.ok_or_else(|| {
                                anyhow::anyhow!("ipc.command_invocation_id_required")
                            })?,
                            serde_json::from_value(request.params.clone())?,
                        )
                        .await?
                }
            };
            return Ok(serde_json::to_value(value)?);
        }
        let value = match request.method.as_str() {
            "agent.related.start" => {
                agent
                    .related_turn(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?
            }
            "agent.start" => {
                agent
                    .start(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?
            }
            "agent.init.start" => {
                agent
                    .start_init(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?
            }
            "agent.compaction.start" => {
                agent
                    .start_compaction(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?
            }
            "agent.status" => {
                agent.status(&serde_json::from_value(request.params.clone())?).await?
            }
            "agent.cancel" => {
                agent.cancel(&serde_json::from_value(request.params.clone())?).await?
            }
            "agent.mode.control" => {
                return Ok(serde_json::to_value(
                    agent
                        .control_mode(
                            request.command_invocation_id.ok_or_else(|| {
                                anyhow::anyhow!("ipc.command_invocation_id_required")
                            })?,
                            serde_json::from_value(request.params.clone())?,
                        )
                        .await?,
                )?);
            }
            // The user touched the waiting question; its countdown starts again.
            "agent.query.touch" => {
                return Ok(serde_json::to_value(
                    agent.touch_query(serde_json::from_value(request.params.clone())?).await?,
                )?);
            }
            "agent.query.answer" => {
                agent
                    .answer(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?
            }
            "agent.approval.resolve" => {
                agent
                    .resolve_approval(
                        request
                            .command_invocation_id
                            .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?,
                        serde_json::from_value(request.params.clone())?,
                    )
                    .await?
            }
            _ => anyhow::bail!("ipc.route_unknown"),
        };
        Ok(serde_json::to_value(value)?)
    }
    .await;
    let (terminal, response) = match result {
        Ok(value) => (IpcRequestTerminal::Succeeded, JsonRpcResponse::success(request.id, value)),
        Err(error) if error.is::<command_execution::terminal_opening::OpeningCancelled>() => (
            IpcRequestTerminal::Cancelled,
            JsonRpcResponse::failure(
                request.id,
                IpcErrorEnvelope {
                    code: ErrorCode::new("terminal.open_cancelled")?,
                    category: ErrorCategory::Cancelled,
                    message: error.to_string(),
                    retryable: false,
                    correlation_id: request.id,
                    terminal: IpcRequestTerminal::Cancelled,
                    details: BTreeMap::new(),
                },
            ),
        ),
        Err(error) => (
            IpcRequestTerminal::Failed,
            JsonRpcResponse::failure(
                request.id,
                IpcErrorEnvelope {
                    code: ErrorCode::new("agent.request_rejected")?,
                    category: ErrorCategory::Validation,
                    message: error.to_string(),
                    retryable: false,
                    correlation_id: request.id,
                    terminal: IpcRequestTerminal::Failed,
                    details: BTreeMap::new(),
                },
            ),
        ),
    };
    send_recorded_response(state, queue, request, response).await?;
    Ok(terminal)
}
