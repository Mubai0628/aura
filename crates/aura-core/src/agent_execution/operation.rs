use super::*;
use anyhow::{Result, ensure};
use aura_domain_common::jcs::canonical_sha256;
use aura_tool_registry::agent_release::{AgentBuiltinEntry, AgentBuiltinHandler as H, release};

/// Whether a frozen call names one of the Agent release's built-in tools rather than an
/// installed package's or an MCP server's tool.
pub(super) fn is_builtin(definition: &aura_tool_domain::ToolDefinitionRecordV1) -> bool {
    definition.tool_package_id.is_none()
        && definition.source_kind != aura_tool_contract::ToolSourceKindV1::Mcp
}

pub(super) fn entry(call: &ValidatedToolCallV1) -> Result<&'static AgentBuiltinEntry> {
    let entry = release()?
        .entry(call.definition.canonical_name.as_str())
        .ok_or_else(|| anyhow::anyhow!("agent.builtin_unavailable"))?;
    ensure!(
        entry.definition == call.definition
            && entry.instance.tool_instance_id == call.binding.tool_instance_id
            && entry.instance.tool_version == call.binding.tool_version
            && entry.instance.source_contract_fingerprint
                == call.binding.source_contract_fingerprint,
        "agent.release_changed"
    );
    Ok(entry)
}
impl AgentExecution {
    /// Resolves the frozen call into its operation, from durable facts only.
    ///
    /// Contract: this function takes `journal.lock()` directly (lines around the run/turn
    /// load), so it is only correct while the dynamic path that reaches it is a real blocking
    /// thread. It therefore stays synchronous and **every** call site must already be inside one
    /// bounded section. The two call sites are:
    /// * `admission.rs::admit_output` - the whole synchronous admission body runs on the one
    ///   bounded Agent Store owner (`AgentExecution::admit_output`);
    /// * `operation.rs::verify_source` - whose own call sites are each inside a bounded section
    ///   (`broker.rs::Port::inspect_sync` and `Port::invoke`, `dispatch.rs::execute_call`,
    ///   `control.rs::execute_control`, `reservation.rs::Reservation::reserve`,
    ///   `web.rs::WebBackend::{prepare,execute}`, and the `broker.rs:146` `spawn_blocking`
    ///   closure for the fs path). The final inventory in the batch root lists them one by one.
    pub fn resolve_call(
        &self,
        batch: &ToolBatchRequestV1,
        registry: &ToolRegistrySnapshotV1,
        call: &ValidatedToolCallV1,
        snapshot: &PermissionSnapshot,
    ) -> Result<CallContext> {
        if call.definition.tool_package_id.is_some() {
            return self.resolve_package_call(batch, registry, call, snapshot);
        }
        if call.definition.source_kind == aura_tool_contract::ToolSourceKindV1::Mcp {
            return self.resolve_mcp_call(batch, registry, call, snapshot);
        }
        let entry = entry(call)?;
        let id = &batch.invocation_identity;
        ensure!(
            batch.calls.contains(&call.call)
                && batch.registry_snapshot_ref == registry.snapshot_ref
                && snapshot.session_id == id.session_id,
            "agent.call_identity"
        );
        let store = self.journal.lock()?;
        let run = aura_persistence_v2::agent::turns::load_run(
            store.database.connection(),
            id.agent_run_id,
        )?;
        let turn =
            aura_persistence_v2::agent::turns::load(store.database.connection(), id.turn_task_id)?;
        ensure!(
            run.turn_task_id == turn.turn_task_id
                && turn.agent_run_id == run.agent_run_id
                && turn.session_id == id.session_id
                && turn.workspace_id == snapshot.workspace_id
                && !turn.state.terminal(),
            "agent.live_identity"
        );
        drop(store);
        let (purpose, arguments) = match &call.call.input {
            NormalizedToolCallInputStateV1::Ready { purpose, arguments, .. } => {
                (purpose.to_string(), arguments.clone())
            }
            _ => anyhow::bail!("agent.invalid_input"),
        };
        let source =
            aura_tool_scheduler::security_source_for_admitted_tool_call_v1(call, registry)?;
        let mut action = arguments;
        let mut network = OperationNetwork::None;
        let mut risks = vec![];
        let mut compatibility = Default::default();
        let workspace_id = snapshot.workspace_id;
        // The declared capabilities are the tool's own; an external target replaces the
        // workspace file capabilities with the external ones, so the permission engine
        // decides with the authority that is really needed and the file layer is asked
        // for the same thing it will enforce.
        let mut capabilities = entry.definition.capabilities.clone();
        let task = ResourceDescriptor::AgentTask {
            session_id: id.session_id,
            turn_task_id: id.turn_task_id,
            agent_run_id: id.agent_run_id,
        };
        let resources = match entry.handler {
            H::MemorySearch
            | H::MemoryRead
            | H::MemoryRecent
            | H::MemoryCreate
            | H::MemoryUpdate
            | H::MemoryLink
            | H::MemoryForget
            | H::MemoryArchive
            | H::MemoryDelete
            | H::MemoryValidate
            | H::MemoryRepair => {
                if entry.handler == H::MemoryRepair && action["phase"] == "apply" {
                    capabilities = vec![ResolvedCapability::MemoryWrite];
                }
                let (resource, normalized) =
                    super::memory::resolve(self, snapshot, entry.handler.name(), &action)?;
                action = normalized;
                vec![resource]
            }
            H::GitSync | H::GitPush => {
                network = OperationNetwork::BroadProcess;
                if entry.handler == H::GitSync && action["kind"] == "pull" {
                    capabilities.push(ResolvedCapability::GitWrite);
                }
                if entry.handler == H::GitPush && action["force_with_lease"].is_string() {
                    capabilities.push(ResolvedCapability::GitDestructive);
                }
                super::git::remote_resources(self, snapshot, entry.handler.name(), &action)?
            }
            H::GitStatus | H::GitDiff | H::GitLog | H::GitCommit | H::GitBranch => {
                vec![ResourceDescriptor::GitRepository { workspace_id }]
            }
            H::WorkspaceInitComplete => {
                self.resolve_workspace_init(&mut action, &capabilities, batch, snapshot)?
            }
            H::CommandsList | H::CommandsDescribe | H::CommandsSave | H::CommandsDelete => self
                .resolve_command_tool(entry.handler, &mut action, &capabilities, batch, snapshot)?,
            H::FileRead
            | H::FileList
            | H::FileWrite
            | H::FilePatch
            | H::FileDelete
            | H::FileFind
            | H::FileSearch => self.resolve_file_target(
                entry.handler,
                &mut action,
                &mut capabilities,
                batch,
                snapshot,
            )?,
            H::MediaProbe | H::MediaFrames => super::media::resolve(
                self,
                entry.handler,
                &mut action,
                &mut capabilities,
                batch,
                snapshot,
            )?,
            H::TerminalOpen => {
                let mut input: aura_windows_user_contract::AgentTerminalOpenInput =
                    serde_json::from_value(action)?;
                check_secret_environment(&input.secret_environment)?;
                let relative = WorkspacePath::parse(&input.working_directory)?;
                let resolved =
                    self.workspace.resolver(workspace_id)?.resolve_existing(&relative)?;
                ensure!(
                    resolved.info.is_directory,
                    "terminal.working_directory: choose a workspace directory"
                );
                input.working_directory = resolved.absolute_path.to_string_lossy().into_owned();
                if input.requires_network || snapshot.mode == PermissionMode::FullAccess {
                    network = OperationNetwork::BroadProcess;
                    capabilities.push(ResolvedCapability::OrdinaryNetwork);
                }
                action = serde_json::to_value(input)?;
                compatibility = [ExecutionCompatibility::ProcessTreeCancellation].into();
                vec![ResourceDescriptor::WorkspaceRoot { workspace_id }]
            }
            H::TerminalWrite => {
                let input: aura_windows_user_contract::AgentTerminalWriteInput =
                    serde_json::from_value(action.clone())?;
                // What is typed is read like a script; where the terminal stands is not known,
                // so only places the text names in full are checked against the workspace.
                let root = self.workspace.resolver(workspace_id)?.canonical_root().to_path_buf();
                let found = super::shell_risks::of_script(
                    aura_windows_user_contract::ManagedShell::PowerShell,
                    &input.input,
                    None,
                    &[root],
                );
                let record = aura_persistence_v2::terminal::get(
                    self.journal.lock()?.database.connection(),
                    id.session_id,
                    &input.terminal_id,
                )?;
                if record.requires_network {
                    network = OperationNetwork::BroadProcess;
                    capabilities.push(ResolvedCapability::OrdinaryNetwork);
                }
                shell_risks(&found, &mut risks, &mut capabilities, &mut network);
                compatibility = [ExecutionCompatibility::ProcessTreeCancellation].into();
                vec![ResourceDescriptor::WorkspaceRoot { workspace_id }]
            }
            H::PowerShell | H::Cmd => {
                let input: aura_windows_user_contract::ShellExecuteInput =
                    serde_json::from_value(action)?;
                check_secret_environment(&input.secret_environment)?;
                let relative = WorkspacePath::parse(&input.working_directory)?;
                let resolved =
                    self.workspace.resolver(workspace_id)?.resolve_existing(&relative)?;
                ensure!(resolved.info.is_directory, "agent.shell_directory");
                let shell = if entry.handler == H::PowerShell {
                    aura_windows_user_contract::ManagedShell::PowerShell
                } else {
                    aura_windows_user_contract::ManagedShell::Cmd
                };
                let resolver = self.workspace.resolver(workspace_id)?;
                let found = super::shell_risks::of_script(
                    shell,
                    &input.script,
                    Some(&resolved.absolute_path),
                    &[resolver.canonical_root().to_path_buf()],
                );
                let mut command = input.command(shell)?;
                command.working_directory = resolved.absolute_path.to_string_lossy().into_owned();
                action = serde_json::to_value(command)?;
                compatibility = [ExecutionCompatibility::ProcessTreeCancellation].into();
                if input.requires_network || snapshot.mode == PermissionMode::FullAccess {
                    network = OperationNetwork::BroadProcess;
                    capabilities.push(ResolvedCapability::OrdinaryNetwork);
                }
                shell_risks(&found, &mut risks, &mut capabilities, &mut network);
                vec![ResourceDescriptor::WorkspaceRoot { workspace_id }]
            }
            H::WebSearch | H::WebFetch => {
                network = OperationNetwork::ExactOrigins;
                // DeepSeek's search or fetch, when the settings choose it, is decided here
                // and frozen into the call, so its request, resource and key never change after.
                // Only Core writes the field: the closed input schema refuses a model argument by
                // that name, and one is dropped here all the same.
                use crate::controlled_web::web_service::WEB_SERVICE_FIELD;
                if let Some(object) = action.as_object_mut() {
                    object.remove(WEB_SERVICE_FIELD);
                }
                if let Some(service) = self.web.deepseek_service(entry.handler.name(), &action)? {
                    action[WEB_SERVICE_FIELD] = serde_json::to_value(service)?;
                }
                let url = self.web.target(id.session_id, entry.handler.name(), &action)?;
                vec![crate::controlled_web::ControlledWeb::resource(&url)?]
            }
            H::WebRead => {
                self.web.target(id.session_id, entry.handler.name(), &action)?;
                vec![task]
            }
            H::HttpRequest | H::HttpDownload => {
                network = OperationNetwork::ExactOrigins;
                let url = aura_net_tools::request_url(
                    action["url"].as_str().ok_or_else(|| anyhow::anyhow!("http.url_required"))?,
                )?;
                let mut resources = if entry.handler == H::HttpDownload {
                    self.resolve_download(&mut action, &mut capabilities, batch, snapshot)?
                } else {
                    vec![]
                };
                resources.push(crate::controlled_web::ControlledWeb::resource(&url)?);
                resources
            }
            H::BrowserOpen
            | H::BrowserNavigate
            | H::BrowserSnapshot
            | H::BrowserScreenshot
            | H::BrowserClick
            | H::BrowserType
            | H::BrowserPress
            | H::BrowserScroll
            | H::BrowserHover
            | H::BrowserSelect
            | H::BrowserWait
            | H::BrowserConsole
            | H::BrowserClose
            | H::BrowserRequestUser
            | H::BrowserEvaluate
            | H::BrowserNetwork
            | H::BrowserInspect
            | H::BrowserPerformance => {
                let resolved =
                    self.resolve_browser(entry.handler, &mut action, &mut capabilities, snapshot)?;
                network = resolved.network;
                risks = resolved.risks;
                resolved.resources
            }
            H::ChromeOpen
            | H::ChromeNavigate
            | H::ChromeSnapshot
            | H::ChromeScreenshot
            | H::ChromeClick
            | H::ChromeType
            | H::ChromePress
            | H::ChromeScroll
            | H::ChromeHover
            | H::ChromeSelect
            | H::ChromeWait
            | H::ChromeConsole
            | H::ChromeClose
            | H::ChromeRequestUser => {
                network = OperationNetwork::None;
                self.resolve_chrome(
                    entry.handler,
                    call.call.tool_call_id,
                    &mut action,
                    &mut capabilities,
                    snapshot,
                )?
                .resources
            }
            H::Registry(handler) => {
                let mut resources = vec![ResourceDescriptor::ToolRegistrySnapshot {
                    resource_fingerprint: canonical_sha256(&registry.snapshot_ref)?,
                }];
                if matches!(
                    handler,
                    aura_tool_registry::BuiltinCoreHandlerV1::ToolsLoad
                        | aura_tool_registry::BuiltinCoreHandlerV1::ToolsUnload
                ) {
                    resources.push(ResourceDescriptor::TurnTaskToolExposure {
                        session_id: id.session_id,
                        turn_task_id: id.turn_task_id,
                        agent_run_id: id.agent_run_id,
                    });
                }
                resources
            }
            _ => vec![task],
        };
        // Deleting for good skips the Recycle Bin; it is asked every time, in every mode.
        if entry.handler == H::FileDelete && action["permanent"].as_bool() == Some(true) {
            risks.push(OperationRiskFlag::PermanentFileDeletion);
        }
        let mut content = OperationContent {
            identity: InvocationIdentity {
                principal_id: PrincipalId::from_bytes(id.session_id.to_bytes()),
                // UNWIRED: no product path creates a run with a parent, so ChildAgent is never
                // produced here.
                actor: if run.parent_agent_run_id.is_some() {
                    InvocationActor::ChildAgent
                } else {
                    InvocationActor::ParentAgent
                },
                workspace_id: Some(workspace_id),
                session_id: Some(id.session_id),
                turn_task_id: Some(id.turn_task_id),
                agent_run_id: Some(id.agent_run_id),
                parent_agent_run_id: run.parent_agent_run_id,
                correlation_id: CorrelationId::new(),
                permission_snapshot_id: Some(snapshot.id),
            },
            source: source.security_source_ref(),
            source_revision_fingerprint: Some(ContentHash::new(
                ContentHashAlgorithm::Sha256,
                entry.definition.definition_fingerprint.hex().to_owned(),
            )?),
            // The classified capabilities, not the tool's declared ones: an external
            // target replaced the workspace file capability with the external one, and
            // the permission engine has to decide with the authority the target really
            // needs. Copying the definition here would silently authorize an external
            // write under a workspace capability.
            capabilities,
            resources,
            risks,
            credentials: vec![],
            purpose: Some(purpose),
            network,
            compatibility,
            action_kind: entry.handler.name().into(),
            action_revision: entry.definition.tool_version.definition_revision,
            action,
        };
        if content.network == OperationNetwork::ExactOrigins {
            network::bind(&self.persistence, &mut content)?;
        }
        content.action = stored_form(&content.action)?;
        let operation = SecuredOperation::new(SecurityOperationId::new(), content)?;
        let verified_external_capabilities = self.external_authority(&operation);
        let verified_external_grants = self.external_authority_slices(&operation);
        Ok(CallContext {
            batch: batch.clone(),
            registry: registry.clone(),
            validated: call.clone(),
            snapshot: snapshot.clone(),
            operation,
            verified_external_capabilities,
            verified_external_grants,
            observed_resources: Arc::new(std::sync::Mutex::new(Vec::new())),
        })
    }
    /// Re-derives the call from durable facts and refuses if anything moved.
    ///
    /// Contract: it calls [`Self::resolve_call`] and then takes `journal.lock()` itself, so
    /// it is a synchronous critical section and must only be reached from inside a bounded section.
    /// Every caller does so; `broker.rs::Port::invoke`'s fs branch is inside the pre-existing
    /// `tokio::task::spawn_blocking` closure, and all other callers run their enclosing section on
    /// the one bounded Agent Store owner.
    pub(crate) fn verify_source(&self, call: &CallContext) -> Result<()> {
        let expected =
            self.resolve_call(&call.batch, &call.registry, &call.validated, &call.snapshot)?;
        // Allocated operation/correlation IDs are not resolver output. All action,
        // source, scope and mode inputs must otherwise remain byte-for-byte equal.
        let mut content = expected.operation.content;
        content.identity.correlation_id = call.operation.content.identity.correlation_id;
        // A directory approval mints a grant before this frozen call resumes. The fresh
        // classification then names that grant. The user approved the frozen operation,
        // so that adoption is not a scope change. Revocation or expiry of the bound
        // grant still stops the call. Any other difference remains a scope change.
        let fingerprint = call.operation.fingerprint.0.hex();
        if !self.external_grants.bound_directory_grants_in_force(fingerprint)? {
            anyhow::bail!("agent.directory_grant_not_in_force");
        }
        // Folders created since along a new file's path (another write of the same response,
        // typically) anchor its target deeper; that is not a scope change.
        if content != call.operation.content
            && !directory_grant_adoption(&call.operation.content, &content)
            && !self.folders_created_since(&call.operation.content, &content)?
        {
            anyhow::bail!("agent.source_or_scope_changed");
        }
        // The journal owns the frozen call and its task identity. The coordinator
        // owns mutable tool enablement; validated Core releases provide Built-In
        // instances. Independently minted scope-head and execution-snapshot IDs
        // cannot establish whether this call is still legitimate.
        let store = self.journal.lock()?;
        let db = store.database.connection();
        ensure!(
            aura_persistence_v2::tool::registry(
                db,
                call.registry.snapshot_ref.tool_registry_snapshot_id
            )?
            .as_deref()
                == Some(&call.registry),
            "agent.registry_changed"
        );
        // Use this section's existing connection: no second journal acquisition,
        // await, or nested admission while verifying ownership.
        let ownership = self.store.verify_scope_ownership_on(db, &call.registry.scope);
        ensure!(ownership.permits_new_work(), "agent.scope_refused: {}", ownership.code());
        // `resolve_call` has already verified this exact definition, version,
        // instance and source fingerprint against the current Agent release.
        // Only the tool being dispatched is relevant: disabling an unused member
        // of the frozen registry must not refuse this otherwise valid call.
        if call.validated.definition.tool_package_id.is_some() {
            // A package tool's switch is its package's enabled state. The live install
            // must still be trusted, enabled and derive exactly the frozen definition.
            super::package_tools::installed_tool_for(db, &call.validated.definition)?;
        } else if !is_builtin(&call.validated.definition) {
            // An MCP tool's switch is its server's. The server must still be trusted, on
            // and unchanged, and list exactly the frozen definition.
            let runtime = super::mcp_tools::runtime(self)?;
            super::mcp_tools::tool_for(&runtime, db, &call.validated.definition)?;
        } else {
            let dispatched = std::collections::BTreeSet::from([call.validated.binding.tool_id]);
            let disabled = self
                .persistence
                .tools_not_enabled_under_scope(&call.registry.scope, &dispatched)
                .map_err(|error| {
                    anyhow::anyhow!("agent.registry_authority_unavailable: {error}")
                })?;
            ensure!(
                disabled.is_empty(),
                "agent.tool_disabled: {}",
                disabled.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
            );
        }
        let run = aura_persistence_v2::agent::turns::load_run(
            db,
            call.batch.invocation_identity.agent_run_id,
        )?;
        ensure!(
            matches!(run.state, aura_agent_domain::AgentRunState::ToolsRunning { tool_batch_id } if tool_batch_id == call.batch.tool_batch_id),
            "agent.not_running_batch"
        );
        let stored =
            aura_persistence_v2::tool::execution::call(db, call.validated.call.tool_call_id)?;
        ensure!(
            stored.call == call.validated.call && stored.result.is_none(),
            "agent.call_already_finished"
        );
        Ok(())
    }
}

/// The fresh resolution adopted the directory grant this approval just minted, and
/// nothing else in the frozen operation changed.
fn directory_grant_adoption(
    frozen: &aura_execution_contract::OperationContent,
    fresh: &aura_execution_contract::OperationContent,
) -> bool {
    let fresh_is_grant = !fresh.resources.is_empty()
        && fresh.resources.iter().all(|resource| {
            matches!(resource, aura_security_contract::ResourceDescriptor::ExternalGrant { .. })
        });
    let frozen_is_directory = frozen.resources.iter().any(|resource| {
        matches!(
            resource,
            aura_security_contract::ResourceDescriptor::ExternalPath { directory: true, .. }
        )
    });
    if !fresh_is_grant || !frozen_is_directory {
        return false;
    }
    let mut adjusted = fresh.clone();
    adjusted.resources.clone_from(&frozen.resources);
    adjusted.action.clone_from(&frozen.action);
    adjusted == *frozen
}

/// The action as the store gives it back. The store keeps canonical JSON, where a whole number a
/// typed input wrote as a float (`280.0`) reads back as `280`, and the reservation refuses a call
/// whose operation reads back different: media frames at 15 s, browser clicks at x 280, y 330. Every tool's action takes that form before it is sealed.
pub(super) fn stored_form<T: serde::Serialize>(input: &T) -> Result<serde_json::Value> {
    let canonical = aura_domain_common::jcs::canonical_json_jcs(input)
        .map_err(|_| anyhow::anyhow!("agent.tool_input_invalid"))?;
    Ok(serde_json::from_slice(&canonical)?)
}

/// Below 完全访问 a Shell deletion asks every time, a download once per session (its capability
/// is what the session answer covers) and a write outside the workspace every time.
/// A command may declare only credentials the user's environment really has, and never one of
/// Aura's own; anything else is refused here, before an approval is asked or anything runs.
fn check_secret_environment(names: &[String]) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    ensure!(
        aura_windows_user_contract::valid_secret_names(names),
        "shell.secret_environment: name at most 32 variables, each by its plain name"
    );
    let trusted = aura_windows_platform::current_user_environment()?;
    aura_backend_windows_user::compat::environment::check_declared(&trusted, names)
        .map_err(|why| anyhow::anyhow!("shell.secret_environment: {why}"))
}

fn shell_risks(
    found: &super::shell_risks::ShellRisks,
    risks: &mut Vec<OperationRiskFlag>,
    capabilities: &mut Vec<ResolvedCapability>,
    network: &mut OperationNetwork,
) {
    if found.deletes {
        risks.push(OperationRiskFlag::PermanentFileDeletion);
    }
    // A download uses the network, whatever the script declared.
    if found.downloads {
        *network = OperationNetwork::BroadProcess;
        for capability in
            [ResolvedCapability::OrdinaryNetwork, ResolvedCapability::DownloadArtifact]
        {
            if !capabilities.contains(&capability) {
                capabilities.push(capability);
            }
        }
    }
    if !found.outside.is_empty() {
        risks.push(OperationRiskFlag::OutsideWorkspaceWrite);
    }
}

#[cfg(test)]
mod secret_environment_tests {
    #[test]
    fn a_declaration_the_environment_cannot_satisfy_is_refused_before_anything_runs() {
        assert!(super::check_secret_environment(&[]).is_ok(), "declaring nothing changes nothing");
        let missing = super::check_secret_environment(&["ZZ_NEVER_SET_FIXTURE_TOKEN".into()])
            .expect_err("missing");
        assert!(
            missing.to_string().starts_with(
                "shell.secret_environment: ZZ_NEVER_SET_FIXTURE_TOKEN is not set in the user's"
            ),
            "{missing}"
        );
        let invalid = super::check_secret_environment(&["A=B".into()]).expect_err("invalid");
        assert!(invalid.to_string().contains("plain name"), "{invalid}");
    }
}
