//! Measures adding and reopening a real folder as a workspace (evidence only, never run by
//! default). Each step prints one JSON line.
//!
//! - `AURA_PROBE_ROOT`: the folder to add; without it the probe prints a skip line and returns.
//! - `AURA_PROBE_DATA`: the store folder; kept when given, so a store an older build wrote can be
//!   reopened by this one. A temporary folder otherwise.
//! - `AURA_PROBE_STEPS`: `create,reopen` by default; `reopen` reopens an existing store only.
//! - `AURA_PROBE_HOLD_SECONDS`: seconds to stay alive at the end, with the process id printed,
//!   so its memory and handles can be read from another shell.
use std::sync::Arc;
use std::time::Instant;

use aura_domain_common::{CorrelationId, RuntimeInstanceId};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::WorkspaceCreateCommand;

fn persistence(data: &std::path::Path) -> Arc<PersistenceCoordinator> {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        data.join("aura.sqlite3"),
        data.join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => Arc::new(value),
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

/// The index of every workspace the runtime holds: entries and folders it watches.
fn sizes(store: &PersistenceCoordinator, runtime: &WorkspaceRuntime) -> serde_json::Value {
    let projection = store.workspace_session_projection().expect("workspace list");
    let indexes = projection
        .workspaces
        .iter()
        .filter_map(|workspace| runtime.catalog(workspace.workspace_id).ok())
        .map(|catalog| {
            serde_json::json!({
                "entries": catalog.entries().map(|entries| entries.len()).unwrap_or(0),
                "watched_directories":
                    catalog.watch_directories().map(|watched| watched.len()).unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();
    serde_json::Value::Array(indexes)
}

#[test]
#[ignore = "measures a real folder; run with AURA_PROBE_ROOT set"]
fn real_root_probe() {
    let Ok(root) = std::env::var("AURA_PROBE_ROOT") else {
        println!("{}", serde_json::json!({ "step": "skipped", "reason": "AURA_PROBE_ROOT unset" }));
        return;
    };
    let temporary = tempfile::tempdir().expect("temporary store folder");
    let data = std::env::var_os("AURA_PROBE_DATA")
        .map_or_else(|| temporary.path().to_path_buf(), std::path::PathBuf::from);
    std::fs::create_dir_all(&data).expect("store folder");
    let steps = std::env::var("AURA_PROBE_STEPS").unwrap_or_else(|_| "create,reopen".to_owned());
    for step in steps.split(',').map(str::trim) {
        let started = Instant::now();
        // A fresh store owner per step, as a Core start opens it.
        let store = persistence(&data);
        let runtime =
            WorkspaceRuntime::open(Arc::clone(&store), WorkspaceRuntimeOptions::default())
                .expect("open the workspace runtime");
        let opened_ms = started.elapsed().as_millis() as u64;
        let created_ms = match step {
            "create" => {
                let started = Instant::now();
                runtime
                    .create(
                        WorkspaceCreateCommand {
                            selected_root: root.clone(),
                            display_name: "real root probe".to_owned(),
                        },
                        CorrelationId::new(),
                    )
                    .expect("add the folder as a workspace");
                Some(started.elapsed().as_millis() as u64)
            }
            "reopen" => None,
            other => panic!("unknown probe step {other}"),
        };
        let walked = Instant::now();
        runtime.wait_until_indexed(std::time::Duration::from_secs(600)).expect("the walks");
        let walk_ms = walked.elapsed().as_millis() as u64;
        println!(
            "{}",
            serde_json::json!({
                "step": step,
                "open_ms": opened_ms,
                "create_ms": created_ms,
                "walk_ms": walk_ms,
                "open_report": runtime.open_report(),
                "indexes": sizes(&store, &runtime),
            })
        );
        let last = steps.split(',').map(str::trim).next_back().unwrap_or_default();
        if step == last
            && let Some(seconds) = std::env::var("AURA_PROBE_HOLD_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
        {
            println!(
                "{}",
                serde_json::json!({ "step": "hold", "pid": std::process::id(), "seconds": seconds })
            );
            std::thread::sleep(std::time::Duration::from_secs(seconds));
        }
        runtime.stop_watchers().expect("stop the watchers");
    }
}
