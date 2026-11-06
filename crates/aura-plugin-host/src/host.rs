//! Keeps each plugin's background service running while it should: started when the plugin page
//! turned it on and the plugin can be used, stopped otherwise, started again after it ended with
//! a growing wait. Everything a service writes is read on the service's own thread and its
//! requests run on Core's runtime, so a service that floods, stalls or crashes never holds Aura
//! up. Starting and stopping a process happen outside the host's own lock.
use crate::backoff::Backoff;
use crate::instance::{Instance, Unsent, follow};
use crate::launch::{Launcher, ServiceLaunch, ServiceProcess};
use crate::protocol::{ChannelAbilities, PROTOCOL_VERSION, ServiceError, ToService};
use crate::redact::Redactor;
use aura_domain_common::UtcTimestamp;
use aura_plugins::PluginServiceStatus;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// A service has this long after it started to say it is ready.
pub const READY_WITHIN: Duration = Duration::from_secs(30);
/// A service asked to end has this long before its process tree is ended.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
pub const MAX_IN_FLIGHT: usize = 16;

pub type CallFuture = Pin<Box<dyn Future<Output = Result<Value, ServiceError>> + Send>>;

/// What the host hands on. Everything but `call` runs while the host holds its own state, so it
/// must return at once and never call the host.
pub trait ServiceCalls: Send + Sync + 'static {
    /// A request from a ready service; the answer is written when the future ends.
    fn call(&self, plugin: &str, method: &str, params: Value) -> CallFuture;
    /// One line for the plugin's log, the service's secrets already taken out.
    fn log(&self, plugin: &str, part: &str, text: &str);
    fn status(&self, plugin: &str, status: &PluginServiceStatus);
    /// The service is ready, with the channels it connected.
    fn ready(&self, plugin: &str, channels: &[ChannelAbilities]);
    /// The service is gone, for whatever reason.
    fn ended(&self, plugin: &str);
}

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

enum Phase {
    /// To be started at `at`.
    Waiting {
        at: Instant,
    },
    /// Being started outside the lock.
    Launching {
        generation: u64,
    },
    Starting {
        instance: Arc<Instance>,
        deadline: Instant,
    },
    Running {
        instance: Arc<Instance>,
    },
    /// Asked to end because what it is started as changed; it starts again once it ended.
    Restarting {
        instance: Arc<Instance>,
        deadline: Instant,
    },
    /// Not started again until what it is started as changes.
    Stopped,
}

struct Slot {
    launch: ServiceLaunch,
    backoff: Backoff,
    phase: Phase,
}

/// A service whose plugin no longer wants it, asked to end.
struct Leaving {
    instance: Arc<Instance>,
    deadline: Instant,
}

#[derive(Default)]
struct State {
    slots: BTreeMap<String, Slot>,
    leaving: Vec<Leaving>,
    generation: u64,
}

pub(crate) struct Inner {
    launcher: Arc<dyn Launcher>,
    pub(crate) calls: Arc<dyn ServiceCalls>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) runtime: tokio::runtime::Handle,
    state: Mutex<State>,
}

pub struct PluginHost {
    inner: Arc<Inner>,
}

fn stop_all(processes: Vec<Box<dyn ServiceProcess>>) {
    for mut process in processes {
        process.stop();
    }
}

impl PluginHost {
    pub fn new(
        launcher: Arc<dyn Launcher>,
        calls: Arc<dyn ServiceCalls>,
        clock: Arc<dyn Clock>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self { inner: Arc::new(Inner { launcher, calls, clock, runtime, state: Mutex::default() }) }
    }

    /// Makes the running services the `wanted` ones: a new one starts, one no longer wanted is
    /// asked to end, one whose identity changed is started again.
    pub fn reconcile(&self, wanted: Vec<ServiceLaunch>) {
        let inner = &self.inner;
        let now = inner.clock.now();
        let mut stops = Vec::new();
        if let Ok(mut state) = inner.state.lock() {
            let names: Vec<String> = wanted.iter().map(|launch| launch.plugin.clone()).collect();
            let gone: Vec<String> =
                state.slots.keys().filter(|name| !names.contains(name)).cloned().collect();
            for name in gone {
                let Some(slot) = state.slots.remove(&name) else { continue };
                if let Some(instance) = slot.phase.instance() {
                    let _ = instance.send(&ToService::Shutdown);
                    state.leaving.push(Leaving { instance, deadline: now + SHUTDOWN_GRACE });
                }
            }
            for launch in wanted {
                let name = launch.plugin.clone();
                match state.slots.get_mut(&name) {
                    None => {
                        let phase = Phase::Waiting { at: now };
                        state
                            .slots
                            .insert(name, Slot { launch, backoff: Backoff::default(), phase });
                    }
                    Some(slot) if slot.launch.identity == launch.identity => {}
                    Some(slot) => {
                        slot.launch = launch;
                        slot.backoff.reset();
                        slot.phase = match std::mem::replace(&mut slot.phase, Phase::Stopped) {
                            Phase::Starting { instance, .. } | Phase::Running { instance } => {
                                let _ = instance.send(&ToService::Shutdown);
                                Phase::Restarting { instance, deadline: now + SHUTDOWN_GRACE }
                            }
                            restarting @ Phase::Restarting { .. } => restarting,
                            Phase::Launching { .. } | Phase::Waiting { .. } | Phase::Stopped => {
                                Phase::Waiting { at: now }
                            }
                        };
                    }
                }
            }
            stops = expired(&mut state, now);
        }
        stop_all(stops);
        self.tick();
    }

    /// Starts what is due, and ends what did not answer in time.
    pub fn tick(&self) {
        let inner = &self.inner;
        let now = inner.clock.now();
        let mut due = Vec::new();
        let mut stops = Vec::new();
        if let Ok(mut state) = inner.state.lock() {
            stops = expired(&mut state, now);
            let mut generation = state.generation;
            for slot in state.slots.values_mut() {
                if matches!(slot.phase, Phase::Waiting { at } if at <= now) {
                    generation += 1;
                    slot.phase = Phase::Launching { generation };
                    due.push(generation);
                }
            }
            state.generation = generation;
        }
        stop_all(stops);
        for generation in due {
            self.launch(generation);
        }
    }

    fn launch(&self, generation: u64) {
        let inner = &self.inner;
        let (sender, receiver) = mpsc::channel();
        let launch = {
            let Ok(state) = inner.state.lock() else { return };
            let Some(slot) = state.slots.values().find(|slot| slot.phase.launching(generation))
            else {
                return;
            };
            slot.launch.clone()
        };
        // Finding and reading the program can take a while; nothing waits for it.
        let started = inner.launcher.start(&launch, sender);
        drop(launch);
        let now = inner.clock.now();
        let Ok(mut state) = inner.state.lock() else { return };
        let Some(slot) = state.slots.values_mut().find(|slot| slot.phase.launching(generation))
        else {
            drop(state);
            stop_all(started.into_iter().collect());
            return;
        };
        let plugin = slot.launch.plugin.clone();
        let process = match started {
            Ok(process) => process,
            Err(reason) => {
                let wait = slot.backoff.ended(Duration::ZERO);
                slot.phase = Phase::Waiting { at: now + wait };
                let reason = format!("没能启动：{reason}");
                inner.calls.log(&plugin, "aura", &reason);
                inner.calls.status(&plugin, &restarting(wait, reason));
                return;
            }
        };
        let secrets = slot.launch.secrets.iter().map(|(_, value)| value.to_string());
        let declared = slot.launch.channels.iter().map(|channel| channel.id.clone()).collect();
        let instance = Arc::new(Instance::new(
            generation,
            plugin.clone(),
            now,
            process,
            declared,
            Redactor::new(secrets),
        ));
        let hello = ToService::Hello {
            protocol: PROTOCOL_VERSION,
            plugin: plugin.clone(),
            channels: slot.launch.channels.clone(),
            settings: slot.launch.settings.clone(),
        };
        let _ = instance.send(&hello);
        slot.phase =
            Phase::Starting { instance: Arc::clone(&instance), deadline: now + READY_WITHIN };
        inner.calls.status(&plugin, &PluginServiceStatus::Starting);
        drop(state);
        let (follower, inner) = (Arc::clone(&instance), Arc::clone(inner));
        let spawned = std::thread::Builder::new()
            .name(format!("aura-plugin-service-{plugin}"))
            .spawn(move || follow(inner, follower, receiver));
        if spawned.is_err() {
            self.inner.stuck(&instance);
        }
    }

    /// Gives a running service that said it is ready a line of Aura's own, such as a reply. One
    /// that stopped reading is ended, as when it stops reading its answers.
    pub fn deliver(&self, plugin: &str, message: &ToService) -> Result<(), String> {
        let not_running = || format!("插件 {plugin} 的后台服务现在没有在运行");
        let instance = {
            let state = self.inner.state.lock().map_err(|_| not_running())?;
            match state.slots.get(plugin).map(|slot| &slot.phase) {
                Some(Phase::Running { instance }) => Arc::clone(instance),
                _ => return Err(not_running()),
            }
        };
        match instance.send(message) {
            Ok(()) => Ok(()),
            Err(Unsent::Gone) => Err(not_running()),
            Err(Unsent::Stuck) => {
                self.inner.stuck(&instance);
                Err(format!("插件 {plugin} 的后台服务不再读 Aura 发给它的内容"))
            }
        }
    }

    /// Every service is asked to end and its process tree ended.
    pub fn stop_everything(&self) {
        self.reconcile(Vec::new());
        let deadline = self.inner.clock.now() + SHUTDOWN_GRACE;
        let stops = match self.inner.state.lock() {
            Ok(mut state) => expired(&mut state, deadline),
            Err(_) => Vec::new(),
        };
        stop_all(stops);
    }

    /// Whether any service is still being stopped.
    #[must_use]
    pub fn idle(&self) -> bool {
        self.inner
            .state
            .lock()
            .is_ok_and(|state| state.slots.is_empty() && state.leaving.is_empty())
    }
}

impl Phase {
    fn instance(&self) -> Option<Arc<Instance>> {
        match self {
            Self::Starting { instance, .. }
            | Self::Running { instance }
            | Self::Restarting { instance, .. } => Some(Arc::clone(instance)),
            Self::Waiting { .. } | Self::Launching { .. } | Self::Stopped => None,
        }
    }

    fn launching(&self, generation: u64) -> bool {
        matches!(self, Self::Launching { generation: g } if *g == generation)
    }

    fn holds(&self, generation: u64) -> bool {
        self.instance().is_some_and(|instance| instance.generation == generation)
    }
}

fn restarting(wait: Duration, reason: String) -> PluginServiceStatus {
    PluginServiceStatus::Restarting {
        seconds: u32::try_from(wait.as_secs()).unwrap_or(u32::MAX),
        reason,
    }
}

/// The processes whose time ran out: a service that never said it was ready, or one asked to end.
fn expired(state: &mut State, now: Instant) -> Vec<Box<dyn ServiceProcess>> {
    let mut stops = Vec::new();
    for slot in state.slots.values() {
        match &slot.phase {
            Phase::Starting { instance, deadline } if *deadline <= now => {
                let why = format!("启动后 {} 秒内没有发 ready", READY_WITHIN.as_secs());
                stops.extend(instance.take(Some(why)));
            }
            Phase::Restarting { instance, deadline } if *deadline <= now => {
                stops.extend(instance.take(None));
            }
            _ => {}
        }
    }
    for leaving in &state.leaving {
        if leaving.deadline <= now {
            stops.extend(leaving.instance.take(None));
        }
    }
    stops
}

impl Inner {
    fn slot_of<'a>(state: &'a mut State, instance: &Instance) -> Option<&'a mut Slot> {
        state.slots.get_mut(&instance.plugin).filter(|slot| slot.phase.holds(instance.generation))
    }

    /// The service said it is ready.
    pub(crate) fn running(&self, instance: &Instance, channels: &[ChannelAbilities]) {
        let Ok(mut state) = self.state.lock() else { return };
        let Some(slot) = Self::slot_of(&mut state, instance) else { return };
        let Phase::Starting { instance, .. } = &slot.phase else { return };
        slot.phase = Phase::Running { instance: Arc::clone(instance) };
        self.calls.ready(&slot.launch.plugin, channels);
        let status = PluginServiceStatus::Running { since: UtcTimestamp::now() };
        self.calls.status(&slot.launch.plugin, &status);
    }

    /// The service stopped reading what Aura sends: it is ended and started again later.
    pub(crate) fn stuck(&self, instance: &Instance) {
        let why = "服务不再读 Aura 发给它的内容".to_owned();
        if let Some(process) = instance.take(Some(why)) {
            // Stopping waits for the process tree; the caller may be one of Core's workers.
            std::thread::spawn(move || stop_all(vec![process]));
        }
    }

    /// The service cannot work with this Aura: it is ended and not started again until what it
    /// is started as changes.
    pub(crate) fn stop_for_good(&self, instance: &Instance, why: String) {
        if let Ok(mut state) = self.state.lock()
            && let Some(slot) = Self::slot_of(&mut state, instance)
        {
            slot.phase = Phase::Stopped;
            self.calls.log(&instance.plugin, "aura", &why);
            self.calls
                .status(&instance.plugin, &PluginServiceStatus::Stopped { reason: why.clone() });
        }
        let process = instance.take(Some(why));
        // Stopping waits for the process tree; this runs on the service's own reading thread.
        stop_all(process.into_iter().collect());
    }

    /// The service's process is gone.
    pub(crate) fn ended(&self, instance: &Instance, exit_code: Option<u32>, memory: bool) {
        let now = self.clock.now();
        let Ok(mut state) = self.state.lock() else { return };
        state.leaving.retain(|leaving| leaving.instance.generation != instance.generation);
        self.calls.ended(&instance.plugin);
        let Some(slot) = state.slots.get_mut(&instance.plugin) else { return };
        if !slot.phase.holds(instance.generation) {
            return;
        }
        if matches!(slot.phase, Phase::Restarting { .. }) {
            slot.phase = Phase::Waiting { at: now };
            return;
        }
        let why = instance.stopped_because().unwrap_or_else(|| match (exit_code, memory) {
            (_, true) => "服务用完了插件的内存上限，被结束".to_owned(),
            (Some(code), false) => format!("服务结束了（退出码 {code}）"),
            (None, false) => "服务结束了".to_owned(),
        });
        let wait = slot.backoff.ended(now.duration_since(instance.started));
        slot.phase = Phase::Waiting { at: now + wait };
        self.calls.log(
            &instance.plugin,
            "aura",
            &format!("{why}；{} 秒后重新启动", wait.as_secs()),
        );
        self.calls.status(&instance.plugin, &restarting(wait, why));
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.stop_everything();
    }
}
