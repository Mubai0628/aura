//! A real service: Node.js running a short script, started as a program session in a Job of
//! its own, given its settings and secrets, its standard error kept without the secret, and
//! gone once it was asked to end.
mod support;
use aura_plugin_host::{
    Clock, Launcher, PluginHost, ProgramLauncher, ServiceCalls, ServiceLaunch, SystemClock,
};
use aura_plugins::PluginServiceStatus;
use std::path::PathBuf;
use std::sync::Arc;
use support::*;

const SERVICE: &str = r#"
import { createInterface } from 'node:readline';
import { writeFileSync } from 'node:fs';
const say = (message) => process.stdout.write(JSON.stringify(message) + '\n');
writeFileSync('started.txt', `${process.env.AURA_PLUGIN_DATA}|${process.env.PYTHONDONTWRITEBYTECODE}`);
process.stderr.write(`started with ${process.env.AURA_SECRET_TOKEN}\n`);
createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.type === 'hello') {
    say({ type: 'ready', protocol: 1, channels: [{ id: 'main', max_text_chars: 500 }] });
    const text = `${message.settings.app_id} ${process.env.AURA_PLUGIN_NAME}`;
    say({ type: 'request', id: 1, method: 'notify', params: { text } });
  } else if (message.type === 'response') {
    say({ type: 'log', level: 'info', text: `answered ${JSON.stringify(message.result.shown)}` });
  } else if (message.type === 'shutdown') {
    process.exit(0);
  }
});
"#;

fn node() -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH");
    std::env::split_paths(&path)
        .map(|folder| folder.join("node.exe"))
        .find(|candidate| candidate.is_file())
        .expect("these tests run Node.js from PATH, as a plugin's service would")
}

#[test]
fn a_node_service_is_started_answered_and_ended() {
    let temp = tempfile::tempdir().expect("temp");
    let script = temp.path().join("service.mjs");
    std::fs::write(&script, SERVICE).expect("script");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let calls = Arc::new(Recorded::default());
    let host = PluginHost::new(
        Arc::new(ProgramLauncher) as Arc<dyn Launcher>,
        Arc::clone(&calls) as Arc<dyn ServiceCalls>,
        Arc::new(SystemClock) as Arc<dyn Clock>,
        runtime.handle().clone(),
    );
    let data = temp.path().join("data");
    let launch = ServiceLaunch {
        command: node().display().to_string(),
        arguments: vec![script.display().to_string()],
        data_folder: data.clone(),
        scratch: temp.path().join("run"),
        limits: Some(aura_windows_platform::JobLimits {
            memory_bytes: 512 * 1024 * 1024,
            active_processes: 8,
        }),
        ..launch("a")
    };
    host.reconcile(vec![launch]);
    eventually("running", || matches!(calls.status(), Some(PluginServiceStatus::Running { .. })));
    eventually("its request", || !calls.calls.lock().expect("calls").is_empty());
    let (plugin, method, params) = calls.calls.lock().expect("calls")[0].clone();
    assert_eq!((plugin.as_str(), method.as_str()), ("chat-bot", "notify"));
    assert_eq!(params["text"], "1024 chat-bot", "its text settings and its name");
    eventually("its log", || calls.logs().iter().any(|(_, text)| text == "answered true"));
    eventually("its standard error", || {
        calls.logs().contains(&("stderr".into(), "started with ••••".into()))
    });
    assert!(!calls.logs().iter().any(|(_, text)| text.contains(SECRET)));
    assert_eq!(
        std::fs::read_to_string(data.join("started.txt")).expect("it runs in its own folder"),
        format!("{}|1", data.display()),
        "its data folder, and Python told not to write next to the plugin's files"
    );
    host.reconcile(Vec::new());
    eventually("it ended by itself once asked", || calls.ended() == 1);
    assert!(host.idle());
}
