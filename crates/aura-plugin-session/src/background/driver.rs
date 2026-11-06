//! Keeps the plugins' background services the ones that should run. Every two seconds the
//! plugins are read again, the host is told which services should run and what each is started
//! as, and the channels that show a command menu are sent theirs when it changed (`menus`); in
//! between, the host starts what is due and ends what did not answer in time. Every second the
//! replies owed to their chats are sent (`pump`).
use super::bridge::ServiceBridge;
use super::pump::PUMP_EVERY;
use super::sessions::AgentAccess;
use super::wanted::{launch_of, service_identity, should_run};
use aura_extension_service::plugins::PluginRuntime;
use aura_ipc::AsyncPriorityQueue;
use aura_plugin_host::{
    Clock, Launcher, PluginHost, ProgramLauncher, SHUTDOWN_GRACE, ServiceCalls, ServiceLaunch,
    SystemClock,
};
use aura_plugins::PluginServiceStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the host starts what is due, and every how many of those the plugins are read.
const TICK: Duration = Duration::from_millis(500);
const TICKS_PER_READ: u32 = 4;

pub struct PluginServices {
    host: PluginHost,
    bridge: Arc<ServiceBridge>,
    pub(super) plugins: Arc<PluginRuntime>,
    /// What each service that should run is started as, so its secrets are read again only
    /// when that changes.
    known: Mutex<BTreeMap<String, ServiceLaunch>>,
    /// When the folders of plugins no longer installed are removed: at start, and once a
    /// service that stopped should have ended.
    sweep_at: Mutex<Option<Instant>>,
    stopped: AtomicBool,
}

impl PluginServices {
    /// Starts keeping the services running on `runtime`, each a program of the user's in a Job
    /// of its own.
    #[must_use]
    pub fn start(plugins: Arc<PluginRuntime>, runtime: &tokio::runtime::Handle) -> Arc<Self> {
        Self::start_with(plugins, Arc::new(ProgramLauncher), runtime)
    }

    /// As [`Self::start`], with what starts the programs.
    #[must_use]
    pub fn start_with(
        plugins: Arc<PluginRuntime>,
        launcher: Arc<dyn Launcher>,
        runtime: &tokio::runtime::Handle,
    ) -> Arc<Self> {
        let bridge = ServiceBridge::new(Arc::clone(&plugins), runtime.clone());
        let calls = Arc::clone(&bridge) as Arc<dyn ServiceCalls>;
        let clock = Arc::new(SystemClock) as Arc<dyn Clock>;
        let host = PluginHost::new(launcher, calls, clock, runtime.clone());
        let services = Arc::new(Self {
            host,
            bridge,
            plugins,
            known: Mutex::default(),
            sweep_at: Mutex::new(Some(Instant::now())),
            stopped: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&services);
        runtime.spawn(async move {
            let mut every = tokio::time::interval(TICK);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            for tick in 0_u32.. {
                every.tick().await;
                let Some(services) = weak.upgrade() else { break };
                // Reading the plugins and starting a program block.
                let going = tokio::task::spawn_blocking(move || {
                    if services.stopped.load(Ordering::Acquire) {
                        return false;
                    }
                    if tick % TICKS_PER_READ == 0 {
                        services.reconcile();
                    } else {
                        services.host.tick();
                    }
                    true
                })
                .await;
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        });
        let weak = Arc::downgrade(&services);
        runtime.spawn(async move {
            let mut every = tokio::time::interval(PUMP_EVERY);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                every.tick().await;
                let Some(services) = weak.upgrade() else { break };
                if services.stopped.load(Ordering::Acquire) {
                    break;
                }
                services.pump().await;
            }
        });
        services
    }

    /// Core's way to its Agent application, for the services' calls in conversations.
    pub fn attach(&self, agent: AgentAccess) {
        self.bridge.sessions().attach(agent);
    }

    /// Main's connection, for what the services show in its window.
    pub fn set_publisher(&self, queue: AsyncPriorityQueue) {
        self.bridge.set_publisher(queue);
    }

    #[must_use]
    pub fn bridge(&self) -> &Arc<ServiceBridge> {
        &self.bridge
    }

    pub(super) fn host(&self) -> &PluginHost {
        &self.host
    }

    /// Makes the running services the ones that should run now. It blocks: it reads the plugins
    /// and, for a service started anew, its secrets.
    pub fn reconcile(&self) {
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        let ready = self.plugins.ready();
        let mut wanted = Vec::new();
        let mut held = Vec::new();
        if let Ok(mut known) = self.known.lock() {
            let mut next = BTreeMap::new();
            for plugin in ready.iter() {
                let Some(service) = should_run(&self.plugins, plugin) else { continue };
                let name = plugin.package.manifest.name.clone();
                let identity = service_identity(plugin);
                let launch = match known.remove(&name) {
                    Some(launch) if launch.identity == identity => Ok(launch),
                    _ => launch_of(&self.plugins, plugin, service, identity),
                };
                match launch {
                    Ok(launch) => {
                        wanted.push(launch.clone());
                        next.insert(name, launch);
                    }
                    Err(reason) => held.push((name, reason)),
                }
            }
            if !known.is_empty()
                && let Ok(mut sweep_at) = self.sweep_at.lock()
            {
                let after = Instant::now() + SHUTDOWN_GRACE + Duration::from_secs(1);
                sweep_at.get_or_insert(after);
            }
            *known = next;
        }
        let mut running: BTreeSet<String> =
            wanted.iter().map(|launch| launch.plugin.clone()).collect();
        self.host.reconcile(wanted);
        for (name, reason) in held {
            // Tried again on the next read.
            self.bridge.hold(&name, PluginServiceStatus::Waiting { reason });
            running.insert(name);
        }
        for name in self.bridge.reported() {
            if !running.contains(&name) {
                self.bridge.forget(&name);
            }
        }
        let due = self
            .sweep_at
            .lock()
            .is_ok_and(|mut sweep_at| sweep_at.take_if(|at| *at <= Instant::now()).is_some());
        if due {
            self.plugins.sweep_service_folders();
        }
        self.send_menus(&ready);
    }

    /// Every service is asked to end and its process tree ended; none starts again.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.host.stop_everything();
    }
}
