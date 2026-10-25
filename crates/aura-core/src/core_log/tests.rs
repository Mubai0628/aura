//! A test's route for Core diagnostics stays its own until the route ends, and the backend's
//! installation records follow it.
use super::*;
use aura_backend_windows_user::installation::InstallationBinding;
use std::sync::mpsc;
use std::time::Duration;

const PROBE: &str = "core_log.route_probe";

/// The records of one event in a routed file. Other tests may still write their own there.
fn records(path: &Path, event: &str) -> Vec<Value> {
    let text = fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["event"] == event)
        .collect()
}

fn probes(path: &Path) -> Vec<String> {
    records(path, PROBE)
        .iter()
        .filter_map(|entry| entry["fields"]["route"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn a_second_route_waits_until_the_first_one_ends() {
    let root = tempfile::tempdir().expect("route probe files");
    let first_path = root.path().join("first.jsonl");
    let second_path = root.path().join("second.jsonl");
    let first = CoreLogRoute::to(first_path.clone());
    let (routed, second_routed) = mpsc::channel();
    let second = std::thread::spawn({
        let second_path = second_path.clone();
        move || {
            let route = CoreLogRoute::to(second_path);
            append_core_log(PROBE, json!({ "route": "second" }));
            let _ = routed.send(());
            drop(route);
        }
    });

    assert!(
        second_routed.recv_timeout(Duration::from_millis(500)).is_err(),
        "a second test cannot re-route the log while the first one's route is in force"
    );
    append_core_log(PROBE, json!({ "route": "first" }));
    drop(first);
    second.join().expect("the second test routes the log once the first route ends");

    assert_eq!(probes(&first_path), ["first"]);
    assert_eq!(probes(&second_path), ["second"]);
}

#[test]
fn installation_records_go_to_the_route_in_force() {
    let root = tempfile::tempdir().expect("route and installation folders");
    let path = root.path().join("core.jsonl");
    let empty = root.path().join("installation");
    fs::create_dir(&empty).expect("an empty installation folder");
    let route = CoreLogRoute::to(path.clone());
    let binding = InstallationBinding::new(empty);
    assert!(binding.advertised().is_err(), "an empty folder is no installation");
    drop(route);

    let opens = records(&path, "tool.installation_open");
    assert_eq!(opens.len(), 1, "{opens:?}");
    assert_eq!(opens[0]["component"], "windows-user");
    assert_eq!(opens[0]["fields"]["ok"], false);
    assert_eq!(opens[0]["fields"]["shared_failure"], false);
}
