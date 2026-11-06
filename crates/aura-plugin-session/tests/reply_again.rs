//! A reply its service could not take whole goes again, whole, to the service started after it:
//! the service that stopped reading is ended, and what Aura handed it may never have reached the
//! chat. The services are scripted by the test.
mod support;

use aura_backend_windows_user::program::SessionEvent;
use aura_plugin_host::{Launcher, ServiceLaunch, ServiceProcess};
use aura_plugin_session::{NO_SCOPE_REPLY, PluginServices};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use support::{Fixture, NODE_SERVICE, eventually};

/// One started service: what Aura wrote to it, and how many lines it reads before it stops.
struct Started {
    events: mpsc::Sender<SessionEvent>,
    written: Mutex<Vec<Value>>,
    reads: usize,
    ended: AtomicBool,
}

impl Started {
    fn say(&self, message: &Value) {
        let _ = self.events.send(SessionEvent::Stdout(format!("{message}\n").into_bytes()));
    }

    fn replies(&self) -> Vec<Value> {
        let written = self.written.lock().expect("written");
        written.iter().filter(|line| line["type"] == "reply").cloned().collect()
    }
}

struct Process(Arc<Started>);

impl ServiceProcess for Process {
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()> {
        let mut written = self.0.written.lock().expect("written");
        if written.len() >= self.0.reads {
            anyhow::bail!("program.stdin_backpressure");
        }
        written.push(serde_json::from_slice(&line).expect("a JSON line"));
        Ok(())
    }

    fn stop(&mut self) {
        if !self.0.ended.swap(true, Ordering::AcqRel) {
            let _ = self.0.events.send(SessionEvent::Ended {
                exit_code: None,
                stop_confirmed: true,
                memory_limit_reached: false,
            });
        }
    }
}

/// The first service reads `first` lines; the ones after it read everything.
struct Scripted {
    first: usize,
    started: Mutex<Vec<Arc<Started>>>,
}

impl Scripted {
    fn count(&self) -> usize {
        self.started.lock().expect("started").len()
    }

    fn nth(&self, at: usize) -> Arc<Started> {
        Arc::clone(&self.started.lock().expect("started")[at])
    }
}

impl Launcher for Scripted {
    fn start(
        &self,
        _: &ServiceLaunch,
        events: mpsc::Sender<SessionEvent>,
    ) -> Result<Box<dyn ServiceProcess>, String> {
        let mut started = self.started.lock().expect("started");
        let reads = if started.is_empty() { self.first } else { usize::MAX };
        let service = Arc::new(Started {
            events,
            written: Mutex::default(),
            reads,
            ended: AtomicBool::new(false),
        });
        started.push(Arc::clone(&service));
        Ok(Box::new(Process(service)))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_its_service_could_not_take_whole_goes_again_to_the_next_one() {
    let f = Fixture::new();
    let manifest = format!(
        "capabilities = [\"session.send\"]\n\n{NODE_SERVICE}\n\
         [[channels]]\nid = \"qq-main\"\nplatform = \"qq\"\ntitle = \"QQ 机器人\"\n"
    );
    f.plugin("chat-bot", &manifest, "");
    let friend = json!([{"channel": "qq-main", "id": "friend", "name": "朋友"}]);
    f.configure("chat-bot", json!({"service": true, "allowed": friend}));
    // The first reads its `hello`, the answer to its message and some of the reply's pieces.
    let launcher = Arc::new(Scripted { first: 4, started: Mutex::default() });
    let services = PluginServices::start_with(
        Arc::clone(&f.plugins),
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        &tokio::runtime::Handle::current(),
    );
    let ready = json!({"type": "ready", "protocol": 1,
        "channels": [{"id": "qq-main", "max_text_chars": 10}]});

    eventually("the service started", || launcher.count() == 1).await;
    let first = launcher.nth(0);
    first.say(&ready);
    first.say(&json!({"type": "request", "id": 1, "method": "message.in", "params": {
        "channel": "qq-main", "chat": {"kind": "private", "id": "friend"},
        "user": {"id": "friend", "name": "老王"}, "message_id": "m-1", "text": "你好"}}));
    eventually("a service started after it", || launcher.count() == 2).await;
    let second = launcher.nth(1);
    second.say(&ready);
    eventually("the whole reply", || {
        let replies = second.replies();
        !replies.is_empty() && replies[0]["parts"].as_u64() == u64::try_from(replies.len()).ok()
    })
    .await;

    let (taken, again) = (first.replies(), second.replies());
    assert!(!taken.is_empty() && taken.len() < again.len(), "the first took some: {taken:?}");
    assert_eq!(taken[..], again[..taken.len()], "the same pieces again, from the first one");
    for (at, part) in again.iter().enumerate() {
        assert_eq!([&part["part"], &part["reply_to"]], [&json!(at + 1), &json!("m-1")]);
    }
    let squeezed = |text: &str| text.split_whitespace().collect::<String>();
    let joined: String = again.iter().map(|part| part["text"].as_str().expect("text")).collect();
    assert_eq!(squeezed(&joined), squeezed(NO_SCOPE_REPLY));
    services.stop();
}
