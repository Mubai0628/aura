//! The servers of this process: discovery runs, one connection per session and server, and
//! what the settings page shows about each. A server that fails affects only its own tools.
use super::connection::{Connection, StartFailure};
use super::credentials::{McpSecrets, SecretStore};
use super::http::HttpPlan;
use super::launch::{self, HashCache};
use super::oauth::McpOAuth;
use aura_config::SettingsFacade;
use aura_domain_common::{SessionId, UtcTimestamp};
use aura_mcp::{McpServerConfig, McpTransport, normalize_tools};
use aura_persistence_v2::mcp::{McpServerFacts, StoredCatalog};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Restarts allowed per session and server within the window; then the server stays down
/// until the user retries in Settings.
const RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(300);
const IDLE_STOP: Duration = Duration::from_secs(600);

/// Held while one session's instance of one server starts.
type StartGate = Arc<Mutex<()>>;

/// Work a server began before it was stopped ends with this, and nothing of it is kept.
const STOPPED_WHILE_STARTING: &str =
    "服务器在启动时被停用、撤销信任、修改或移除，这次没有使用它；如仍需要，请重试";

mod plugins;
mod signing;
mod view;

/// Core's network gateway and the runtime it runs on: every request to a Streamable HTTP
/// server goes through them.
#[derive(Clone)]
pub(crate) struct McpNetwork {
    pub gateway: Arc<aura_network_control::CoreHttpClientGateway>,
    pub runtime: tokio::runtime::Handle,
}

#[derive(Default)]
struct Discovery {
    running: bool,
    last_error: Option<String>,
    stderr: Vec<String>,
}

pub(crate) struct McpRuntime {
    settings: Arc<SettingsFacade>,
    journal: Arc<crate::broker::BrokerJournal>,
    network: Option<McpNetwork>,
    secrets: Arc<McpSecrets>,
    oauth: Arc<McpOAuth>,
    data_root: PathBuf,
    client_version: String,
    pub(super) hashes: HashCache,
    connections: Mutex<HashMap<(SessionId, String), Arc<Connection>>>,
    /// One start at a time for each session and server: calls that arrive together share the
    /// instance the first one starts, and a running instance is replaced only once it ended.
    starting: Mutex<HashMap<(SessionId, String), StartGate>>,
    restarts: Mutex<HashMap<(SessionId, String), Vec<Instant>>>,
    /// How many times each server's instances were stopped (switched off, untrusted, changed or
    /// removed). Work that began before a stop finds the count moved on and is dropped: a
    /// listing is not stored and a starting instance is not kept.
    stops: Mutex<HashMap<String, u64>>,
    discoveries: Mutex<HashMap<String, Discovery>>,
    revising: Mutex<std::collections::HashSet<String>>,
    /// The plugin runtime, whose ready plugins bring servers too.
    plugins: std::sync::OnceLock<std::sync::Weak<crate::plugins::PluginRuntime>>,
}

/// Each tool keeps its revision while everything its definition is derived from stays the
/// same; anything else takes the next one.
fn assign_revisions(
    tx: &aura_persistence_core::Transaction<'_>,
    server: &McpServerConfig,
    stored: &StoredCatalog,
) -> aura_persistence_core::StoreResult<()> {
    for tool in &stored.catalog.tools {
        let id = super::identity::tool_id(&server.id, &tool.wire_name);
        let key = super::identity::definition_key(server, &stored.config_fingerprint, tool)
            .map_err(|_| aura_persistence_core::PersistenceCoreError::Invalid("MCP tool key"))?;
        aura_persistence_v2::mcp::tool_revision(tx, &id.to_string(), &key, UtcTimestamp::now())?;
    }
    Ok(())
}

impl McpRuntime {
    pub(crate) fn new(
        settings: Arc<SettingsFacade>,
        journal: Arc<crate::broker::BrokerJournal>,
        network: Option<McpNetwork>,
        secret_store: Arc<dyn SecretStore>,
    ) -> Arc<Self> {
        let data_root = journal
            .database_path()
            .ok()
            .and_then(|path| {
                path.parent().and_then(std::path::Path::parent).map(|p| p.join("v2-mcp"))
            })
            .unwrap_or_else(|| std::env::temp_dir().join("aura-v2-mcp"));
        let secrets = Arc::new(McpSecrets::new(secret_store, Arc::clone(&journal)));
        let oauth = McpOAuth::new(Arc::clone(&journal), network.clone(), Arc::clone(&settings));
        let runtime = Arc::new(Self {
            settings,
            journal,
            network,
            secrets,
            oauth,
            data_root,
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
            hashes: HashCache::default(),
            connections: Mutex::new(HashMap::new()),
            starting: Mutex::new(HashMap::new()),
            restarts: Mutex::new(HashMap::new()),
            stops: Mutex::new(HashMap::new()),
            discoveries: Mutex::new(HashMap::new()),
            revising: Mutex::new(std::collections::HashSet::new()),
            plugins: std::sync::OnceLock::new(),
        });
        runtime.sweep_credentials();
        // Idle session instances are stopped; the reaper ends with the runtime.
        let weak = Arc::downgrade(&runtime);
        let _ = std::thread::Builder::new().name("aura-mcp-idle".into()).spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(30));
                let Some(runtime) = weak.upgrade() else { return };
                runtime.stop_where(|_, connection| {
                    !connection.alive() || connection.idle_for() >= IDLE_STOP
                });
            }
        });
        runtime
    }

    pub(crate) fn settings(&self) -> &SettingsFacade {
        &self.settings
    }

    fn scratch(&self, server: &str) -> PathBuf {
        self.data_root.join(server)
    }

    /// The servers in force with their configuration checked now.
    /// Every server: the user's from `mcp.json`, then those ready plugins bring. Changes
    /// are written to `mcp.json` only for the user's own.
    pub(crate) fn configured(&self) -> aura_config::McpRegistry {
        let mut registry = self.settings.mcp_registry();
        let brought = self.plugin_servers(&registry.servers);
        registry.servers.extend(brought.into_iter().map(|server| server.config));
        registry
    }

    /// Whether this exact configuration is trusted, on, unchanged since the trust, and has the
    /// credentials it names. Reads the store; a caller inside its own store access uses
    /// [`McpRuntime::usable_in`].
    pub(crate) fn usable(&self, server: &McpServerConfig) -> Result<launch::Checked, String> {
        let checked = launch::check(server, &self.hashes);
        let gap = match self.journal.lock() {
            Ok(journal) => self.credential_gap(
                server,
                checked.fingerprint.as_deref(),
                journal.database.connection(),
            ),
            Err(error) => Some(error.to_string()),
        };
        Self::decide(server, checked, gap)
    }

    /// [`McpRuntime::usable`] inside the caller's own store access (`db`): the agent lists and
    /// checks tools while it holds the store, whose lock is not reentrant.
    pub(crate) fn usable_in(
        &self,
        server: &McpServerConfig,
        db: &aura_persistence_core::Connection,
    ) -> Result<launch::Checked, String> {
        let checked = launch::check(server, &self.hashes);
        let gap = self.credential_gap(server, checked.fingerprint.as_deref(), db);
        Self::decide(server, checked, gap)
    }

    fn decide(
        server: &McpServerConfig,
        checked: launch::Checked,
        credential_gap: Option<String>,
    ) -> Result<launch::Checked, String> {
        if !checked.problems.is_empty() {
            return Err(checked.problems.join("；"));
        }
        if let Some(gap) = credential_gap {
            return Err(gap);
        }
        match (&server.trusted_fingerprint, &checked.fingerprint) {
            (None, _) => Err(format!("服务器 {} 尚未信任", server.id)),
            (Some(trusted), Some(current)) if trusted == current => {
                if server.enabled {
                    Ok(checked)
                } else {
                    Err(format!("服务器 {} 已停用", server.id))
                }
            }
            _ => Err(format!("服务器 {} 的配置在信任之后变了，需要在设置里重新信任", server.id)),
        }
    }

    fn connect(
        &self,
        server: &McpServerConfig,
        checked: &launch::Checked,
    ) -> Result<Connection, StartFailure> {
        let refused = |reason: String| StartFailure { reason, stderr: Vec::new() };
        let timeout = Duration::from_millis(server.startup_timeout_ms);
        if let McpTransport::Http { url, headers, .. } = &server.transport {
            let network = self
                .network
                .as_ref()
                .ok_or_else(|| refused("Aura 的网络网关不可用，HTTP 服务器无法连接".into()))?;
            let plan = HttpPlan {
                endpoint: url.clone(),
                headers: headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect(),
                network_proxy: self
                    .settings
                    .tool_settings_snapshot()
                    .ok()
                    .and_then(|settings| settings.network_proxy),
                credential: self.http_credential(server).map_err(refused)?,
            };
            let gateway = Arc::clone(&network.gateway);
            return Connection::start_http(
                plan,
                gateway,
                network.runtime.clone(),
                timeout,
                &self.client_version,
            );
        }
        let resolved =
            checked.resolved.as_ref().ok_or_else(|| refused("服务器程序没有解析到".into()))?;
        let secrets = self.secret_environment(server).map_err(refused)?;
        let limits = self.plugin_limits(&server.id);
        let plan =
            launch::stdio_plan(server, resolved, &self.scratch(&server.id), &secrets, limits)
                .map_err(|error| refused(error.to_string()))?;
        drop(secrets);
        Connection::start(plan, timeout, &self.client_version)
    }

    fn store_catalog(
        &self,
        server: &McpServerConfig,
        checked: &launch::Checked,
        initialized: &aura_mcp::InitializeResult,
        tools: Vec<aura_mcp::McpTool>,
        since: u64,
    ) -> Result<StoredCatalog, String> {
        let stored = StoredCatalog {
            server_id: server.id.clone(),
            config_fingerprint: checked.fingerprint.clone().unwrap_or_default(),
            protocol_version: initialized.protocol_version.clone(),
            server: McpServerFacts {
                name: initialized.server_info.name.clone(),
                version: initialized.server_info.version.clone(),
                instructions: initialized.instructions.clone(),
            },
            catalog: normalize_tools(&server.id, tools),
            discovered_at: UtcTimestamp::now(),
        };
        let mut journal = self.journal.lock().map_err(|error| error.to_string())?;
        // Checked under the journal lock that `forget` deletes under, after its stop.
        if self.stop_count(&server.id) != since {
            return Err(STOPPED_WHILE_STARTING.to_owned());
        }
        journal
            .database
            .unit_of_work(|tx| {
                assign_revisions(tx, server, &stored)?;
                aura_persistence_v2::mcp::save(tx, &stored)
            })
            .map_err(|error| error.to_string())?;
        Ok(stored)
    }

    fn stop_count(&self, id: &str) -> u64 {
        self.stops
            .lock()
            .map(|stops| stops.get(id).copied().unwrap_or_default())
            .unwrap_or(u64::MAX)
    }

    /// Assigns revisions for the stored listing under the server's current settings (after
    /// its display name or call timeout changed). Nothing happens for a server that is not
    /// usable or whose listing was made for another configuration.
    pub(crate) fn revise(&self, server: &McpServerConfig) {
        let Some(fingerprint) = self.usable(server).ok().and_then(|checked| checked.fingerprint)
        else {
            return;
        };
        let Some(stored) =
            self.catalog(&server.id).filter(|stored| stored.config_fingerprint == fingerprint)
        else {
            return;
        };
        if let Ok(mut journal) = self.journal.lock() {
            let _ = journal.database.unit_of_work(|tx| assign_revisions(tx, server, &stored));
        }
    }

    /// [`McpRuntime::revise`] on its own thread, for a reader that found a tool without a
    /// revision for the current settings (for example after `mcp.json` was edited by hand).
    pub(crate) fn revise_later(self: &Arc<Self>, server: McpServerConfig) {
        if let Ok(mut revising) = self.revising.lock()
            && !revising.insert(server.id.clone())
        {
            return;
        }
        let runtime = Arc::clone(self);
        let _ = std::thread::Builder::new().name("aura-mcp-revise".into()).spawn(move || {
            runtime.revise(&server);
            if let Ok(mut revising) = runtime.revising.lock() {
                revising.remove(&server.id);
            }
        });
    }

    /// Starts the server, lists its tools and stores them for the registry. Runs on the
    /// caller's thread for up to the startup timeout per step.
    pub(crate) fn discover(&self, server: &McpServerConfig) -> Result<StoredCatalog, String> {
        let since = self.stop_count(&server.id);
        let checked = match self.usable(server) {
            Ok(checked) => checked,
            Err(reason) => {
                // A discovery started in the background ends here too, never left "starting".
                if let Ok(mut discoveries) = self.discoveries.lock() {
                    let entry = discoveries.entry(server.id.clone()).or_default();
                    entry.running = false;
                    entry.last_error = Some(reason.clone());
                }
                return Err(reason);
            }
        };
        if let Ok(mut discoveries) = self.discoveries.lock() {
            discoveries.entry(server.id.clone()).or_default().running = true;
        }
        let (stored, stderr) = match self.connect(server, &checked) {
            Err(failure) => (Err(failure.reason), failure.stderr),
            Ok(connection) => {
                let tools = if connection.initialized.tools_supported {
                    connection.list_tools(Duration::from_millis(server.startup_timeout_ms))
                } else {
                    Ok(Vec::new())
                };
                let stderr = connection.close_and_drain();
                let stored = tools.and_then(|tools| {
                    self.store_catalog(server, &checked, &connection.initialized, tools, since)
                });
                (stored, stderr)
            }
        };
        // A removed server's entry is gone and stays gone; a listing dropped because the server
        // was stopped meanwhile is not its error.
        if let Ok(mut discoveries) = self.discoveries.lock()
            && let Some(entry) = discoveries.get_mut(&server.id)
        {
            entry.running = false;
            entry.stderr = stderr;
            entry.last_error =
                stored.as_ref().err().filter(|error| *error != STOPPED_WHILE_STARTING).cloned();
        }
        stored
    }

    /// Starts discovery on its own thread (after trust, enabling or a configuration change).
    pub(crate) fn discover_later(self: &Arc<Self>, server: McpServerConfig) {
        if let Ok(mut discoveries) = self.discoveries.lock() {
            let entry = discoveries.entry(server.id.clone()).or_default();
            if entry.running {
                return;
            }
            entry.running = true;
        }
        let runtime = Arc::clone(self);
        let _ = std::thread::Builder::new().name("aura-mcp-discovery".into()).spawn(move || {
            let _ = runtime.discover(&server);
        });
    }

    /// The session's running instance of the server, started on first use. A crashed one is
    /// restarted a bounded number of times.
    pub(crate) fn connection(
        self: &Arc<Self>,
        session: SessionId,
        server: &McpServerConfig,
    ) -> Result<Arc<Connection>, String> {
        let key = (session, server.id.clone());
        if let Some(connection) = self.alive(&key)? {
            self.relist_if_changed(&connection, server);
            return Ok(connection);
        }
        let gate = Arc::clone(
            self.starting.lock().map_err(|e| e.to_string())?.entry(key.clone()).or_default(),
        );
        let _starting = gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // A call that waited here uses what the first one started.
        if let Some(connection) = self.alive(&key)? {
            self.relist_if_changed(&connection, server);
            return Ok(connection);
        }
        let since = self.stop_count(&server.id);
        let checked = self.usable(server)?;
        {
            let mut restarts = self.restarts.lock().map_err(|e| e.to_string())?;
            let recent = restarts.entry(key.clone()).or_default();
            recent.retain(|at| at.elapsed() < RESTART_WINDOW);
            if recent.len() >= RESTARTS {
                return Err(format!(
                    "服务器 {} 在 5 分钟内已启动 {RESTARTS} 次仍然退出，已暂停；请在设置里查看错误并重试",
                    server.id
                ));
            }
            recent.push(Instant::now());
        }
        let connection =
            Arc::new(self.connect(server, &checked).map_err(|failure| failure.reason)?);
        let previous = {
            let mut connections = self.connections.lock().map_err(|e| e.to_string())?;
            // `stop_server` counts the stop before it clears the instances under this lock.
            if self.stop_count(&server.id) != since {
                drop(connections);
                connection.close();
                return Err(STOPPED_WHILE_STARTING.to_owned());
            }
            connections.insert(key, Arc::clone(&connection))
        };
        if let Some(previous) = previous {
            previous.close();
        }
        Ok(connection)
    }

    fn alive(&self, key: &(SessionId, String)) -> Result<Option<Arc<Connection>>, String> {
        let connections = self.connections.lock().map_err(|e| e.to_string())?;
        Ok(connections.get(key).filter(|connection| connection.alive()).cloned())
    }

    /// The server said its tools changed: list them again on its own running instance, in the
    /// background, and store them for the registry.
    pub(crate) fn relist_if_changed(
        self: &Arc<Self>,
        connection: &Arc<Connection>,
        server: &McpServerConfig,
    ) {
        if !connection.take_tools_changed() {
            return;
        }
        let (runtime, connection, server) =
            (Arc::clone(self), Arc::clone(connection), server.clone());
        let since = self.stop_count(&server.id);
        let _ = std::thread::Builder::new().name("aura-mcp-relist".into()).spawn(move || {
            let Ok(checked) = runtime.usable(&server) else { return };
            let listed = connection.list_tools(Duration::from_millis(server.startup_timeout_ms));
            let stored = listed.and_then(|tools| {
                runtime.store_catalog(&server, &checked, &connection.initialized, tools, since)
            });
            if let Ok(mut discoveries) = runtime.discoveries.lock() {
                discoveries.entry(server.id.clone()).or_default().last_error = stored.err();
            }
        });
    }

    fn stop_where(&self, matches: impl Fn(&(SessionId, String), &Connection) -> bool) {
        let stopped: Vec<Arc<Connection>> = match self.connections.lock() {
            Ok(mut connections) => {
                let keys: Vec<_> = connections
                    .iter()
                    .filter(|(k, c)| matches(k, c))
                    .map(|(k, _)| k.clone())
                    .collect();
                keys.iter().filter_map(|key| connections.remove(key)).collect()
            }
            Err(_) => Vec::new(),
        };
        for connection in stopped {
            connection.close();
        }
    }

    /// Stops every instance of one server (it was switched off, untrusted, changed or removed).
    pub(crate) fn stop_server(&self, id: &str) {
        if let Ok(mut stops) = self.stops.lock() {
            *stops.entry(id.to_owned()).or_default() += 1;
        }
        self.stop_where(|(_, server), _| server == id);
        if let Ok(mut restarts) = self.restarts.lock() {
            restarts.retain(|(_, server), _| server != id);
        }
        if let Ok(mut starting) = self.starting.lock() {
            starting.retain(|(_, server), _| server != id);
        }
    }

    pub(crate) fn shutdown(&self) {
        self.stop_where(|_, _| true);
    }

    pub(crate) fn running_instances(&self, id: &str) -> u32 {
        self.connections
            .lock()
            .map(|c| c.iter().filter(|((_, server), c)| server == id && c.alive()).count())
            .unwrap_or_default()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    pub(crate) fn stderr(&self, id: &str) -> Vec<String> {
        let running = self.connections.lock().ok().and_then(|connections| {
            connections.iter().find(|((_, server), _)| server == id).map(|(_, c)| c.stderr_tail())
        });
        running.unwrap_or_else(|| {
            self.discoveries
                .lock()
                .ok()
                .and_then(|d| d.get(id).map(|d| d.stderr.clone()))
                .unwrap_or_default()
        })
    }

    pub(crate) fn catalog(&self, id: &str) -> Option<StoredCatalog> {
        let journal = self.journal.lock().ok()?;
        aura_persistence_v2::mcp::load(journal.database.connection(), id).ok().flatten()
    }

    pub(crate) fn forget(&self, id: &str) -> Result<(), String> {
        self.stop_server(id);
        if let Ok(mut discoveries) = self.discoveries.lock() {
            discoveries.remove(id);
        }
        let mut journal = self.journal.lock().map_err(|error| error.to_string())?;
        journal
            .database
            .unit_of_work(|tx| aura_persistence_v2::mcp::delete(tx, id))
            .map_err(|error| error.to_string())
    }
}
