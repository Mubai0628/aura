//! Workspace watcher ownership and bounded catalog reconciliation.
use super::*;

pub(super) struct WorkspaceWatchHandle {
    join: Option<JoinHandle<()>>,
    state: Arc<RwLock<WorkspaceWatcherState>>,
    stop: Arc<AtomicBool>,
}

impl WorkspaceWatchHandle {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn spawn(
        workspace_id: WorkspaceId,
        resolver: Arc<SecurePathResolver>,
        catalog: Arc<PathCatalog>,
        persistence: Arc<PersistenceCoordinator>,
        projection_signals: WorkspaceProjectionSignals,
        coalesce: Duration,
        poll_interval: Duration,
        rescan_attempts: u32,
        batch_hook: Option<WatcherBatchHook>,
    ) -> Result<Self, WorkspaceError> {
        let directories = catalog.watch_directories()?;
        let initial = WorkspaceFileWatcher::start(&directories, poll_interval)
            .map_err(|error| WorkspaceError::Watcher(error.to_string()))?;
        let state = Arc::new(RwLock::new(backend_state(initial.backend())));
        let watcher_path = resolver.canonical_root().to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_state = Arc::clone(&state);
        let join = thread::Builder::new()
            .name(format!("aura-workspace-watch-{workspace_id}"))
            .spawn(move || {
                let mut watcher = initial;
                while !thread_stop.load(Ordering::Acquire) {
                    let first = match watcher.receiver().recv_timeout(coalesce) {
                        Ok(event) => event,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            // The backend is gone, so this channel can never deliver
                            // anything again and a new watcher is the only recovery. The
                            // rebuild that follows re-reads the workspace, so nothing
                            // observed through the dead channel is assumed applied.
                            let Some(replacement) = force_rescan_once(
                                workspace_id,
                                &resolver,
                                &catalog,
                                &persistence,
                                &projection_signals,
                                &thread_state,
                                &thread_stop,
                                poll_interval,
                                rescan_attempts,
                            ) else {
                                break;
                            };
                            watcher = replacement;
                            continue;
                        }
                    };
                    set_watcher_state(&thread_state, WorkspaceWatcherState::Coalescing);
                    let deadline = Instant::now() + coalesce;
                    let mut events = vec![first];
                    while Instant::now() < deadline {
                        match watcher.receiver().try_recv() {
                            Ok(event) => events.push(event),
                            Err(_) => break,
                        }
                    }
                    if events.iter().all(|event| event.kind == FileWatchEventKind::OtherMutation) {
                        // Pure observations report no change, so there is nothing to
                        // re-read and the watcher simply returns to its backend
                        // state. A describable mutation is never dropped here.
                        set_watcher_state(&thread_state, backend_state(watcher.backend()));
                        continue;
                    }
                    let rescan_required =
                        events.iter().any(|event| event.kind == FileWatchEventKind::RescanRequired);
                    let applied = if rescan_required {
                        rebuild_after_undescribed_batch(
                            workspace_id,
                            &resolver,
                            &catalog,
                            &persistence,
                            &thread_state,
                        )
                    } else {
                        reconcile_scoped_batch(
                            &mut watcher,
                            &resolver,
                            &catalog,
                            &projection_signals,
                            &events,
                            batch_hook.as_ref(),
                        )
                    };
                    // Keep the same receiver so events delivered during this batch survive.
                    let synced = if rescan_required {
                        if let Some(hook) = &batch_hook {
                            hook.run();
                        }
                        applied.and_then(|_| {
                            sync_watch_registrations(
                                &mut watcher,
                                &resolver,
                                &catalog,
                                &projection_signals,
                            )
                        })
                    } else {
                        applied
                    };
                    match synced {
                        Ok(()) => {
                            set_watcher_state(&thread_state, backend_state(watcher.backend()));
                        }
                        Err(_) => {
                            // Registration failed, or the delta could not be applied.
                            // A rebuild is the documented recovery for a state this
                            // side can no longer describe; it is not the routine path.
                            let Some(replacement) = force_rescan_once(
                                workspace_id,
                                &resolver,
                                &catalog,
                                &persistence,
                                &projection_signals,
                                &thread_state,
                                &thread_stop,
                                poll_interval,
                                rescan_attempts,
                            ) else {
                                break;
                            };
                            watcher = replacement;
                        }
                    }
                }
            })
            .map_err(|source| WorkspaceError::Io {
                action: "spawn workspace watcher",
                path: watcher_path,
                source,
            })?;
        Ok(Self { join: Some(join), state, stop })
    }

    pub(super) fn state(&self) -> Result<WorkspaceWatcherState, WorkspaceError> {
        self.state
            .read()
            .map(|state| *state)
            .map_err(|_| WorkspaceError::LockPoisoned("workspace watcher state"))
    }
}

/// Applies one batch of watcher events to the index.
fn apply_watch_changes(
    resolver: &SecurePathResolver,
    catalog: &PathCatalog,
    events: &[aura_windows_platform::FileWatchEvent],
) -> Result<(), WorkspaceError> {
    // An external change is bounded to the paths that actually changed, so a write
    // inside one directory no longer rescans the whole workspace. A batch that
    // describes nothing indexable (Aura's own staging paths, an access
    // observation) is not a reason to rebuild either; only a change that cannot be
    // described at all escalates to a full rebuild instead of being silently
    // dropped.
    match crate::catalog::changes_for_watch_events(resolver, catalog, events) {
        crate::catalog::WatchDescription::Undescribed => {
            catalog.rebuild(resolver, CorrelationId::new()).map(|_| ())
        }
        crate::catalog::WatchDescription::Described(changes) if changes.is_empty() => Ok(()),
        crate::catalog::WatchDescription::Described(changes) => catalog
            .apply_changes(
                resolver,
                crate::catalog::CatalogUpdate {
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes,
                    reason: format!("watcher:{}", events.len()),
                    inputs: events.len(),
                },
                &catalog.options(),
                CorrelationId::new(),
            )
            .map(|_| ()),
    }
}

/// Re-reads the whole workspace for a batch the events could not describe.
///
/// A rebuild re-reads every indexable path, so the batch itself does not have to be
/// described; it is reserved for an event that named nothing this side can resolve, and
/// for the recovery paths below.
fn rebuild_after_undescribed_batch(
    workspace_id: WorkspaceId,
    resolver: &SecurePathResolver,
    catalog: &PathCatalog,
    persistence: &PersistenceCoordinator,
    state: &RwLock<WorkspaceWatcherState>,
) -> Result<(), WorkspaceError> {
    set_watcher_state(state, WorkspaceWatcherState::Rescanning);
    let _ = persistence.mark_catalog_stale(workspace_id);
    catalog.rebuild(resolver, CorrelationId::new()).map(|_| ())
}

/// Brings the watcher's registrations in line with the index, and re-reads the
/// directories that were not watched a moment ago.
///
/// A directory becomes watched only after the event that revealed it has been
/// applied, so anything written inside it before its registration produced no event at
/// all. Re-reading exactly those subtrees is what closes that window; the scope is the
/// new directories, never the whole workspace.
///
/// A re-read can itself discover directories — anything created inside the one that was
/// not watched yet — so the two steps repeat until the index stops naming directories
/// that are not registered. Each round is bounded by the new directories it found, and
/// the bound below stops a pathological tree from turning this into an unbounded loop;
/// a round that hits the cap leaves the remaining registrations to the next event,
/// which is a delay rather than a loss.
fn sync_watch_registrations(
    watcher: &mut WorkspaceFileWatcher,
    resolver: &SecurePathResolver,
    catalog: &PathCatalog,
    projection_signals: &WorkspaceProjectionSignals,
) -> Result<(), WorkspaceError> {
    const REGISTRATION_ROUNDS: usize = 8;
    for _ in 0..REGISTRATION_ROUNDS {
        let directories = catalog.watch_directories()?;
        let added = watcher
            .set_directories(&directories)
            .map_err(|error| WorkspaceError::Watcher(error.to_string()))?;
        if added.is_empty() {
            break;
        }
        let mut changes = Vec::new();
        for directory in &added {
            let Ok(relative) = directory.strip_prefix(resolver.canonical_root()) else {
                continue;
            };
            let text = relative.to_string_lossy().replace('\\', "/");
            let Ok(path) = WorkspacePath::parse(if text.is_empty() {
                WorkspacePath::ROOT.to_owned()
            } else {
                text
            }) else {
                continue;
            };
            changes.push((path, crate::catalog::CatalogChange::DirectoryRescanned));
        }
        if changes.is_empty() {
            break;
        }
        catalog.apply_changes(
            resolver,
            crate::catalog::CatalogUpdate {
                writes_missing_text: false,
                repaired_paths: Vec::new(),
                changes,
                reason: "watcher:registration-window".into(),
                inputs: added.len(),
            },
            &catalog.options(),
            CorrelationId::new(),
        )?;
    }
    projection_signals.bump()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn force_rescan_once(
    workspace_id: WorkspaceId,
    resolver: &SecurePathResolver,
    catalog: &PathCatalog,
    persistence: &PersistenceCoordinator,
    projection_signals: &WorkspaceProjectionSignals,
    state: &RwLock<WorkspaceWatcherState>,
    stop: &AtomicBool,
    poll_interval: Duration,
    rescan_attempts: u32,
) -> Option<WorkspaceFileWatcher> {
    for attempt in 0..rescan_attempts.max(1) {
        if stop.load(Ordering::Acquire) {
            return None;
        }
        set_watcher_state(state, WorkspaceWatcherState::RescanRequired);
        let _ = persistence.mark_catalog_stale(workspace_id);
        set_watcher_state(state, WorkspaceWatcherState::Rescanning);

        let replacement = catalog
            .rebuild(resolver, CorrelationId::new())
            .and_then(|_| catalog.watch_directories())
            .and_then(|directories| {
                WorkspaceFileWatcher::start(&directories, poll_interval)
                    .map_err(|error| WorkspaceError::Watcher(error.to_string()))
            });

        if let Ok(watcher) = replacement {
            let _ = projection_signals.bump();
            set_watcher_state(state, backend_state(watcher.backend()));
            return Some(watcher);
        }
        if attempt + 1 < rescan_attempts.max(1) && !wait_for_watcher_retry(stop, poll_interval) {
            return None;
        }
    }
    set_watcher_state(state, WorkspaceWatcherState::Failed);
    None
}

fn wait_for_watcher_retry(stop: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::Acquire) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return true;
        }
        thread::sleep(remaining.min(Duration::from_millis(50)));
    }
    false
}

impl Drop for WorkspaceWatchHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn backend_state(backend: FileWatchBackend) -> WorkspaceWatcherState {
    match backend {
        FileWatchBackend::Native => WorkspaceWatcherState::Watching,
        FileWatchBackend::Polling => WorkspaceWatcherState::PollingFallback,
    }
}

fn set_watcher_state(state: &RwLock<WorkspaceWatcherState>, value: WorkspaceWatcherState) {
    if let Ok(mut state) = state.write() {
        *state = value;
    }
}

/// Reconcile a describable native batch while retaining its receiver and scope.
fn reconcile_scoped_batch(
    watcher: &mut WorkspaceFileWatcher,
    resolver: &SecurePathResolver,
    catalog: &PathCatalog,
    projection_signals: &WorkspaceProjectionSignals,
    events: &[aura_windows_platform::FileWatchEvent],
    hook: Option<&WatcherBatchHook>,
) -> Result<(), WorkspaceError> {
    let applied = apply_watch_changes(resolver, catalog, events);
    if let Some(hook) = hook {
        hook.run();
    }
    applied.and_then(|_| sync_watch_registrations(watcher, resolver, catalog, projection_signals))
}
