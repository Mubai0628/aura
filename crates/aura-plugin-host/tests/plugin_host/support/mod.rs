//! A launcher whose services are scripted by the test, a clock the test moves, and a
//! `ServiceCalls` that records everything the host hands on.
#![allow(dead_code)]
use aura_backend_windows_user::program::SessionEvent;
use aura_plugin_host::{
    CallFuture, ChannelAbilities, Clock, Launcher, PluginHost, ServiceCalls, ServiceError,
    ServiceLaunch, ServiceProcess,
};
use aura_plugins::{PluginChannel, PluginPlatform, PluginServiceStatus};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const SECRET: &str = "s3cret-value-77";

pub struct ManualClock(Mutex<Instant>);

impl ManualClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Instant::now())))
    }

    pub fn advance(&self, by: Duration) {
        *self.0.lock().expect("clock") += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        *self.0.lock().expect("clock")
    }
}

/// One started service the test speaks for.
pub struct FakeService {
    pub identity: String,
    written: Mutex<Vec<String>>,
    events: mpsc::Sender<SessionEvent>,
    pub full: AtomicBool,
    pub stopped: AtomicBool,
}

impl FakeService {
    pub fn say(&self, line: &str) {
        let _ = self.events.send(SessionEvent::Stdout(format!("{line}\n").into_bytes()));
    }

    pub fn say_json(&self, value: Value) {
        self.say(&value.to_string());
    }

    pub fn stderr(&self, text: &str) {
        let _ = self.events.send(SessionEvent::Stderr(text.as_bytes().to_vec()));
    }

    pub fn ready(&self) {
        self.say_json(json!({"type": "ready", "protocol": 1,
            "channels": [{"id": "main", "max_text_chars": 2000}]}));
    }

    /// The program ends on its own.
    pub fn exit(&self, code: u32) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.events.send(SessionEvent::Ended {
            exit_code: Some(code),
            stop_confirmed: true,
            memory_limit_reached: false,
        });
    }

    /// Every line Aura wrote to it, as JSON.
    pub fn lines(&self) -> Vec<Value> {
        self.written
            .lock()
            .expect("written")
            .iter()
            .map(|line| serde_json::from_str(line).expect("Aura writes JSON lines"))
            .collect()
    }

    pub fn raw(&self) -> String {
        self.written.lock().expect("written").concat()
    }

    /// The answer Aura wrote to request `id`, once it is there.
    pub fn answer(&self, id: Value) -> Value {
        eventually("the answer to a request", || {
            self.lines().into_iter().any(|line| line["type"] == "response" && line["id"] == id)
        });
        self.lines()
            .into_iter()
            .find(|line| line["type"] == "response" && line["id"] == id)
            .expect("answer")
    }

    /// The first `error` line with `code`, once it is there.
    pub fn error(&self, code: &str) -> Value {
        eventually(code, || {
            self.lines().into_iter().any(|line| line["type"] == "error" && line["code"] == code)
        });
        self.lines()
            .into_iter()
            .find(|line| line["type"] == "error" && line["code"] == code)
            .expect("error")
    }
}

struct FakeProcess(Arc<FakeService>);

impl ServiceProcess for FakeProcess {
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()> {
        if self.0.stopped.load(Ordering::Acquire) {
            anyhow::bail!("program.stdin_closed");
        }
        if self.0.full.load(Ordering::Acquire) {
            anyhow::bail!("program.stdin_backpressure");
        }
        self.0.written.lock().expect("written").push(String::from_utf8(line).expect("utf-8"));
        Ok(())
    }

    fn stop(&mut self) {
        if !self.0.stopped.swap(true, Ordering::AcqRel) {
            let _ = self.0.events.send(SessionEvent::Ended {
                exit_code: None,
                stop_confirmed: true,
                memory_limit_reached: false,
            });
        }
    }
}

#[derive(Default)]
pub struct FakeLauncher {
    started: Mutex<Vec<Arc<FakeService>>>,
    pub fail: Mutex<Option<String>>,
}

impl FakeLauncher {
    pub fn started(&self) -> Vec<Arc<FakeService>> {
        self.started.lock().expect("started").clone()
    }

    pub fn count(&self) -> usize {
        self.started().len()
    }

    pub fn last(&self) -> Arc<FakeService> {
        self.started().last().cloned().expect("a started service")
    }
}

impl Launcher for FakeLauncher {
    fn start(
        &self,
        launch: &ServiceLaunch,
        events: mpsc::Sender<SessionEvent>,
    ) -> Result<Box<dyn ServiceProcess>, String> {
        if let Some(reason) = self.fail.lock().expect("fail").clone() {
            return Err(reason);
        }
        let service = Arc::new(FakeService {
            identity: launch.identity.clone(),
            written: Mutex::default(),
            events,
            full: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        });
        self.started.lock().expect("started").push(Arc::clone(&service));
        Ok(Box::new(FakeProcess(service)))
    }
}

/// Records what the host hands on; `notify` answers at once, `held` waits for a permit.
pub struct Recorded {
    pub calls: Mutex<Vec<(String, String, Value)>>,
    pub logs: Mutex<Vec<(String, String, String)>>,
    pub statuses: Mutex<Vec<(String, PluginServiceStatus)>>,
    pub ready: Mutex<Vec<(String, Vec<ChannelAbilities>)>>,
    pub ended: Mutex<Vec<String>>,
    pub held: Arc<tokio::sync::Semaphore>,
}

impl Default for Recorded {
    fn default() -> Self {
        Self {
            calls: Mutex::default(),
            logs: Mutex::default(),
            statuses: Mutex::default(),
            ready: Mutex::default(),
            ended: Mutex::default(),
            held: Arc::new(tokio::sync::Semaphore::new(0)),
        }
    }
}

impl Recorded {
    pub fn status(&self) -> Option<PluginServiceStatus> {
        self.statuses.lock().expect("statuses").last().map(|(_, status)| status.clone())
    }

    pub fn logs(&self) -> Vec<(String, String)> {
        self.logs
            .lock()
            .expect("logs")
            .iter()
            .map(|(_, part, text)| (part.clone(), text.clone()))
            .collect()
    }

    pub fn ended(&self) -> usize {
        self.ended.lock().expect("ended").len()
    }
}

impl ServiceCalls for Recorded {
    fn call(&self, plugin: &str, method: &str, params: Value) -> CallFuture {
        self.calls.lock().expect("calls").push((plugin.into(), method.into(), params.clone()));
        let held = Arc::clone(&self.held);
        match method {
            "notify" => Box::pin(async move { Ok(json!({"shown": true, "params": params})) }),
            "held" => Box::pin(async move {
                let _permit = held.acquire().await.map_err(|_| ServiceError::new("x", "closed"))?;
                Ok(json!({"released": true}))
            }),
            other => {
                let message = format!("不认识的方法 {other}");
                Box::pin(async move { Err(ServiceError::new("protocol.unknown_method", message)) })
            }
        }
    }

    fn log(&self, plugin: &str, part: &str, text: &str) {
        self.logs.lock().expect("logs").push((plugin.into(), part.into(), text.into()));
    }

    fn status(&self, plugin: &str, status: &PluginServiceStatus) {
        self.statuses.lock().expect("statuses").push((plugin.into(), status.clone()));
    }

    fn ready(&self, plugin: &str, channels: &[ChannelAbilities]) {
        self.ready.lock().expect("ready").push((plugin.into(), channels.to_vec()));
    }

    fn ended(&self, plugin: &str) {
        self.ended.lock().expect("ended").push(plugin.into());
    }
}

pub fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn launch(identity: &str) -> ServiceLaunch {
    let folder = std::env::temp_dir().join("aura-plugin-host-unused");
    ServiceLaunch {
        plugin: "chat-bot".into(),
        identity: identity.into(),
        command: "service.exe".into(),
        arguments: Vec::new(),
        data_folder: folder.join("data"),
        scratch: folder.join("run"),
        limits: None,
        channels: vec![PluginChannel {
            id: "main".into(),
            platform: PluginPlatform::Other,
            title: "测试渠道".into(),
        }],
        settings: BTreeMap::from([("app_id".to_owned(), "1024".to_owned())]),
        secrets: vec![("AURA_SECRET_TOKEN".into(), Zeroizing::new(SECRET.to_owned()))],
    }
}

pub struct Rig {
    pub runtime: tokio::runtime::Runtime,
    pub clock: Arc<ManualClock>,
    pub launcher: Arc<FakeLauncher>,
    pub calls: Arc<Recorded>,
    pub host: PluginHost,
}

pub fn rig() -> Rig {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let clock = ManualClock::new();
    let launcher = Arc::new(FakeLauncher::default());
    let calls = Arc::new(Recorded::default());
    let host = PluginHost::new(
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        Arc::clone(&calls) as Arc<dyn ServiceCalls>,
        Arc::clone(&clock) as Arc<dyn Clock>,
        runtime.handle().clone(),
    );
    Rig { runtime, clock, launcher, calls, host }
}

impl Rig {
    /// A wanted service, started and ready.
    pub fn running(&self) -> Arc<FakeService> {
        self.host.reconcile(vec![launch("a")]);
        let service = self.launcher.last();
        service.ready();
        eventually("the service running", || {
            matches!(self.calls.status(), Some(PluginServiceStatus::Running { .. }))
        });
        service
    }
}
