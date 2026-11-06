//! A plugin's background service through Core's own pieces: the plugin added and trusted on the
//! plugin page's routes, its service turned on there, run by Node.js. What it asks is checked
//! against what its manifest declares; what it shows reaches Main as `plugin.ui.v1` in order;
//! once its switch is off the service ends and what it showed goes with it.
mod support;

use aura_backend_windows_user::program::SessionEvent;
use aura_ipc::{AsyncPriorityQueue, WireFrame};
use aura_plugin_host::{Launcher, ServiceLaunch, ServiceProcess};
use aura_plugin_session::{PluginServices, SERVICE_NOTICES, UI_NOTIFICATION, service_identity};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use support::{Fixture, NODE_SERVICE, answer, eventually};

/// Sends each request once it is ready and logs every answer as `<id> ok <result>` or
/// `<id> <code>`.
const SERVICE: &str = r#"
import { createInterface } from 'node:readline';
const say = (message) => process.stdout.write(JSON.stringify(message) + '\n');
const requests = [
  ['notify', { text: '已连接 ' + process.env.AURA_PLUGIN_NAME, tone: 'success' }],
  ['status', { text: '在线' }],
  ['settings.get', {}],
  ['sessions.configure', { scope: 'all' }],
  ['notify', { text: '第二条' }],
  ['notify', { text: '第三条' }],
  ['notify', { text: '第四条' }],
  ['status', { text: '第一行\n第二行' }],
];
createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.type === 'hello') {
    say({ type: 'ready', protocol: 1 });
    requests.forEach(([method, params], id) => say({ type: 'request', id, method, params }));
  } else if (message.type === 'response') {
    const outcome = message.error ? message.error.code : 'ok ' + JSON.stringify(message.result);
    say({ type: 'log', level: 'info', text: `${message.id} ${outcome}` });
  } else if (message.type === 'shutdown') {
    process.exit(0);
  }
});
"#;

/// A plugin in development with this test's service and the capabilities named, trusted.
fn plugin(f: &Fixture, name: &str, capabilities: &str) {
    let manifest = format!(
        "capabilities = [{capabilities}]\n\n{NODE_SERVICE}\n\
         [[settings]]\nkey = \"app_id\"\ntitle = \"AppID\"\nrequired = true\n"
    );
    f.plugin(name, &manifest, SERVICE);
}

/// Every `plugin.ui.v1` notification Main is sent.
fn shown(queue: &AsyncPriorityQueue) -> Arc<Mutex<Vec<Value>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (queue, into) = (queue.clone(), Arc::clone(&seen));
    tokio::spawn(async move {
        while let Ok(frame) = queue.recv().await {
            let WireFrame::Control(bytes) = frame.frame else { continue };
            let message: Value = serde_json::from_slice(&bytes).expect("JSON");
            assert_eq!(message["method"], UI_NOTIFICATION);
            into.lock().expect("seen").push(message["params"].clone());
        }
    });
    seen
}

/// Fails every start. Once told to hold, a start waits until it is let go, so the host does
/// nothing in the meantime.
#[derive(Default)]
struct Refusing {
    holding: AtomicBool,
    held: AtomicUsize,
    released: (Mutex<bool>, Condvar),
}

impl Refusing {
    fn release(&self) {
        let (released, wake) = &self.released;
        *released.lock().expect("released") = true;
        wake.notify_all();
    }
}

/// Lets a held start go when the test ends, also when it fails: the runtime waits for it.
struct LetGo(Arc<Refusing>);

impl Drop for LetGo {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl Launcher for Refusing {
    fn start(
        &self,
        _: &ServiceLaunch,
        _: std::sync::mpsc::Sender<SessionEvent>,
    ) -> Result<Box<dyn ServiceProcess>, String> {
        if self.holding.load(Ordering::SeqCst) {
            self.held.fetch_add(1, Ordering::SeqCst);
            let (released, wake) = &self.released;
            let mut released = released.lock().expect("released");
            while !*released {
                released = wake.wait(released).expect("released");
            }
        }
        Err("这里不启动程序".into())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_service_asks_only_what_its_plugin_declares_and_shows_it_in_order() {
    let f = Fixture::new();
    plugin(&f, "chat-bot", "\"notify\", \"status\", \"settings\"");
    plugin(&f, "quiet-bot", "\"status\"");
    for name in ["chat-bot", "quiet-bot"] {
        f.configure(name, json!({"service": true, "text": {"app_id": "1024"}}));
    }
    let queue = AsyncPriorityQueue::new();
    let seen = shown(&queue);
    let services =
        PluginServices::start(Arc::clone(&f.plugins), &tokio::runtime::Handle::current());
    services.set_publisher(queue.clone());

    eventually("both answered", || {
        ["chat-bot", "quiet-bot"].iter().all(|name| answer(&f.logged(name), 7).is_some())
    })
    .await;
    let chat = f.logged("chat-bot");
    assert_eq!(answer(&chat, 0).as_deref(), Some("ok {}"));
    assert_eq!(answer(&chat, 1).as_deref(), Some("ok {}"));
    assert_eq!(answer(&chat, 2).as_deref(), Some(r#"ok {"values":{"app_id":"1024"}}"#));
    assert_eq!(answer(&chat, 3).as_deref(), Some("protocol.unknown_method"), "no way to widen");
    let fourth = answer(&chat, 6);
    assert_eq!(fourth.as_deref(), Some("plugin_session.notice_too_many"), "{SERVICE_NOTICES} max");
    assert_eq!(answer(&chat, 7).as_deref(), Some("plugin_session.status_invalid"));
    let quiet = f.logged("quiet-bot");
    assert_eq!(answer(&quiet, 0).as_deref(), Some("plugin_session.capability_missing"));
    assert_eq!(answer(&quiet, 1).as_deref(), Some("ok {}"));
    assert_eq!(answer(&quiet, 2).as_deref(), Some("plugin_session.capability_missing"));

    let changes = seen.lock().expect("seen").clone();
    let mine: Vec<&Value> = changes
        .iter()
        .filter(|change| change["kind"] != "service" && change.to_string().contains("chat-bot"))
        .collect();
    assert_eq!(
        mine[0]["notice"],
        json!({"plugin": "chat-bot", "text": "已连接 chat-bot", "tone": "success"})
    );
    assert_eq!(mine[1]["status"], json!({"plugin": "chat-bot", "text": "在线", "tone": "info"}));
    let notices = mine.iter().filter(|change| change["kind"] == "notice").count();
    assert_eq!(notices, SERVICE_NOTICES);
    assert!(
        changes.iter().any(|change| *change == json!({"kind": "service", "plugin": "quiet-bot"}))
    );
    assert_eq!(f.view("chat-bot")["service"]["status"]["state"], "running");

    f.configure("chat-bot", json!({"service": false}));
    eventually("its entry cleared", || {
        seen.lock()
            .expect("seen")
            .iter()
            .any(|change| *change == json!({"kind": "ended", "plugin": "chat-bot"}))
    })
    .await;
    eventually("shown as off", || {
        f.view("chat-bot")["service"]["status"] == json!({"state": "off"})
    })
    .await;
    assert_eq!(f.view("quiet-bot")["service"]["status"]["state"], "running");
    services.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_service_turned_off_and_on_again_shows_nothing_of_its_last_run() {
    let f = Fixture::new();
    plugin(&f, "chat-bot", "\"status\"");
    f.configure("chat-bot", json!({"service": true, "text": {"app_id": "1024"}}));
    let launcher = Arc::new(Refusing::default());
    let _let_go = LetGo(Arc::clone(&launcher));
    let services = PluginServices::start_with(
        Arc::clone(&f.plugins),
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        &tokio::runtime::Handle::current(),
    );
    eventually("its failed start shown", || {
        f.view("chat-bot")["service"]["status"]["state"] == "restarting"
    })
    .await;
    // The host waits in its next start, so only the test changes anything from here on.
    launcher.holding.store(true, Ordering::SeqCst);
    eventually("the next start held", || launcher.held.load(Ordering::SeqCst) > 0).await;

    f.configure("chat-bot", json!({"service": false}));
    let reconciling = Arc::clone(&services);
    tokio::task::spawn_blocking(move || reconciling.reconcile()).await.expect("reconciled");
    f.configure("chat-bot", json!({"service": true}));
    assert_eq!(
        f.view("chat-bot")["service"]["status"],
        json!({"state": "starting"}),
        "not the restart it was waiting for when it was turned off"
    );
    services.stop();
}

#[test]
fn a_service_is_started_again_when_its_trust_or_settings_change_and_not_otherwise() {
    let f = Fixture::new();
    plugin(&f, "chat-bot", "\"status\"");
    let identity = || {
        let ready = f.plugins.ready();
        let plugin = ready.iter().find(|ready| ready.package.manifest.name == "chat-bot");
        service_identity(plugin.expect("ready"))
    };
    let first = identity();
    f.configure("chat-bot", json!({"service": true}));
    assert_eq!(identity(), first, "turning it on is not a new start");
    f.configure("chat-bot", json!({"text": {"app_id": "1024"}}));
    let set = identity();
    assert_ne!(set, first, "a setting's value is given at the start");
    f.configure("chat-bot", json!({"scope": {"workspaces": [], "sessions": []}}));
    assert_eq!(identity(), set, "the scope is Aura's, not the service's");
    let folder = f.root.path().join("sources").join("chat-bot");
    std::fs::write(folder.join("service.mjs"), format!("{SERVICE}\n// changed\n")).expect("edit");
    let preview = f.call("agent.plugins.preview", json!({"name": "chat-bot"}));
    f.call(
        "agent.plugins.trust",
        json!({"name": "chat-bot", "expected_fingerprint": preview["fingerprint"]}),
    );
    assert_ne!(identity(), set, "what was trusted changed");
}
