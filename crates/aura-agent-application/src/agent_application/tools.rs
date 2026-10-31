//! One catalog and configuration source for settings and model exposure.
use super::*;
use aura_agent_execution::{
    agent_execution::{self, mcp_tools, package_tools},
    controlled_web,
};
use aura_config::{ConfigModule, FIXED_TOOL_IDS_FIELD, MAX_DYNAMIC_TOOL_SCHEMAS_FIELD};
use aura_persistence_v2::tool::{self, exposure, session_tools};
use aura_tool_contract::*;
use aura_tool_domain::ToolEphemeralLoadRecordV1;
use aura_tool_registry::{ParentRegistryBuildInputV1, build_retained_parent_registry_snapshot};
use std::collections::BTreeSet;
mod proxy;
use aura_core_broker::agent_store;
use aura_git_service::git;
use proxy::{proxy_recovery, proxy_unchanged, proxy_view};

pub(super) struct RegistryCache {
    key: jcs::CanonicalHashV1,
    registry: ToolRegistrySnapshotV1,
}
fn required(name: &str) -> bool {
    [AgentTaskMode::Normal, AgentTaskMode::Plan, AgentTaskMode::Goal].into_iter().any(|mode| {
        [
            AgentTaskPurpose::Conversation,
            AgentTaskPurpose::WorkspaceInit,
            AgentTaskPurpose::Compaction,
        ]
        .into_iter()
        .any(|purpose| super::context::core_tool(name, mode, purpose))
    })
}

impl AgentApplication {
    /// Applies a committed strategy snapshot to the global capacity, refusing to
    /// move backwards to an older revision.
    ///
    /// A read that started before a concurrent save can finish after it; without
    /// this guard a late old snapshot would silently restore the previous limit.
    /// Applies the committed strategy: the watermark check and the capacity write are
    /// one serialized operation.
    ///
    /// Two readers can read the committed settings and arrive here at the same time —
    /// the settings getter is not held by the save lock — so a check-then-apply with
    /// only an atomic watermark lets an older read apply its limit after a newer one
    /// and then store its older revision back over the newer. Holding this lock across
    /// both steps is what makes the committed revision and the applied capacity agree;
    /// a compare-and-set on the watermark alone would still leave the capacity
    /// assignment outside the decision.
    ///
    /// `pub(crate)` so a regression can drive the exact interleaving: read a snapshot,
    /// let a newer save commit, then hand this function the older snapshot.
    pub fn apply_committed_strategy(
        &self,
        settings: &ToolSettingsSnapshotV1,
    ) -> anyhow::Result<()> {
        use std::sync::atomic::Ordering;
        let incoming = settings.revision.value();
        let _guard = self
            .strategy_apply_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("tools.configuration_unavailable"))?;
        if incoming <= self.applied_strategy_revision.load(Ordering::Acquire) {
            // Nothing newer to apply. Not an error: a reader that lost the race still
            // answered its caller, and the newest capacity is already in force.
            return Ok(());
        }
        // Only a successful apply advances the watermark, so a rejected value
        // cannot block a later valid revision.
        self.execution.set_global_tool_concurrency(settings.max_concurrent_tool_calls)?;
        self.applied_strategy_revision.store(incoming, Ordering::Release);
        Ok(())
    }
}

/// The tools loaded for a session whose configuration was never customized.
///
/// Every name here must exist in the built-in release and must not be a
/// mode-required control tool: `required` tools are always loaded by their own
/// rule and are not selectable, so listing one would make the whole default set
/// invalid. Loading a tool only gives the model its definition; it never widens
/// execution permission, and it does not consume a session's dynamic allowance.
///
/// The order is lexicographic because the settings contract requires a sorted,
/// unique id list; the groups below are what the selection covers:
///
/// - file work: `fs.delete`, `fs.find`, `fs.list`, `fs.patch`, `fs.read`, `fs.search`, `fs.write`
/// - managed Shell: `shell.powershell`, `shell.query`, `shell.stop`
/// - task bookkeeping: `todo.get`, `todo.replace`, `todo.update`
/// - this conversation's history: `history.list_turns`, `history.read`, `history.search`
/// - Aura's browser: `browser.click`, `browser.open`, `browser.press`, `browser.screenshot`,
///   `browser.snapshot`, `browser.type`
/// - the web beyond pages: `http.download`, `http.request`
const RECOMMENDED_FIXED_TOOL_NAMES: &[&str] = &[
    // Opening a page in Aura's browser, reading
    // it, and the input a smoke test of a page or a game needs.
    "browser.click",
    "browser.open",
    "browser.press",
    "browser.screenshot",
    "browser.snapshot",
    "browser.type",
    // Deleting into the
    // Recycle Bin is at hand, so the model does not reach for a Shell command.
    "fs.delete",
    "fs.find",
    "fs.list",
    "fs.patch",
    "fs.read",
    "fs.search",
    "fs.write",
    // Reading the repository's
    // state is part of most coding tasks; a tool loaded mid-conversation also changes the request's
    // tool list, which the provider's prompt cache cannot reuse.
    "git.diff",
    "git.log",
    "git.status",
    // The conversation's history by round, which a compacted context keeps only as a summary.
    "history.list_turns",
    "history.read",
    "history.search",
    // Calling an API and saving a file from the web.
    "http.download",
    "http.request",
    "memory.create",
    "memory.read",
    "memory.search",
    "memory.update",
    "shell.powershell",
    "shell.query",
    "shell.stop",
    "todo.get",
    "todo.replace",
    "todo.update",
    // Web search and
    // fetch - DeepSeek's when a DeepSeek connection has a key - and reading what was fetched.
    "web.fetch",
    "web.read",
    "web.search",
];

/// The revision of the recommended set. A list its user
/// saved took in the revision current then; what was recommended later joins it on reading, until
/// its user saves it again.
pub(crate) const RECOMMENDATION_REVISION: u32 = 5;

/// The revision a recommended tool joined in: the browser's tools 2, the HTTP tools 3,
/// `fs.delete` 4, the history tools 5; every other one was recommended before any list was
/// saved (1).
fn recommended_since(name: &str) -> u32 {
    if name.starts_with("history.") {
        5
    } else if name == "fs.delete" {
        4
    } else if name.starts_with("http.") {
        3
    } else if name.starts_with("browser.") {
        2
    } else {
        1
    }
}

/// Tools recommended until revision [`RETIRED_IN`], when the history tools took their place.
const RETIRED: &[&str] =
    &["context.read_compaction", "context.read_events", "context.read_tool_result"];
const RETIRED_IN: u32 = 5;

impl AgentApplication {
    /// A saved list with what was recommended after the revision it took in, and without what
    /// stopped being recommended since, sorted and unique.
    pub(super) fn with_later_recommendations(
        mut ids: Vec<ToolId>,
        seen: u32,
    ) -> anyhow::Result<Vec<ToolId>> {
        let release = aura_tool_registry::agent_release::release()?;
        ids.extend(release.manifest().entries.iter().filter_map(|entry| {
            let name = entry.handler.name();
            (RECOMMENDED_FIXED_TOOL_NAMES.contains(&name)
                && !required(name)
                && recommended_since(name) > seen)
                .then_some(entry.definition.tool_id)
        }));
        // What the history tools replaced leaves a list saved while it was recommended.
        if seen < RETIRED_IN {
            ids.retain(|id| {
                release.manifest().entries.iter().all(|entry| {
                    entry.definition.tool_id != *id || !RETIRED.contains(&entry.handler.name())
                })
            });
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// The recommended default selection, resolved against the published release.
    ///
    /// A name that the release does not contain is skipped rather than invented,
    /// and the result is sorted because the settings contract requires a sorted,
    /// unique list.
    fn recommended_fixed_tool_ids() -> anyhow::Result<Vec<ToolId>> {
        let release = aura_tool_registry::agent_release::release()?;
        let mut ids = release
            .manifest()
            .entries
            .iter()
            .filter(|entry| {
                RECOMMENDED_FIXED_TOOL_NAMES.contains(&entry.handler.name())
                    // A mode-required tool is loaded by its own rule and cannot be
                    // part of a user-owned selection.
                    && !required(entry.handler.name())
            })
            .map(|entry| entry.definition.tool_id)
            .collect::<Vec<_>>();
        ids.sort();
        Ok(ids)
    }

    /// The committed strategy snapshot plus the fixed selection it carries.
    ///
    /// `pub(crate)` so a regression can hold the exact snapshot a reader would hold
    /// while a newer save commits, which is the interleaving the capacity watermark
    /// has to survive.
    pub fn tool_configuration(&self) -> anyhow::Result<(ToolSettingsSnapshotV1, Vec<ToolId>)> {
        let document = self.settings.snapshot(ConfigModule::Tools)?;
        let settings = aura_config::tool_settings_snapshot_from_document(&document.document)?;
        if let Some(ids) = aura_config::fixed_tool_ids_from_document(&document.document)? {
            let seen = aura_config::fixed_tool_recommendations_from_document(&document.document)?;
            return Ok((settings, Self::with_later_recommendations(ids, seen)?));
        }
        // Never customized, so the recommended default applies.
        Ok((settings, Self::recommended_fixed_tool_ids()?))
    }

    pub fn tool_settings(&self) -> anyhow::Result<AgentToolSettingsView> {
        let (settings, fixed_tool_ids) = self.tool_configuration()?;
        self.apply_committed_strategy(&settings)?;
        let release = aura_tool_registry::agent_release::release()?;
        let git_available = git::runtime::available();
        let tools = release
            .manifest()
            .entries
            .iter()
            .map(|e| AgentToolCatalogEntry {
                tool_id: e.definition.tool_id,
                name: e.handler.name().into(),
                description: e.definition.model_summary.clone(),
                source: "builtin".into(),
                default_loaded: fixed_tool_ids.contains(&e.definition.tool_id)
                    || required(e.handler.name()),
                required: required(e.handler.name()),
                available: !e.handler.name().starts_with("git.") || git_available,
            })
            .collect();
        let snapshot = self.settings.snapshot(ConfigModule::Tools)?;
        let (network_proxy, network_proxy_diagnostic, network_proxy_draft) =
            proxy_view(&snapshot.document);
        // The web service choice, and the DeepSeek connections with a key it can use.
        let web_service = aura_config::web_service_from_document(&snapshot.document)?;
        let web_service_connections = self
            .web_service()
            .connections()
            .unwrap_or_default()
            .into_iter()
            .map(|(display_name, service)| WebServiceConnectionV1 {
                endpoint_id: service.endpoint_id,
                display_name,
            })
            .collect();
        Ok(AgentToolSettingsView {
            revision: settings.revision,
            max_dynamic_tool_schemas: settings.max_dynamic_tool_schemas,
            max_concurrent_tool_calls: settings.max_concurrent_tool_calls,
            default_timeout_ms: settings.default_timeout_ms,
            max_retries: settings.max_retries,
            fixed_tool_ids,
            tools,
            network_proxy,
            network_proxy_diagnostic,
            network_proxy_draft,
            network_proxy_recovery: proxy_recovery(&self.settings)?,
            web_service,
            web_service_connections,
        })
    }

    fn web_service(&self) -> controlled_web::web_service::ConfiguredWebService {
        controlled_web::web_service::ConfiguredWebService::new(
            Arc::clone(&self.settings),
            self.p05.clone(),
        )
    }

    pub async fn update_tool_settings(
        &self,
        command: CommandInvocationId,
        mut input: AgentToolSettingsUpdate,
    ) -> anyhow::Result<AgentToolSettingsView> {
        use aura_persistence_v2::tool::configuration::{self, ConfigurationCommand};
        let _guard = self.tool_configuration_lock.lock().await;
        // Restoring the recommended list is an explicit user action. It replaces
        // the submitted ids with the published recommendation, so the stored list
        // is always the one this build actually recommends rather than whatever a
        // client happened to send.
        if input.use_recommended_fixed_tool_ids {
            input.fixed_tool_ids = Self::recommended_fixed_tool_ids()?;
        }
        // One Rust-owned range check for all four strategy fields. A rejected save
        // names the offending field instead of failing as one opaque error.
        input.validate()?;
        if let Some(Some(proxy)) = &input.network_proxy {
            let endpoints = aura_network_control::ordinary_tools::registered_control_endpoints()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            proxy
                .validate_control_endpoints(&endpoints)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        if let Some(WebServiceSettingV1::DeepSeek { endpoint_id }) = &input.web_service {
            anyhow::ensure!(
                self.web_service()
                    .connections()?
                    .iter()
                    .any(|(_, service)| service.endpoint_id == *endpoint_id),
                "tools.web_service_connection_unusable: 这个连接不能用于网页搜索与抓取（需要带密钥的 DeepSeek 官方 API 连接），请重新选择。"
            );
        }
        let release = aura_tool_registry::agent_release::release()?;
        anyhow::ensure!(
            input.fixed_tool_ids.iter().all(|id| release.manifest().entries.iter().any(|e| e
                .definition
                .tool_id
                == *id
                && !required(e.handler.name()))),
            "tools.fixed_selection_invalid"
        );
        let current = self.tool_settings()?;
        let stored = input.clone();
        let old = self
            .store
            .read(agent_store::StoreStep::operation("agent.tools.configuration.load"), move |db| {
                configuration::load(db, command)
            })
            .await?;
        if let Some(previous) = &old {
            anyhow::ensure!(previous.input == input, "tools.command_identity_conflict");
            if let Some(result) = &previous.result {
                return Ok(result.clone());
            }
        } else {
            anyhow::ensure!(
                current.revision == input.expected_revision,
                "tools.configuration_revision_conflict"
            );
            let recorded = stored.clone();
            self.store
                .write(
                    agent_store::StoreStep::operation("agent.tools.configuration.claim"),
                    move |journal| {
                        journal.database.unit_of_work(|tx| {
                            configuration::save(
                                tx,
                                command,
                                &ConfigurationCommand { input: recorded, result: None },
                                UtcTimestamp::now(),
                            )
                        })
                    },
                )
                .await?;
        }
        let desired = current.fixed_tool_ids == input.fixed_tool_ids
            && current.max_dynamic_tool_schemas == input.max_dynamic_tool_schemas
            && current.max_concurrent_tool_calls == input.max_concurrent_tool_calls
            && current.default_timeout_ms == input.default_timeout_ms
            && current.max_retries == input.max_retries
            && proxy_unchanged(&self.settings, &input.network_proxy)?
            && input.web_service.as_ref().is_none_or(|web| *web == current.web_service);
        let result = if desired
            && (current.revision == input.expected_revision
                || (old.is_some()
                    && Some(current.revision.value())
                        == input.expected_revision.value().checked_add(1)))
        {
            current
        } else {
            anyhow::ensure!(
                current.revision == input.expected_revision,
                "tools.configuration_revision_conflict"
            );
            self.settings.update(
                ConfigModule::Tools,
                input.expected_revision.value(),
                |values| {
                    values.insert(
                        FIXED_TOOL_IDS_FIELD.into(),
                        serde_json::json!(input.fixed_tool_ids),
                    );
                    // The list as saved took in every tool recommended so far.
                    values.insert(
                        aura_config::FIXED_TOOL_RECOMMENDATIONS_FIELD.into(),
                        serde_json::json!(RECOMMENDATION_REVISION),
                    );
                    values.insert(
                        MAX_DYNAMIC_TOOL_SCHEMAS_FIELD.into(),
                        serde_json::json!(input.max_dynamic_tool_schemas),
                    );
                    values.insert(
                        aura_config::MAX_CONCURRENT_TOOL_CALLS_FIELD.into(),
                        serde_json::json!(input.max_concurrent_tool_calls),
                    );
                    values.insert(
                        aura_config::TOOL_DEFAULT_TIMEOUT_MS_FIELD.into(),
                        serde_json::json!(input.default_timeout_ms),
                    );
                    values.insert(
                        aura_config::MAX_RETRIES_FIELD.into(),
                        serde_json::json!(input.max_retries),
                    );
                    match &input.web_service {
                        None => {}
                        Some(WebServiceSettingV1::Automatic) => {
                            values.remove(aura_config::WEB_SERVICE_FIELD);
                        }
                        Some(chosen) => {
                            values.insert(
                                aura_config::WEB_SERVICE_FIELD.into(),
                                serde_json::to_value(chosen).expect("web service json"),
                            );
                        }
                    }
                    match &input.network_proxy {
                        None => {}
                        Some(None) => {
                            values.remove(aura_config::NETWORK_PROXY_FIELD);
                        }
                        Some(Some(proxy)) => {
                            values.insert(
                                aura_config::NETWORK_PROXY_FIELD.into(),
                                serde_json::to_value(proxy).expect("proxy json"),
                            );
                        }
                    }
                },
            )?;
            // `tool_settings` reconciled the limiter with the committed value.
            // Existing members keep their slot; only new admissions see the
            // new capacity.
            self.tool_settings()?
        };
        let recorded = input.clone();
        let recorded_result = result.clone();
        self.store
            .write(
                agent_store::StoreStep::operation("agent.tools.configuration.save"),
                move |journal| {
                    journal.database.unit_of_work(|tx| {
                        configuration::save(
                            tx,
                            command,
                            &ConfigurationCommand {
                                input: recorded,
                                result: Some(recorded_result),
                            },
                            UtcTimestamp::now(),
                        )
                    })
                },
            )
            .await?;
        Ok(result)
    }

    /// Builds the registry snapshot for one turn from facts already read on the caller's bounded
    /// section.
    ///
    /// It takes the connection rather than the database handle so its only caller can run
    /// it inside one admitted read (`AgentStore::read` hands out a `&Connection`), instead of the
    /// caller taking `journal.lock()` itself on an async task.
    pub fn prepare_registry(
        &self,
        db: &aura_persistence_core::Connection,
        turn: &TurnTask,
        permission: &aura_security_contract::PermissionSnapshot,
        at: UtcTimestamp,
    ) -> anyhow::Result<(
        ToolRegistrySnapshotV1,
        Vec<ToolEphemeralLoadRecordV1>,
        ToolSettingsSnapshotV1,
        package_tools::SnapshotPackageTools,
    )> {
        let scope = ToolRegistryScopeV1::Session {
            workspace_id: turn.workspace_id,
            session_id: turn.session_id,
            turn_task_id: turn.turn_task_id,
            agent_run_id: turn.agent_run_id,
        };
        let previous = exposure::load(db, &scope)?;
        let purpose = aura_persistence_v2::agent::application::purpose(db, turn.agent_run_id)?;
        let dynamic = session_tools::load(db, turn.session_id)?;
        let (settings, mut fixed) = self.tool_configuration()?;
        let release = aura_tool_registry::agent_release::release()?;
        // The user's Chrome is offered while its extension is connected, and loaded for a
        // conversation the user shared a tab with.
        let chrome = self.execution.chrome.get();
        let chrome_connected = chrome.is_some_and(|chrome| chrome.connected());
        if chrome_connected && chrome.is_some_and(|chrome| chrome.has_shares(turn.session_id)) {
            fixed.extend(
                release
                    .manifest()
                    .entries
                    .iter()
                    .filter(|entry| entry.handler.name().starts_with("chrome."))
                    .map(|entry| entry.definition.tool_id),
            );
            fixed.sort();
            fixed.dedup();
        }
        let mode_hash = agent_execution::registry_mode_hash(permission)?;
        let authority = self.execution.persistence.tool_authority_snapshot(
            Some(turn.workspace_id),
            Some(turn.session_id),
            Some(turn.turn_task_id),
        )?;
        let mut candidates = release.candidates();
        for candidate in &mut candidates {
            if let Some(instance) = authority.instances.iter().find(|i| {
                i.tool_instance_id == candidate.instance.tool_instance_id
                    && i.tool_version == candidate.instance.tool_version
                    && i.source_contract_fingerprint
                        == candidate.instance.source_contract_fingerprint
            }) {
                candidate.instance = instance.clone();
            }
        }
        let git_available = git::runtime::available();
        git::runtime::constrain_candidates(&mut candidates, git_available);
        if !chrome_connected {
            for candidate in &mut candidates {
                if candidate.definition.canonical_name.as_str().starts_with("chrome.") {
                    candidate.instance.runtime_health =
                        aura_tool_domain::ToolRuntimeHealthV1::Unavailable;
                }
            }
        }
        // Installed, trusted, enabled package tools. Like every non-built-in tool
        // they are discoverable and loaded per session; a tool whose declared credential
        // is not bound stays a visible exclusion.
        let installed = package_tools::installed_tools(db)?;
        candidates.extend(installed.iter().map(package_tools::InstalledPackageTool::candidate));
        // The tools of usable MCP servers, discoverable and loaded per session like
        // package tools; they are never core tools.
        let mcp_tools = self
            .execution
            .mcp
            .get()
            .map_or_else(Vec::new, |runtime| mcp_tools::usable_tools(runtime, db));
        candidates.extend(mcp_tools.iter().map(mcp_tools::McpTool::candidate));
        let mcp_state = mcp_tools
            .iter()
            .map(|tool| tool.definition.definition_fingerprint.clone())
            .collect::<Vec<_>>();
        let package_state = installed
            .iter()
            .map(|tool| {
                (
                    tool.definition.definition_fingerprint.clone(),
                    tool.trust_revision,
                    tool.credential.as_ref().map(|binding| binding.revision),
                )
            })
            .collect::<Vec<_>>();
        // A turn keeps the dynamic tools its first step was given, so the request's tool
        // list stays the same until the turn ends; a load or unload during it applies from the next.
        let frozen = previous.as_ref().map(|exposure| exposure.loads.clone());
        let loads = frozen.unwrap_or_else(|| {
            dynamic.as_ref().map_or_else(Vec::new, |d| {
                d.tool_ids
                    .iter()
                    .filter_map(|id| {
                        release
                            .manifest()
                            .entries
                            .iter()
                            .map(|e| &e.definition)
                            .chain(installed.iter().map(|tool| &tool.definition))
                            .chain(mcp_tools.iter().map(|tool| &tool.definition))
                            .find(|definition| definition.tool_id == *id)
                    })
                    .map(|definition| ToolEphemeralLoadRecordV1 {
                        session_id: turn.session_id,
                        turn_task_id: turn.turn_task_id,
                        tool_id: definition.tool_id,
                        tool_version: definition.tool_version.clone(),
                        entity_revision: d.revision,
                        loaded_at: turn.created_at,
                    })
                    .collect::<Vec<_>>()
            })
        });
        let key = jcs::canonical_sha256(&(
            release.fingerprint(),
            &settings,
            &fixed,
            &loads,
            &mode_hash,
            turn.mode,
            purpose,
            authority.registry_revision,
            git_available,
            &package_state,
            &mcp_state,
        ))?;
        let mut cache =
            self.registry_cache.lock().map_err(|_| anyhow::anyhow!("tools.cache_unavailable"))?;
        if let Some(cached) = cache.get(&turn.agent_run_id).filter(|c| c.key == key) {
            let mut named = package_tools::named_by(&installed, &cached.registry);
            named.extend(mcp_tools::named_by(&mcp_tools, &cached.registry));
            return Ok((cached.registry.clone(), loads, settings, named));
        }
        let mut registry = build_retained_parent_registry_snapshot(ParentRegistryBuildInputV1 {
            registry_revision: Revision::new(
                previous.as_ref().map_or(1, |v| v.head.registry_revision.value() + 1),
            ),
            scope,
            settings: settings.clone(),
            mode_permission_context_hash: mode_hash,
            core_exposed_tool_ids: release
                .manifest()
                .entries
                .iter()
                .filter(|e| {
                    super::context::core_tool(e.handler.name(), turn.mode, purpose)
                        || (!required(e.handler.name()) && fixed.contains(&e.definition.tool_id))
                })
                .map(|e| e.definition.tool_id)
                .collect::<BTreeSet<_>>(),
            candidates,
            workspace_pins: vec![],
            session_pins: vec![],
            ephemeral_loads: loads.clone(),
            created_at: at,
        })?
        .snapshot;
        if let Some(old) = previous
            .as_ref()
            .map(|v| tool::registry(db, v.head.tool_registry_snapshot_id))
            .transpose()?
            .flatten()
            && old.eligibility_context_hash == registry.eligibility_context_hash
            && old.catalog_entries == registry.catalog_entries
            && old.effective_bindings == registry.effective_bindings
            && old.entry_bindings == registry.entry_bindings
        {
            registry = (*old).clone();
        }
        if cache.len() >= 64 {
            cache.clear();
        }
        cache.insert(turn.agent_run_id, RegistryCache { key, registry: registry.clone() });
        let mut named = package_tools::named_by(&installed, &registry);
        named.extend(mcp_tools::named_by(&mcp_tools, &registry));
        Ok((registry, loads, settings, named))
    }

    /// Connects to the draft proxy and finishes its local handshake. No tunnel and no target.
    pub async fn test_network_proxy(
        &self,
        input: AgentNetworkProxyTestRequest,
    ) -> anyhow::Result<AgentNetworkProxyTestResult> {
        let proxy = match ToolNetworkProxyV1::new(true, input.protocol, input.host, input.port) {
            Ok(proxy) => proxy,
            Err(error) => {
                return Ok(AgentNetworkProxyTestResult { ok: false, message: error.to_string() });
            }
        };
        let message = match aura_network_control::probe_user_proxy(&proxy).await {
            Ok(()) => {
                return Ok(AgentNetworkProxyTestResult {
                    ok: true, message: "连接成功".into()
                });
            }
            Err(aura_network_control::NetworkControlError::UserProxyFailure(failure)) => {
                failure.reason
            }
            Err(aura_network_control::NetworkControlError::ControlEndpointBlocked) => {
                "代理地址是 Aura 已登记的控制端点，不能把工具代理指到它。".into()
            }
            Err(aura_network_control::NetworkControlError::DestinationBlocked) => {
                "只允许本机回环地址（127.0.0.0/8、::1 或 localhost）".into()
            }
            Err(_) => "无法连接代理端口（拒绝连接或连接失败）".into(),
        };
        Ok(AgentNetworkProxyTestResult { ok: false, message })
    }
}

#[cfg(test)]
mod tests {
    use super::{RECOMMENDED_FIXED_TOOL_NAMES, required};

    /// The recommended default list must be real, selectable, and stable.
    ///
    /// Every name has to exist in the published release, because a name the
    /// release does not contain silently contributes nothing and the default would
    /// quietly shrink as the release changes. None of them may be mode-required,
    /// because a required tool is loaded by its own rule and cannot appear in a
    /// user-owned selection — listing one would make the whole default set fail
    /// validation.
    #[test]
    fn the_recommended_default_list_is_valid_against_the_published_release() {
        let release = aura_tool_registry::agent_release::release().expect("release");
        let available =
            release.manifest().entries.iter().map(|entry| entry.handler.name()).collect::<Vec<_>>();

        assert!(!RECOMMENDED_FIXED_TOOL_NAMES.is_empty());
        for name in RECOMMENDED_FIXED_TOOL_NAMES {
            assert!(
                available.contains(name),
                "{name} is recommended but the release does not publish it"
            );
            assert!(
                !required(name),
                "{name} is mode-required and must not be part of a user selection"
            );
        }
        // The list itself must not repeat a name, and it must be sorted so the
        // resolved ids satisfy the settings contract without a second pass.
        let mut sorted = RECOMMENDED_FIXED_TOOL_NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted, RECOMMENDED_FIXED_TOOL_NAMES,
            "the recommended names must be sorted and unique"
        );

        // Resolving against the release yields exactly one id per name.
        let ids = super::AgentApplication::recommended_fixed_tool_ids().expect("recommended ids");
        assert_eq!(ids.len(), RECOMMENDED_FIXED_TOOL_NAMES.len());
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "ids must be sorted and unique");
    }

    /// What was
    /// recommended after a list was saved joins it, and nothing its user left out comes back.
    #[test]
    fn a_customized_list_takes_in_what_was_recommended_after_it_was_saved() {
        let release = aura_tool_registry::agent_release::release().expect("release");
        let id = |name: &str| release.entry(name).expect("published").definition.tool_id;
        // Saved at revision 1: no browser tool, and web.search left out by its user.
        let mut saved = vec![id("fs.read"), id("git.status"), id("todo.get")];
        saved.sort();
        let merged =
            super::AgentApplication::with_later_recommendations(saved.clone(), 1).expect("merged");
        for name in ["browser.click", "browser.open", "browser.screenshot", "browser.snapshot"] {
            assert!(merged.contains(&id(name)), "{name} joins the saved list");
        }
        assert!(!merged.contains(&id("web.search")), "what its user left out stays out");
        assert!(merged.windows(2).all(|pair| pair[0] < pair[1]), "sorted and unique");
        // The HTTP tools join a list saved before them, a browser
        // tool does not join one saved at revision 2.
        for name in ["http.request", "http.download"] {
            assert!(merged.contains(&id(name)), "{name} joins the saved list");
        }
        let after_l13 =
            super::AgentApplication::with_later_recommendations(saved.clone(), 2).expect("merged");
        assert!(after_l13.contains(&id("http.download")));
        assert!(!after_l13.contains(&id("browser.click")), "its user saved after L13 without it");
        // Deleting into the Recycle Bin joins a list saved before it; the HTTP
        // tools do not join one saved at revision 3 without them.
        let after_l13_2 =
            super::AgentApplication::with_later_recommendations(saved.clone(), 3).expect("merged");
        assert!(after_l13_2.contains(&id("fs.delete")));
        assert!(
            !after_l13_2.contains(&id("http.request")),
            "its user saved after L13.2 without it"
        );
        // The history tools take the place of the context readers in a list saved before
        // them; a list saved since keeps whatever its user chose.
        let mut with_context = saved.clone();
        with_context.push(id("context.read_events"));
        with_context.sort();
        let after_l13_3 =
            super::AgentApplication::with_later_recommendations(with_context.clone(), 4)
                .expect("merged");
        for name in ["history.list_turns", "history.search", "history.read"] {
            assert!(after_l13_3.contains(&id(name)), "{name} joins the saved list");
        }
        assert!(!after_l13_3.contains(&id("context.read_events")));
        assert!(
            super::AgentApplication::with_later_recommendations(with_context, 5)
                .expect("merged")
                .contains(&id("context.read_events")),
            "chosen after the history tools came"
        );
        // Saved at the current revision: the list is exactly what its user chose.
        let current = super::RECOMMENDATION_REVISION;
        assert_eq!(
            super::AgentApplication::with_later_recommendations(saved.clone(), current)
                .expect("unchanged"),
            saved
        );
    }
}
