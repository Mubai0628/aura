//! What a plugin's background service asks of Aura, and what it shows in Aura's window. Each
//! request is checked against the plugin as it is trusted now, as its panel's calls are. What it
//! shows goes to Main in the order the service sent it; a window that cannot keep up refuses it
//! for now, so the service never waits on Main. Its calls in conversations are `sessions`'.
use super::sessions::ServiceSessions;
use crate::checks::{Standing, checked_notice, checked_status, permitted, unusable};
use aura_agent_domain::{
    PluginLogSource, PluginNotice, PluginServiceUi, PluginStatus, PluginUiTone,
};
use aura_core_base::outbound_json::send_notification;
use aura_extension_service::plugins::{PluginRuntime, ReadyPlugin};
use aura_ipc::{AsyncPriorityQueue, QueuePriority};
use aura_plugin_host::{
    CallFuture, ChannelAbilities, MenuCommand, ServiceCalls, ServiceError, shown,
};
use aura_plugins::{PluginCapability, PluginServiceStatus};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// The notification Main receives the changes on.
pub const UI_NOTIFICATION: &str = "plugin.ui.v1";
/// A service shows at most this many notices in this long, as Main holds each plugin to.
pub const SERVICE_NOTICES: usize = 3;
pub const SERVICE_NOTICE_WINDOW: Duration = Duration::from_secs(10);
/// Changes on their way to Main.
const WAITING_CHANGES: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoticeParams {
    text: String,
    #[serde(default)]
    tone: PluginUiTone,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusParams {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    tone: PluginUiTone,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingParams {
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NoParams {}

pub(super) fn parsed<T: DeserializeOwned>(params: Value) -> Result<T, ServiceError> {
    let params = if params.is_null() { json!({}) } else { params };
    serde_json::from_value(params)
        .map_err(|error| ServiceError::new("protocol.bad_params", format!("参数不对：{error}")))
}

pub(super) fn failed(error: &anyhow::Error) -> ServiceError {
    ServiceError::from_reason(&error.to_string())
}

/// `then` with the plugin as it is trusted now, once it may use `needed`.
pub(super) fn with_ready<T>(
    plugins: &PluginRuntime,
    plugin: &str,
    needed: PluginCapability,
    then: impl FnOnce(&ReadyPlugin) -> Result<T, ServiceError>,
) -> Result<T, ServiceError> {
    let ready = plugins.ready();
    let Some(found) = ready.iter().find(|ready| ready.package.manifest.name == plugin) else {
        let standing = plugins.state_of(plugin).map_or(Standing::Missing, Standing::Installed);
        return Err(failed(&unusable(plugin, standing)));
    };
    let declared = &found.package.manifest.declared.capabilities;
    permitted(plugin, Standing::Ready(declared), needed).map_err(|error| failed(&error))?;
    then(found)
}

pub struct ServiceBridge {
    plugins: Arc<PluginRuntime>,
    sessions: Arc<ServiceSessions>,
    runtime: tokio::runtime::Handle,
    changes: mpsc::Sender<PluginServiceUi>,
    main: Arc<Mutex<Option<AsyncPriorityQueue>>>,
    /// Each running service's status entry, sent again once Main connects. Changes are queued
    /// while this is held, so they reach Main in the order the entries changed.
    entries: Mutex<BTreeMap<String, PluginStatus>>,
    notices: Mutex<BTreeMap<String, VecDeque<Instant>>>,
    /// The channels each ready service connected.
    channels: Mutex<BTreeMap<String, Vec<ChannelAbilities>>>,
    /// The command menu each channel that shows one was sent last, by plugin and channel, while
    /// its service runs.
    menus: Mutex<BTreeMap<(String, String), Vec<MenuCommand>>>,
    /// The plugins whose service the host reported on, until it no longer runs.
    reported: Mutex<BTreeSet<String>>,
}

impl ServiceBridge {
    #[must_use]
    pub fn new(plugins: Arc<PluginRuntime>, runtime: tokio::runtime::Handle) -> Arc<Self> {
        let (changes, mut waiting) = mpsc::channel::<PluginServiceUi>(WAITING_CHANGES);
        let main: Arc<Mutex<Option<AsyncPriorityQueue>>> = Arc::default();
        let to = Arc::clone(&main);
        runtime.spawn(async move {
            while let Some(change) = waiting.recv().await {
                let queue = to.lock().ok().and_then(|queue| queue.clone());
                // Before Main connects there is no window; the entries are sent once it does.
                if let Some(queue) = queue {
                    let _ = send_notification(&queue, QueuePriority::P1, UI_NOTIFICATION, &change)
                        .await;
                }
            }
        });
        Arc::new(Self {
            sessions: ServiceSessions::new(Arc::clone(&plugins), runtime.clone()),
            plugins,
            runtime,
            changes,
            main,
            entries: Mutex::default(),
            notices: Mutex::default(),
            channels: Mutex::default(),
            menus: Mutex::default(),
            reported: Mutex::default(),
        })
    }

    /// Main's connection, on each of Main's requests; the first time, the entries the services
    /// hold now are sent.
    pub fn set_publisher(&self, queue: AsyncPriorityQueue) {
        let Ok(mut main) = self.main.lock() else { return };
        let first = main.is_none();
        *main = Some(queue);
        drop(main);
        if first && let Ok(entries) = self.entries.lock() {
            for status in entries.values() {
                let _ = self.changes.try_send(PluginServiceUi::Status { status: status.clone() });
            }
        }
    }

    /// The channels the plugin's running service connected.
    #[must_use]
    pub fn channels(&self, plugin: &str) -> Vec<ChannelAbilities> {
        let channels = self.channels.lock().ok();
        channels.and_then(|channels| channels.get(plugin).cloned()).unwrap_or_default()
    }

    pub(crate) fn sessions(&self) -> &Arc<ServiceSessions> {
        &self.sessions
    }

    /// Whether the channel should be sent `menu`: since its service connected it was sent none,
    /// or another one. It is kept as sent.
    pub(crate) fn menu_due(&self, plugin: &str, channel: &str, menu: &[MenuCommand]) -> bool {
        let Ok(mut menus) = self.menus.lock() else { return false };
        let key = (plugin.to_owned(), channel.to_owned());
        if menus.get(&key).is_some_and(|sent| sent.as_slice() == menu) {
            return false;
        }
        menus.insert(key, menu.to_vec());
        true
    }

    /// The menu did not reach the service: it is sent again on the next read of the plugins.
    pub(crate) fn menu_lost(&self, plugin: &str, channel: &str) {
        if let Ok(mut menus) = self.menus.lock() {
            menus.remove(&(plugin.to_owned(), channel.to_owned()));
        }
    }

    /// The plugin page shows the plugin's service as its settings say now; the host no longer
    /// runs it, and nothing more is owed to its chats.
    pub(crate) fn forget(&self, plugin: &str) {
        let known = self.reported.lock().is_ok_and(|mut reported| reported.remove(plugin));
        if known {
            self.plugins.set_service_status(plugin, None);
            self.changed(plugin);
        }
        self.sessions.forget(plugin);
    }

    /// A service that should run and cannot be started now: the plugin page says why.
    pub(crate) fn hold(&self, plugin: &str, status: PluginServiceStatus) {
        ServiceCalls::status(self, plugin, &status);
    }

    /// The plugins the host reported on.
    pub(crate) fn reported(&self) -> BTreeSet<String> {
        self.reported.lock().map(|reported| reported.clone()).unwrap_or_default()
    }

    /// The plugin page is told the service's state changed; one lost to a full queue is made
    /// up by the next.
    fn changed(&self, plugin: &str) {
        let _ = self.changes.try_send(PluginServiceUi::Service { plugin: plugin.to_owned() });
    }

    /// A change that must arrive: queued now, or as soon as there is room.
    fn must_send(&self, change: PluginServiceUi) {
        if let Err(mpsc::error::TrySendError::Full(change)) = self.changes.try_send(change) {
            let changes = self.changes.clone();
            self.runtime.spawn(async move {
                let _ = changes.send(change).await;
            });
        }
    }

    fn queue(&self, change: PluginServiceUi) -> Result<(), ServiceError> {
        self.changes.try_send(change).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                ServiceError::new("plugin_session.window_busy", "Aura 的窗口暂时收不过来，稍后再发")
            }
            mpsc::error::TrySendError::Closed(_) => {
                ServiceError::new("aura.stopping", "Aura 正在退出")
            }
        })
    }

    fn notify(&self, plugin: &str, params: Value) -> Result<Value, ServiceError> {
        let params: NoticeParams = parsed(params)?;
        with_ready(&self.plugins, plugin, PluginCapability::Notify, |_| Ok(()))?;
        let text = checked_notice(&params.text).map_err(|error| failed(&error))?;
        let now = Instant::now();
        let mut notices = self
            .notices
            .lock()
            .map_err(|_| ServiceError::new("aura.failed", "提示的记录不可用"))?;
        let recent = notices.entry(plugin.to_owned()).or_default();
        while recent.front().is_some_and(|at| now.duration_since(*at) >= SERVICE_NOTICE_WINDOW) {
            recent.pop_front();
        }
        if recent.len() >= SERVICE_NOTICES {
            return Err(ServiceError::new(
                "plugin_session.notice_too_many",
                format!(
                    "{} 秒内最多显示 {SERVICE_NOTICES} 条提示，等一会儿再发",
                    SERVICE_NOTICE_WINDOW.as_secs()
                ),
            ));
        }
        let notice = PluginNotice { plugin: plugin.to_owned(), text, tone: params.tone };
        self.queue(PluginServiceUi::Notice { notice })?;
        recent.push_back(now);
        Ok(json!({}))
    }

    fn status_entry(&self, plugin: &str, params: Value) -> Result<Value, ServiceError> {
        let params: StatusParams = parsed(params)?;
        with_ready(&self.plugins, plugin, PluginCapability::Status, |_| Ok(()))?;
        let text = checked_status(params.text.as_deref()).map_err(|error| failed(&error))?;
        let status = PluginStatus { plugin: plugin.to_owned(), text, tone: params.tone };
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| ServiceError::new("aura.failed", "状态条目的记录不可用"))?;
        self.queue(PluginServiceUi::Status { status: status.clone() })?;
        match status.text {
            Some(_) => entries.insert(plugin.to_owned(), status),
            None => entries.remove(plugin),
        };
        Ok(json!({}))
    }

    /// The values of the plugin's text settings now; secret ones are only in its environment.
    fn settings(&self, plugin: &str, params: Value) -> Result<Value, ServiceError> {
        let NoParams {} = parsed(params)?;
        with_ready(&self.plugins, plugin, PluginCapability::Settings, |ready| {
            Ok(json!({ "values": ready.text_values() }))
        })
    }

    /// Sets one of the plugin's text settings as its panel does; the service then starts again
    /// with the new value.
    fn set_setting(&self, plugin: &str, params: Value) -> Result<Value, ServiceError> {
        let SettingParams { key, value } = parsed(params)?;
        with_ready(&self.plugins, plugin, PluginCapability::Settings, |_| Ok(()))?;
        let values = self.plugins.set_text_setting(plugin, &key, &value);
        Ok(json!({ "values": values.map_err(|error| failed(&error))? }))
    }
}

impl ServiceCalls for ServiceBridge {
    fn call(&self, plugin: &str, method: &str, params: Value) -> CallFuture {
        if let Some(needed) = ServiceSessions::needs(method) {
            return self.sessions.call(plugin, method, needed, params);
        }
        // Each runs here, on the service's own reading thread, so what it shows keeps the order
        // the service sent it in.
        let outcome = match method {
            "notify" => self.notify(plugin, params),
            "status" => self.status_entry(plugin, params),
            "settings.get" => self.settings(plugin, params),
            "settings.set" => self.set_setting(plugin, params),
            _ => Err(ServiceError::new(
                "protocol.unknown_method",
                format!("没有方法 {}", shown(method)),
            )),
        };
        Box::pin(std::future::ready(outcome))
    }

    fn log(&self, plugin: &str, part: &str, text: &str) {
        self.plugins.log(plugin, PluginLogSource::Service, part, text);
    }

    fn status(&self, plugin: &str, status: &PluginServiceStatus) {
        if let Ok(mut reported) = self.reported.lock() {
            reported.insert(plugin.to_owned());
        }
        self.plugins.set_service_status(plugin, Some(status.clone()));
        self.changed(plugin);
    }

    fn ready(&self, plugin: &str, channels: &[ChannelAbilities]) {
        if let Ok(mut connected) = self.channels.lock() {
            connected.insert(plugin.to_owned(), channels.to_vec());
        }
    }

    fn ended(&self, plugin: &str) {
        if let Ok(mut connected) = self.channels.lock() {
            connected.remove(plugin);
        }
        if let Ok(mut menus) = self.menus.lock() {
            menus.retain(|(menu_plugin, _), _| menu_plugin != plugin);
        }
        self.sessions.ended(plugin);
        let Ok(mut entries) = self.entries.lock() else { return };
        entries.remove(plugin);
        self.must_send(PluginServiceUi::Ended { plugin: plugin.to_owned() });
    }
}
