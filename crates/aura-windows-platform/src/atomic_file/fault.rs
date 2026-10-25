//! Target-qualified publication diagnostics. Inactive until a local owner registers a hook.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

/// One step of the publication sequence that a test may make fail.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Stage {
    /// Re-querying the target handle after it was moved aside to the backup name.
    QueryMovedBackup,
    /// Putting the moved-aside object back under the destination name.
    RestoreBackup,
    /// Deleting the publication when the sequence is rolled back.
    DeletePublication,
    /// Deleting the superseded backup when the publication commits.
    DeleteBackupOnCommit,
    /// Re-querying the identified handle to verify the actual parent.
    VerifyParentIdentity,
    /// A creation's staging write/verify, before the object takes the destination name.
    CreateAbsentBeforeRename,
    /// A creation's rename itself, which is the authoritative "fail if it exists" action.
    CreateAbsentRename,
}

impl Stage {
    /// The stable name a test names this stage by. Stable so a failure message and a report
    /// line can be matched to the same step, and so a renamed variant cannot silently change
    /// which step a test armed.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::QueryMovedBackup => "query_moved_backup",
            Self::RestoreBackup => "restore_backup",
            Self::DeletePublication => "delete_publication",
            Self::DeleteBackupOnCommit => "delete_backup_on_commit",
            Self::VerifyParentIdentity => "verify_parent_identity",
            Self::CreateAbsentBeforeRename => "create_absent_before_rename",
            Self::CreateAbsentRename => "create_absent_rename",
        }
    }
}

/// The armed stages, keyed by their stable name so arming is by name and not by index.
type FaultRegistry = std::sync::Mutex<BTreeMap<u64, (Stage, PathBuf)>>;

fn armed() -> &'static FaultRegistry {
    static ARMED: std::sync::OnceLock<FaultRegistry> = std::sync::OnceLock::new();
    ARMED.get_or_init(|| std::sync::Mutex::new(BTreeMap::new()))
}

fn next_registration() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

static ANY_ARMED: AtomicBool = AtomicBool::new(false);

/// A diagnostic fault belongs to one named destination and one registration.
/// No thread-local state is used: the real publisher may run on another worker.
pub fn arm(stage: Stage, destination: &Path) -> DisarmGuard {
    let id = next_registration();
    let mut state = armed().lock().unwrap_or_else(|error| error.into_inner());
    state.insert(id, (stage, observation_key(destination)));
    ANY_ARMED.store(true, Ordering::Relaxed);
    DisarmGuard { id }
}

fn is_armed(stage: Stage, destination: &Path) -> bool {
    if !ANY_ARMED.load(Ordering::Relaxed) {
        return false;
    }
    let key = observation_key(destination);
    armed()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .values()
        .any(|(candidate, path)| *candidate == stage && path == &key)
}

pub fn fail_if_armed(stage: Stage, destination: &Path) -> std::io::Result<()> {
    if is_armed(stage, destination) {
        return Err(std::io::Error::other(format!(
            "injected fault at publication stage {}",
            stage.name()
        )));
    }
    Ok(())
}

#[must_use]
pub fn armed_now(stage: Stage, destination: &Path) -> bool {
    is_armed(stage, destination)
}

type InterferenceHook = Box<dyn Fn(&std::path::Path) + Send + Sync>;
type InterferenceRegistry =
    std::sync::Mutex<Vec<(u64, &'static str, std::sync::Arc<InterferenceHook>)>>;

fn interferences() -> &'static InterferenceRegistry {
    static HOOKS: std::sync::OnceLock<InterferenceRegistry> = std::sync::OnceLock::new();
    HOOKS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

static ANY_INTERFERENCE: AtomicBool = AtomicBool::new(false);

/// Runs `action` when the publication reaches `stage`, for a destination under `directory`.
///
/// # What this is for
///
/// Some of batch A's required facts are about what happens when the world changes **during** an
/// operation, not before it: a third party adding a member to a directory this execution created,
/// or replacing this execution's result, while the operation is between its steps. A test cannot
/// arrange that from outside without either racing the operation or re-implementing it.
///
/// So this is a real interference point at a real stage: the action runs inside the production
/// publication, against the real path, and the production code continues afterwards. That is the
/// opposite of fabricating a fact — the operation really does meet the changed world, and the
/// assertions afterwards are about what the production code then did.
///
/// Default-off: with nothing installed the production path pays one relaxed atomic load. An
/// action may not change the publication's return value, because the outcome under test must be
/// the production code's own.
pub fn interfere_at(
    stage: Stage,
    directory: &std::path::Path,
    action: Box<dyn Fn(&std::path::Path) + Send + Sync>,
) -> InterferenceGuard {
    let id = next_registration();
    let key = observation_key(directory);
    let hook: InterferenceHook = Box::new(move |path: &std::path::Path| {
        // The action only runs for a destination inside the directory it was installed for, so
        // one test's interference cannot perturb another test's path.
        if observation_key(path).starts_with(&key) {
            action(path);
        }
    });
    let mut hooks = interferences().lock().unwrap_or_else(|error| error.into_inner());
    hooks.push((id, stage.name(), std::sync::Arc::new(hook)));
    ANY_INTERFERENCE.store(true, Ordering::Relaxed);
    InterferenceGuard { id }
}

/// Runs any interference installed for `stage` at `path`. A no-op while none is installed.
pub(crate) fn reached_interference(stage: Stage, path: &std::path::Path) {
    if !ANY_INTERFERENCE.load(Ordering::Relaxed) {
        return;
    }
    let hooks = interferences()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|(_, armed, _)| *armed == stage.name())
        .map(|(_, _, hook)| std::sync::Arc::clone(hook))
        .collect::<Vec<_>>();
    for hook in hooks {
        hook(path);
    }
}

/// Disarms one interference when it drops.
pub struct InterferenceGuard {
    id: u64,
}

impl Drop for InterferenceGuard {
    fn drop(&mut self) {
        let mut hooks = interferences().lock().unwrap_or_else(|error| error.into_inner());
        hooks.retain(|(id, _, _)| *id != self.id);
        ANY_INTERFERENCE.store(!hooks.is_empty(), Ordering::Relaxed);
    }
}

pub struct DisarmGuard {
    id: u64,
}

impl Drop for DisarmGuard {
    fn drop(&mut self) {
        let mut state = armed().lock().unwrap_or_else(|error| error.into_inner());
        state.remove(&self.id);
        ANY_ARMED.store(!state.is_empty(), Ordering::Relaxed);
    }
}

/// An observation point **inside the real commit**, keyed by destination.
///
/// It fires in `handle_bound_replace_verified` once the destination is open with no sharing and
/// its content condition has been verified — that is, during the real publication, after every
/// caller-held index lock has been released and before the rename. This exists so an
/// acceptance test can prove two saves were inside the commit at the same time, using the real
/// commit rather than a barrier placed around the call.
///
/// The hook is default-off: with no hook installed for a destination, the production path pays
/// one relaxed atomic load and returns. A hook may not fail the publication: it observes.
pub fn observe_commit(path: &std::path::Path, hook: CommitHook) -> CommitHookGuard {
    let id = next_registration();
    let key = observation_key(path);
    let mut hooks = observations().lock().unwrap_or_else(|error| error.into_inner());
    hooks.push((id, key, std::sync::Arc::new(hook)));
    ANY_OBSERVED.store(true, Ordering::Relaxed);
    CommitHookGuard { id }
}

type CommitHook = Box<dyn Fn(&std::path::Path) + Send + Sync>;
type CommitHookRegistry = std::sync::Mutex<Vec<(u64, PathBuf, std::sync::Arc<CommitHook>)>>;

fn observations() -> &'static CommitHookRegistry {
    static HOOKS: std::sync::OnceLock<CommitHookRegistry> = std::sync::OnceLock::new();
    HOOKS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

static ANY_OBSERVED: AtomicBool = AtomicBool::new(false);

/// One installed condition observation: the destination key and the reporting hook.
type ConditionHook = Box<dyn Fn(&str, &str) + Send + Sync>;
type ConditionHookRegistry = std::sync::Mutex<Vec<(u64, PathBuf, std::sync::Arc<ConditionHook>)>>;

fn condition_observations() -> &'static ConditionHookRegistry {
    static HOOKS: std::sync::OnceLock<ConditionHookRegistry> = std::sync::OnceLock::new();
    HOOKS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

static ANY_CONDITION_OBSERVED: AtomicBool = AtomicBool::new(false);

/// Observes which **content condition** a checked replacement really consumed.
///
/// # Why this seam exists
///
/// Whether a caller's original `expected` reaches the exclusive handle cannot be decided by
/// reading the caller: a condition can be computed correctly and then discarded by a later match
/// arm, which is exactly the defect this observes. The only place the answer is provable is here,
/// at the instant the condition is consumed, so this seam reports the algorithm and hex the
/// publication was actually given.
///
/// It is default-off and observation-only: with no hook installed for a destination the
/// production path pays one relaxed atomic load, and a hook can never change the publication's
/// outcome.
/// One installed condition observer: it is handed the algorithm and the hex the publication
/// really consumed.
///
/// It is a named alias rather than an inline `Box<dyn Fn>` so the same shape can be used by
/// every seam that reports a consumed condition, and so a reader can see at the signature that
/// this is an observation, never a decision.
pub type ConditionObserver = Box<dyn Fn(&str, &str) + Send + Sync>;

pub fn observe_replace_condition(
    path: &std::path::Path,
    hook: ConditionObserver,
) -> CommitHookGuard {
    let id = next_registration();
    let key = observation_key(path);
    let mut hooks = condition_observations().lock().unwrap_or_else(|error| error.into_inner());
    hooks.push((id, key, std::sync::Arc::new(hook)));
    ANY_CONDITION_OBSERVED.store(true, Ordering::Relaxed);
    CommitHookGuard { id }
}

/// Reports the content condition a checked replacement consumed for `path`.
///
/// `algorithm` is the stable lowercase name of the digest function and `expected_hex` is the
/// caller's own expected digest. A no-op while nothing is installed.
pub(crate) fn consumed_replace_condition(
    path: &std::path::Path,
    algorithm: &str,
    expected_hex: &str,
) {
    if !ANY_CONDITION_OBSERVED.load(Ordering::Relaxed) {
        return;
    }
    let key = observation_key(path);
    let hooks = condition_observations()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|(_, expected, _)| expected == &key)
        .map(|(_, _, hook)| std::sync::Arc::clone(hook))
        .collect::<Vec<_>>();
    for hook in hooks {
        hook(algorithm, expected_hex);
    }
}

/// The key two spellings of one destination share: the canonical, lowercased path.
fn observation_key(path: &Path) -> PathBuf {
    // Preserve actual OS spelling and UTF-16; never use linguistic case folding.
    // Canonicalize the parent too when the selected destination is still absent.
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        path.parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .and_then(|parent| path.file_name().map(|name| parent.join(name)))
            .unwrap_or_else(|| path.to_path_buf())
    })
}

/// Calls every hook installed for `path`. A no-op while none is installed.
pub(crate) fn reached_commit(path: &std::path::Path) {
    if !ANY_OBSERVED.load(Ordering::Relaxed) {
        return;
    }
    let key = observation_key(path);
    let hooks = observations()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|(_, expected, _)| expected == &key)
        .map(|(_, _, hook)| std::sync::Arc::clone(hook))
        .collect::<Vec<_>>();
    for hook in hooks {
        hook(path);
    }
}

/// Enters the observed commit window for `path`, reporting the exit when the guard drops.
///
/// The window has two ends, and an acceptance test needs both: an entry-only observation can
/// be satisfied by two saves that ran one after the other, which is precisely the
/// serialisation such a test exists to reject. The guard reports the exit however the
/// publication returns, so the overlapping count is a real count of saves that were inside at
/// the same time.
pub(crate) fn enter_commit(path: &std::path::Path) -> Option<CommitWindowGuard> {
    if !ANY_OBSERVED.load(Ordering::Relaxed) {
        return None;
    }
    let key = observation_key(path);
    let exits = exits()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|(_, expected, _)| expected == &key)
        .map(|(_, _, hook)| std::sync::Arc::clone(hook))
        .collect::<Vec<_>>();
    if exits.is_empty() {
        return None;
    }
    Some(CommitWindowGuard { path: path.to_path_buf(), exits })
}

/// Exit hooks, installed alongside the entry ones.
fn exits() -> &'static CommitHookRegistry {
    static EXITS: std::sync::OnceLock<CommitHookRegistry> = std::sync::OnceLock::new();
    EXITS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

type CommitExitHook = Box<dyn Fn(&std::path::Path) + Send + Sync>;

/// The reporting half of an installed observation.
pub struct CommitWindowGuard {
    path: std::path::PathBuf,
    exits: Vec<std::sync::Arc<CommitExitHook>>,
}

impl Drop for CommitWindowGuard {
    fn drop(&mut self) {
        for exit in &self.exits {
            exit(&self.path);
        }
    }
}

/// Installs an observation for `path` that reports both ends of the commit window.
///
/// `on_enter` fires once the destination is open with no sharing and its content condition has
/// been verified; `on_exit` fires when that save leaves the window, however it leaves. A test
/// uses the pair to count how many saves were inside at the same time.
pub fn observe_commit_window(
    path: &std::path::Path,
    on_enter: CommitHook,
    on_exit: CommitHook,
) -> CommitHookGuard {
    let id = next_registration();
    let key = observation_key(path);
    // Fixed lock order and flag publication under the registration lock.
    let mut enters = observations().lock().unwrap_or_else(|error| error.into_inner());
    let mut exits = exits().lock().unwrap_or_else(|error| error.into_inner());
    enters.push((id, key.clone(), std::sync::Arc::new(on_enter)));
    exits.push((id, key, std::sync::Arc::new(on_exit)));
    ANY_OBSERVED.store(true, Ordering::Relaxed);
    CommitHookGuard { id }
}

/// Removes one installed observation when it drops.
pub struct CommitHookGuard {
    id: u64,
}

impl Drop for CommitHookGuard {
    fn drop(&mut self) {
        let mut hooks = observations().lock().unwrap_or_else(|error| error.into_inner());
        hooks.retain(|(id, _, _)| *id != self.id);
        exits()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|(id, _, _)| *id != self.id);
        ANY_OBSERVED.store(!hooks.is_empty(), Ordering::Relaxed);
        drop(hooks);
        let mut conditions =
            condition_observations().lock().unwrap_or_else(|error| error.into_inner());
        conditions.retain(|(id, _, _)| *id != self.id);
        ANY_CONDITION_OBSERVED.store(!conditions.is_empty(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
