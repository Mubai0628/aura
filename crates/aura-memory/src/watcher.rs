use crate::{MemoryError, MemoryResult, Vault};
use aura_domain_common::WorkspacePath;
use notify::{RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    sync::{Arc, Weak, mpsc},
    thread,
    time::Duration,
};

pub struct VaultWatcher {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}
impl VaultWatcher {
    pub fn start(vault: &Arc<Vault>) -> MemoryResult<Self> {
        let (events_tx, events) = mpsc::channel();
        let (stop, stopped) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = events_tx.send(event);
        })
        .map_err(|e| MemoryError::Watcher(e.to_string()))?;
        // Watch the nearest existing ancestor while the Vault is absent; never
        // create it to make monitoring possible. Only events below this root count.
        let mut watched = vault.root();
        while !watched.try_exists()? {
            watched = watched.parent().ok_or(MemoryError::Invalid("watch ancestor"))?;
        }
        let mut watched = watched.to_path_buf();
        let mut recursive = watched == vault.root();
        watcher
            .watch(
                &watched,
                if recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive },
            )
            .map_err(|e| MemoryError::Watcher(e.to_string()))?;
        let weak = Arc::downgrade(vault);
        let worker = thread::Builder::new().name("aura-memory-watch".into()).spawn(move || {
            loop {
                if stopped.try_recv().is_ok() || weak.strong_count() == 0 {
                    break;
                }
                if let Some(vault) = weak.upgrade() {
                    let mut nearest = vault.root();
                    while !nearest.try_exists().unwrap_or(false) {
                        let Some(parent) = nearest.parent() else {
                            break;
                        };
                        nearest = parent;
                    }
                    let next_recursive = nearest == vault.root();
                    if watched != nearest || recursive != next_recursive {
                        let _ = watcher.unwatch(&watched);
                        if watcher
                            .watch(
                                nearest,
                                if next_recursive {
                                    RecursiveMode::Recursive
                                } else {
                                    RecursiveMode::NonRecursive
                                },
                            )
                            .is_err()
                        {
                            let _ = vault
                                .lock()
                                .index
                                .set_state("watcher_error:监视失效；请重建索引", None);
                        }
                        watched = nearest.to_path_buf();
                        recursive = next_recursive;
                        let _ = vault.rebuild();
                    }
                }
                match events.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => {
                        let mut batch = vec![event];
                        while let Ok(event) = events.recv_timeout(Duration::from_millis(75)) {
                            batch.push(event);
                            if batch.len() >= 1024 {
                                break;
                            }
                        }
                        update(&weak, batch);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
            }
        })?;
        Ok(Self { stop, worker: Some(worker) })
    }
}
impl Drop for VaultWatcher {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn update(weak: &Weak<Vault>, events: Vec<notify::Result<notify::Event>>) {
    let Some(vault) = weak.upgrade() else {
        return;
    };
    let mut paths = BTreeSet::new();
    let mut rescan = false;
    let mut failed = false;
    for event in events {
        let event = match event {
            Ok(e) => e,
            Err(_) => {
                failed = true;
                continue;
            }
        };
        if matches!(event.kind, notify::EventKind::Access(_)) {
            continue;
        }
        for path in event.paths {
            let Ok(relative) = path.strip_prefix(vault.root()) else {
                continue;
            };
            if relative.as_os_str().is_empty() || path.is_dir() {
                rescan = true;
                continue;
            }
            let Ok(relative) = WorkspacePath::parse(relative.to_string_lossy()) else {
                continue;
            };
            if relative.as_str().to_ascii_lowercase().ends_with(".md")
                && !relative.as_str().eq_ignore_ascii_case("README.md")
            {
                paths.insert(relative);
            }
        }
    }
    if failed {
        let _ = vault.lock().index.set_state("watcher_error:监视失效；请重建索引", None);
        return;
    }
    let result = if rescan {
        vault.rebuild()
    } else {
        vault.refresh_paths(&paths.into_iter().collect::<Vec<_>>())
    };
    if result.is_err() {
        let _ = vault.lock().index.set_state("watcher_error:监视更新失败；请重建索引", None);
    }
}
