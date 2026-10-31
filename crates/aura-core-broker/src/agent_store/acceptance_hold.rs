//! Default-off, acceptance-only external control of the Agent hold points.
//!
//! WHY THIS EXISTS
//! S04-E has to produce a *controlled* fault on one real Agent run while another session keeps
//! executing, on a bound Core image, driven from outside the process. A `sleep`, a large workspace or
//! a cold cache produces a wait, not a controlled fault: the reviewer has to be able to say which
//! real step was parked, on which run, and that everything after it still ran. The Agent run path
//! already owns exactly the right hold points - one bounded section per Store step - but until now
//! they could only be armed *in process* (`AgentStore::arm_hold`), so a driver that only has the
//! production IPC boundary could not reach one at all.
//!
//! WHAT THIS IS
//! An adapter, not a second mechanism. It binds the same [`Barrier`] the regressions use to a control
//! document named by the environment, installs it into the one Store owner's own hold slot, and
//! exposes one arrival on the Agent tool-dispatch path. Nothing here decides anything about a run:
//! it parks an arriving thread and lets it continue. It holds no response, answers nothing, and
//! cannot fabricate, replace, resolve, reject, reorder or replay anything.
//!
//! DEFAULT-OFF, IN TWO SENSES THAT MATTER
//! * The whole module is `#[cfg(feature = "acceptance-barrier")]`. `cargo build -p aura-core` - the
//!   command the execution image build and `pnpm dev` both run - compiles none of it, so the
//!   default image has no reader for [`CONTROL_DOCUMENT_ENV`] and no arrival on the tool path.
//!   Scanning the default-built image for this module's marker string and its environment variable
//!   name shows that.
//! * Even in a feature-enabled image nothing is held unless the environment names a control document
//!   AND that document currently arms this point. With no variable, `install_from_environment`
//!   returns immediately and not one store step changes behaviour.
//!
//! WHAT IT MAY HOLD, AND WHAT THAT COSTS
//! * `writer_execution` / `agent_store`: one Agent Store step, selected by its own identity
//!   (`run=<agent_run_id>/<operation>`). This is the point every read and every transaction of the
//!   run path already passes through.
//! * `agent_tool_reservation`: the Agent tool dispatch, after the call's reservation has committed
//!   and before any backend was dispatched, selected by `run=<agent_run_id>/tool=<tool_call_id>`.
//!   It is reached from `agent_execution::reservation` and it parks the dispatching thread, which is
//!   the real work the orchestrator itself scheduled.
//! * Both are bounded twice: the driver releases them, and the barrier's own monotonic fallback
//!   ([`aura_persistence::barrier::DEFAULT_FALLBACK_HOLD`]) ends them anyway, so a driver that
//!   disappears is a slow case and never a hung Core.
//!
//! THE CONTROL PROTOCOL (one directory, plain JSON, written atomically by the driver)
//! * `agent-hold-capability.json` - written by the Core at startup, only in a feature-enabled image
//!   that was given a control document. Its absence is what makes the acceptance driver report the
//!   Agent hold as unavailable instead of waiting for a hold that cannot happen.
//! * `agent-hold.json`          - written by the driver: `{point,label,select,once,step_millis}`.
//! * `agent-hold-release.json`  - written by the driver: `{release:true,at_ms}`.
//! * `agent-hold-journal.jsonl` - appended by the Core: `control`, `arrived` and `released`, with the
//!   arriving case's own identity, so "the hold was reached" is a record and not a timing guess.
use super::{AgentStore, HOLD_LABEL};
use aura_persistence::barrier::{Barrier, DEFAULT_FALLBACK_HOLD, Point};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

/// The environment variable that names the control document. Unset in every product run.
pub const CONTROL_DOCUMENT_ENV: &str = "AURA_S04_AGENT_HOLD";

/// The marker string `verify-barrier-default-off.mjs` looks for in a default-built image.
pub const BUILD_MARKER: &str = "aura.s04.agent-hold.v1";

/// The label the Agent Store hold points carry. The same one the regressions arm.
pub const STORE_LABEL: &str = HOLD_LABEL;

/// The label of the tool-dispatch hold point.
pub const TOOL_LABEL: &str = "agent_tool_reservation";

/// The label of the post-execution Tool hold point (D7).
pub const POST_EXECUTION_LABEL: &str = "agent_tool_post_execution";

const CAPABILITY_FILE: &str = "agent-hold-capability.json";
const DOCUMENT_FILE: &str = "agent-hold.json";
const JOURNAL_FILE: &str = "agent-hold-journal.jsonl";
const RELEASE_FILE: &str = "agent-hold-release.json";
/// The observer's own journal, beside the hold journal.
const EXECUTION_JOURNAL_FILE: &str = "agent-execution-journal.jsonl";

/// The process-wide binding. `None` inside the `Some` means the variable named no document.
static BOUND: OnceLock<Option<Barrier>> = OnceLock::new();

/// The directory the control document named, so the observer writes beside it.
///
/// It is a latest-wins slot rather than a `OnceLock` because a process may construct more than one
/// Core (a regression does), and the observer must follow the Core that is really running.
static CONTROL_DIRECTORY: OnceLock<std::sync::Mutex<Option<PathBuf>>> = OnceLock::new();

fn control_directory_slot() -> &'static std::sync::Mutex<Option<PathBuf>> {
    CONTROL_DIRECTORY.get_or_init(|| std::sync::Mutex::new(None))
}

fn bound_control_directory() -> Option<PathBuf> {
    control_directory_slot().lock().ok().and_then(|slot| slot.clone())
}

/// Regression-only: bind the observer to a directory without a control document.
///
/// The acceptance path binds it from the environment before the Core starts; a regression that
/// only measures the observer's counting does not need a hold point, and must not have to mutate
/// this process's environment to get one.
#[cfg(any(test, feature = "test-hooks"))]
pub fn bind_control_directory_for_test(directory: &Path) {
    if let Ok(mut slot) = control_directory_slot().lock() {
        *slot = Some(directory.to_path_buf());
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

/// The control document this process was given, or `None` when it was given none.
fn document_from_environment() -> Option<PathBuf> {
    let value = std::env::var_os(CONTROL_DOCUMENT_ENV)?;
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

/// Bind this process to the control document named by [`CONTROL_DOCUMENT_ENV`].
///
/// Returns whether a document was named. Called once per Core, from the one place the Agent Store
/// owner is constructed; a second call is idempotent and installs nothing new.
pub fn install_from_environment(store: &AgentStore) -> bool {
    let Some(document) = document_from_environment() else {
        let _ = BOUND.set(None);
        return false;
    };
    let directory = document.parent().map(Path::to_path_buf).unwrap_or_default();
    if let Ok(mut slot) = control_directory_slot().lock() {
        *slot = Some(directory.clone());
    }
    let barrier = Barrier::environment_bound(
        document.clone(),
        Some(directory.join(RELEASE_FILE)),
        Some(directory.join(JOURNAL_FILE)),
    );
    // The one Store owner reads its own slot on every step: installing it there is what makes an
    // already-bounded section externally holdable. Nothing else about the owner changes.
    if let Ok(mut hold) = store.hold.lock() {
        *hold = Some(barrier.clone());
    }
    announce_capability(&document, &directory);
    let _ = BOUND.set(Some(barrier));
    true
}

/// The Core's own statement that THIS image really carries the Agent hold points.
///
/// A driver that only has the file protocol cannot otherwise tell "this image has no such hold
/// point" from "the hold was never reached". Written once, next to the control document.
fn announce_capability(document: &Path, directory: &Path) {
    let document_text = document.display().to_string();
    let capability = serde_json::json!({
        "schema": "aura.s04.agent-hold.capability.v1",
        "protocol": BUILD_MARKER,
        "feature": "acceptance-barrier",
        "compiled_under": "cfg(feature = \"acceptance-barrier\"): the default image contains none of \
                           this module",
        "pid": std::process::id(),
        "started_at_ms": now_ms(),
        "control_document": document_text,
        // The name the acceptance driver uses by convention for the document it writes; the Core
        // follows whatever path the environment named, so this is stated rather than assumed.
        "document_name_convention": DOCUMENT_FILE,
        "release_document": directory.join(RELEASE_FILE).display().to_string(),
        "journal": directory.join(JOURNAL_FILE).display().to_string(),
        "fallback_hold_ms": u64::try_from(DEFAULT_FALLBACK_HOLD.as_millis()).unwrap_or(u64::MAX),
        "fallback_clock": "instant_monotonic",
        "points": [
            { "point": "writer_execution", "label": STORE_LABEL,
              "select": "run=<agent_run_id>/<operation>" },
            { "point": "agent_tool_reservation", "label": TOOL_LABEL,
              "select": "run=<agent_run_id>/tool=<tool_call_id>" },
            { "point": "agent_tool_post_execution", "label": POST_EXECUTION_LABEL,
              "select": "run=<agent_run_id>/tool=<tool_call_id>/attempt=<ordinal>",
              "meaning": "the backend really ran and only the terminal receipt is still missing" },
        ],
    });
    let path = directory.join(CAPABILITY_FILE);
    if let Err(error) = std::fs::write(&path, format!("{capability}\n")) {
        eprintln!(
            "the agent hold capability could not be announced at {}: {error}",
            path.display()
        );
    }
}

/// The bound barrier, when this process was given a control document.
fn bound() -> Option<&'static Barrier> {
    BOUND.get().and_then(Option::as_ref)
}

/// Park the arriving tool dispatch when a control document armed this exact case.
///
/// Called after the reservation committed and before any backend was dispatched. It can only delay
/// that dispatch: authorization, the claim, the ledger reservation and the router decision have all
/// already run, and the backend that runs afterwards is the production one.
pub fn hold_tool_reservation(run: &str, tool_call_id: &str) {
    let Some(barrier) = bound() else {
        return;
    };
    let identity = format!("run={run}/tool={tool_call_id}");
    // The permit is dropped here on purpose: `Drop` is what publishes the hold's real duration, and
    // the park itself already ended inside `arrive_identified`.
    let _ = barrier.arrive_identified(Point::AgentToolReservation, TOOL_LABEL, Some(&identity));
}

/// Park the arriving Tool backend **after its real side effect and before the terminal receipt**.
///
/// This is D7's hold point. It is reached from `agent_execution::broker::Port::invoke` after the
/// domain operation really ran and after its post-state evidence was read, and before the ledger is
/// completed - so a hold that outlives the production budget leaves exactly the state D7 is about:
/// a real side effect, a real operation/attempt/runtime binding, and no terminal receipt.
///
/// It delays the return and nothing else. It cannot manufacture an outcome, cannot skip the ledger,
/// the router or the authorisation (all of which have already run), and cannot re-order or replay
/// anything.
pub fn hold_tool_post_execution(run: &str, tool_call_id: &str, attempt_ordinal: u64) {
    let Some(barrier) = bound() else {
        return;
    };
    let identity = format!("run={run}/tool={tool_call_id}/attempt={attempt_ordinal}");
    let _ = barrier.arrive_identified(
        Point::AgentToolPostExecution,
        POST_EXECUTION_LABEL,
        Some(&identity),
    );
}

/// Record one really-entered Tool backend invocation and the publication stages it really reached.
///
/// WHY THIS EXISTS
/// A "the file exists and its bytes match the expected ones" fact is a derived **file state**, not
/// an observed write: a second write of the same content leaves exactly the same state behind. The
/// no-replay gate needs a count that is bound to the original identity and that a second identical
/// publication would change. This observer is that count.
///
/// WHAT IT IS
/// * One line per real backend entry, appended to `agent-execution-journal.jsonl` beside the
///   control document, carrying the run, the tool call, the attempt ordinal, the action kind,
///   whether the backend was really entered, the post-state the production evidence step really
///   read from the target, and the workspace mutation stages production recorded around the
///   publication (`completed` is a publication that really landed, `compensated` one that was
///   rolled back).
/// * An **observer**: it appends a line when a control document named a directory, and does
///   nothing at all otherwise. It changes no return value, no result, no ledger row and no
///   ordering; a journal that cannot be written loses the record, never the execution.
/// * Default-off twice over, exactly like the hold points: the module is compiled only under
///   `acceptance-barrier`, and even then nothing is written unless this process was given a
///   control document.
pub struct BackendExecutionIdentity<'a> {
    pub run: &'a str,
    pub tool_call_id: &'a str,
    pub attempt_ordinal: u64,
    pub action_kind: &'a str,
}

pub fn observe_backend_invocation(
    identity: BackendExecutionIdentity<'_>,
    backend_entered: bool,
    succeeded: bool,
    post_state: &serde_json::Value,
    mutation_stages: &[&str],
) {
    let Some(directory) = bound_control_directory() else {
        return;
    };
    let entry = serde_json::json!({
        "schema": "aura.s04.execution-observer.v1",
        "at_ms": now_ms(),
        "pid": std::process::id(),
        "run": identity.run,
        "tool_call_id": identity.tool_call_id,
        "attempt_ordinal": identity.attempt_ordinal,
        "action_kind": identity.action_kind,
        "backend_entered": backend_entered,
        "succeeded": succeeded,
        // The post-state the production evidence step read from the target, recorded verbatim so
        // the reader sees the same fact the ledger used and not a re-derivation of it.
        "post_state": post_state,
        "mutation_stages": mutation_stages,
        // A publication that really landed, as production recorded it. This is a count of
        // publications, not of file states: two identical writes are two entries.
        "published": mutation_stages.contains(&"completed"),
    });
    let path = directory.join(EXECUTION_JOURNAL_FILE);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path)
        && let Err(error) = std::io::Write::write_all(&mut file, format!("{entry}\n").as_bytes())
    {
        eprintln!("the S04 execution observer could not append to {}: {error}", path.display());
    }
}
