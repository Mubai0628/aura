//! Default-off controlled boundary for the S02-E isolation acceptance (R3).
//!
//! WHY THIS EXISTS
//! E1/E4 need one designated request to be *really* still being processed while a shorter caller
//! budget expires, and E2 needs one designated write to *really* commit while its reply is held. No
//! fixture size, sleep or cold-cache luck produces those events: a sleep shows that the script
//! waited, and an enlarged tree showed nothing at all (the real search answered `resolved` inside the
//! budget). A production delay/debug RPC is explicitly not allowed either. So the hold is an injected
//! adapter, compiled only for acceptance runs.
//!
//! DEFAULT-OFF, IN THE STRONGEST AVAILABLE SENSE
//! * The whole module is `#[cfg(feature = "acceptance-barrier")]`. `cargo build -p aura-core` - the
//!   command the execution image build and `pnpm dev` both run - compiles none of it, so the
//!   production image has no boundary code, no `AURA_S02_BARRIER_CONTROL` reader and no control file.
//!   Scanning the default-built image for this module's marker string shows that.
//! * A feature-enabled build still does nothing unless `AURA_S02_BARRIER_CONTROL` names a control
//!   directory, and the two hooks are reached only by a request whose method AND params match an
//!   installed hold. With no install file, every request takes the same path it always did.
//!
//! WHAT IT MAY DO
//! * Delay, and nothing else. The hook at the processing boundary sits after the broker's
//!   authorization, claim, ledger reservation and router decision, and before the real handler: the
//!   handler still runs and still produces the real result. The hook at the reply boundary sits after
//!   the handler returned and after the command outcome was recorded, and before the frame is
//!   written: the effect has really happened once, and only the reply is late.
//! * It cannot fabricate, replace, resolve, reject, reorder or replay anything: no function here
//!   holds a response or returns one, and nothing answers a request. Authentication, the request
//!   ledger, the command claim, the router and the reply path have all already run.
//! * A hold is bounded twice: the driver releases it, and `MAX_HOLD_MS` releases it anyway. The
//!   fallback budget is measured on `Instant` (a monotonic clock), so a wall-clock step can neither
//!   end a hold early nor park a request forever. Wall-clock time is written into the control
//!   documents for display only.
//! * The claim on "one designated request at a time" is owned by [`HoldOwner`] and released by
//!   `Drop`, so a request future that is cancelled - a severed connection, an aborted task - cannot
//!   permanently occupy the slot the next scenario needs.
//!
//! THE OBSERVATION RECORD (what makes a counted effect a measured one)
//! * `state.json` is the boundary's own account of the request it really held.
//! * `effects.jsonl` is append-only and is written for the DESIGNATED request only, after the real
//!   handler and the real broker transaction returned (`broker::files::dispatch_files` calls
//!   [`record_effect`] once its `broker::run` completed). It carries the request id, the
//!   `CommandInvocationId` the Main really sent, whether the transaction committed and the real
//!   `IpcRequestTerminal` of that same dispatch (`succeeded` when it returned `Ok`, otherwise the
//!   terminal of the error envelope it really returned), so an acceptance scenario can count real
//!   effects for one invocation instead of inferring a count from a file digest, and can read what the
//!   Core's own dispatch ended with instead of deriving it from `committed` alone. It is never written
//!   for a request no install designated.
//! * `error.json` is written when a control document could not be read or appended. A read error is
//!   reported as an error, never silently as "no hold was installed", and a write error is never
//!   hidden: the driver sees the reason and the scenario records it.
//!
//! THE CONTROL PROTOCOL (one directory, plain JSON files, written atomically)
//! * `capability.json` - written by the Core at startup, only in a feature-enabled build that was
//!   given the directory. Its absence is what makes the acceptance driver report the boundary as
//!   unavailable instead of waiting for a hold that cannot happen.
//! * `install.json`  - written by the acceptance driver: `{marker, boundary, method}`.
//! * `state.json`    - written by the Core: which request was held, when, and how it ended.
//! * `release.json`  - written by the acceptance driver: `{marker, release: true}`.
//! * `effects.jsonl` - appended by the Core, designated requests only, after the real transaction.
//! * `error.json`    - written by the Core when a control document really could not be read/written.
//!
//! A request matches an install when its method equals the installed method AND the marker appears
//! literally in its serialized params. The acceptance driver always puts the marker in a field the
//! request really carries (search pattern text, written file content), so the match is a property of
//! the request, never of a timing guess.
use std::{
    env,
    fs::OpenOptions,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use aura_domain_common::CorrelationId;
use aura_runtime_contract::IpcRequestTerminal;
use serde::Deserialize;
use serde_json::{Value, json};

/// The environment variable that names the control directory. Unset in every product run.
pub const CONTROL_DIRECTORY_ENV: &str = "AURA_S02_BARRIER_CONTROL";

/// The marker string `verify-barrier-default-off.mjs` looks for in a default-built image.
pub const BUILD_MARKER: &str = "aura.acceptance-barrier.v1";

const CAPABILITY_FILE: &str = "capability.json";
const EFFECTS_FILE: &str = "effects.jsonl";
const ERROR_FILE: &str = "error.json";
const INSTALL_FILE: &str = "install.json";
const RELEASE_FILE: &str = "release.json";
const STATE_FILE: &str = "state.json";

/// How often the held request asks the driver whether it may continue.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The bounded fallback release. A hold whose driver has gone away still ends here.
pub const MAX_HOLD_MS: u64 = 60_000;

/// Which real boundary a hold sits at.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Boundary {
    /// Before the real handler runs: the request has not produced a result yet.
    Processing,
    /// After the real handler returned and its outcome was recorded, before the reply is written.
    Reply,
}

impl Boundary {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Processing => "processing",
            Self::Reply => "reply",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Install {
    boundary: Boundary,
    marker: String,
    method: String,
}

/// What `install.json` really says about one request. An unreadable install is its own answer.
enum InstallMatch {
    /// The file is absent or names a different boundary/method/marker: not the designated request.
    NoMatch,
    /// This request is the one the install designates, at any boundary.
    Match(Install),
    /// The file exists but could not be read as an install. Never reported as "no hold".
    Invalid(String),
}

/// The identity of the one request an install designated.
#[derive(Clone, Debug)]
struct Designation {
    command_invocation_id: Option<String>,
    marker: String,
    request_id: CorrelationId,
}

/// The request currently parked at a boundary. Cleared by [`HoldOwner::drop`], never only by the
/// normal end of `hold`, so a cancelled hold future cannot keep the slot.
static HELD: Mutex<Option<Designation>> = Mutex::new(None);

/// The request the current install designated. It survives the end of the hold, because the real
/// effect of that request is recorded after the hold ended: the handler and the broker transaction
/// run after a processing hold is released.
static DESIGNATED: Mutex<Option<Designation>> = Mutex::new(None);

/// Serialises effect-journal appends so one line is never interleaved with another.
static JOURNAL: Mutex<()> = Mutex::new(());

/// Ownership of the single hold slot. `Drop` is the only release path that cannot be skipped.
struct HoldOwner {
    request_id: CorrelationId,
}

impl Drop for HoldOwner {
    fn drop(&mut self) {
        let Ok(mut slot) = HELD.lock() else {
            return;
        };
        if slot.as_ref().is_some_and(|held| held.request_id == self.request_id) {
            *slot = None;
        }
    }
}

/// The control directory, resolved once. `None` means this run has no injected boundary at all.
fn control_directory() -> Option<&'static Path> {
    static RESOLVED: OnceLock<Option<PathBuf>> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let value = env::var_os(CONTROL_DIRECTORY_ENV)?;
            if value.is_empty() {
                return None;
            }
            Some(PathBuf::from(value))
        })
        .as_deref()
}

/// Wall-clock milliseconds. Used for the timestamps the driver displays, never for a budget.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

fn write_json(path: &Path, document: &Value) -> std::io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, format!("{document}\n"))?;
    // A reader either sees the previous document or this one, never half of either. On Windows
    // `rename` replaces an existing destination, which is what makes the state file readable while
    // the hold is still running.
    std::fs::rename(&temporary, path)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> std::io::Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Report a real control-document failure instead of letting it read as "nothing happened".
fn write_error(reason: &str, marker: Option<&str>) {
    let Some(directory) = control_directory() else {
        return;
    };
    let document = json!({
        "schema": "aura.acceptance-barrier.error.v1",
        "protocol": BUILD_MARKER,
        "marker": marker,
        "reason": reason,
        "at_ms": now_ms(),
        "pid": std::process::id(),
    });
    if let Err(error) = write_json(&directory.join(ERROR_FILE), &document) {
        eprintln!("acceptance barrier error was not written: {error}");
    }
}

/// Append one identity-only line to the effect journal. Returns the reason it could not be written.
fn append_effect(document: &Value) -> Option<String> {
    let directory = control_directory()?;
    let Ok(_guard) = JOURNAL.lock() else {
        return Some("the barrier effect journal lock was poisoned".to_owned());
    };
    let path = directory.join(EFFECTS_FILE);
    let opened = OpenOptions::new().create(true).append(true).open(&path);
    let mut file = match opened {
        Ok(file) => file,
        Err(error) => return Some(format!("the effect journal could not be opened: {error}")),
    };
    let line = format!("{document}\n");
    if let Err(error) = file.write_all(line.as_bytes()) {
        return Some(format!("the effect journal could not be appended: {error}"));
    }
    None
}

/// Announce that this image really carries the boundary, so the driver can refuse to wait for one
/// that does not exist. Written once, at startup, before the pipe is published.
pub fn announce_startup() {
    let Some(directory) = control_directory() else {
        return;
    };
    let document = json!({
        "schema": "aura.acceptance-barrier.capability.v1",
        "protocol": BUILD_MARKER,
        "feature": "acceptance-barrier",
        "pid": std::process::id(),
        "started_at_ms": now_ms(),
        "max_hold_ms": MAX_HOLD_MS,
        "boundaries": [Boundary::Processing.as_str(), Boundary::Reply.as_str()],
        // The clock the fallback budget is measured on, so a reader does not have to assume it.
        "fallback_clock": "instant_monotonic",
    });
    if let Err(error) = write_json(&directory.join(CAPABILITY_FILE), &document) {
        eprintln!("acceptance barrier capability was not written: {error}");
    }
}

/// Read the installed hold and say honestly how this request relates to it.
///
/// `boundary` is the boundary being asked about; `None` asks the identity question only ("is this
/// request the one the install designates?"), which is what the processing hook needs so the effect
/// recorded later can be tied to the designated invocation even when the hold itself is at the reply
/// boundary. An install file that exists but cannot be read is its own answer: the caller reports it
/// instead of treating it as "no hold".
fn install_match(boundary: Option<Boundary>, method: &str, params: &Value) -> InstallMatch {
    let Some(directory) = control_directory() else {
        return InstallMatch::NoMatch;
    };
    let install: Install = match read_json(&directory.join(INSTALL_FILE)) {
        Ok(Some(install)) => install,
        Ok(None) => return InstallMatch::NoMatch,
        Err(error) => {
            return InstallMatch::Invalid(format!(
                "the barrier install file could not be read: {error}"
            ));
        }
    };
    if let Some(boundary) = boundary
        && install.boundary != boundary
    {
        return InstallMatch::NoMatch;
    }
    if install.method != method {
        return InstallMatch::NoMatch;
    }
    // The marker has to be a real part of what this request carries: a match is a property of the
    // request, never of which request happened to arrive next.
    let Some(serialized) = serde_json::to_string(params).ok() else {
        return InstallMatch::NoMatch;
    };
    if install.marker.is_empty() || !serialized.contains(&install.marker) {
        return InstallMatch::NoMatch;
    }
    InstallMatch::Match(install)
}

/// Remember the request this install designated, without losing an invocation id already known.
fn remember_designation(marker: &str, request_id: CorrelationId, invocation: Option<String>) {
    let Ok(mut slot) = DESIGNATED.lock() else {
        return;
    };
    let invocation = match (&*slot, invocation) {
        (Some(previous), None) if previous.request_id == request_id => {
            previous.command_invocation_id.clone()
        }
        (_, invocation) => invocation,
    };
    *slot = Some(Designation {
        command_invocation_id: invocation,
        marker: marker.to_owned(),
        request_id,
    });
}

/// The designation for one request, if this install really designated it.
fn designation_for(request_id: CorrelationId) -> Option<Designation> {
    DESIGNATED
        .lock()
        .ok()?
        .as_ref()
        .filter(|designation| designation.request_id == request_id)
        .cloned()
}

/// Take the single hold slot. The returned owner releases it on `Drop`, including on cancellation.
fn claim(designation: &Designation) -> Option<HoldOwner> {
    let Ok(mut slot) = HELD.lock() else {
        return None;
    };
    if slot.is_some() {
        return None;
    }
    *slot = Some(designation.clone());
    Some(HoldOwner { request_id: designation.request_id })
}

/// Whether the driver has released this hold.
fn released(marker: &str) -> bool {
    let Some(directory) = control_directory() else {
        return true;
    };
    #[derive(Deserialize)]
    struct Release {
        marker: String,
        release: bool,
    }
    match read_json::<Release>(&directory.join(RELEASE_FILE)) {
        Ok(document) => document.is_some_and(|release| release.release && release.marker == marker),
        Err(error) => {
            // A release file that cannot be read is NOT a release: reporting it as one would hide a
            // real read failure behind an early end to the hold.
            write_error(
                &format!("the barrier release file could not be read: {error}"),
                Some(marker),
            );
            false
        }
    }
}

fn write_state(
    boundary: Boundary,
    designation: &Designation,
    state: &str,
    blocked_at_ms: u64,
    fallback_release: bool,
) {
    let Some(directory) = control_directory() else {
        return;
    };
    let now = now_ms();
    let document = json!({
        "schema": "aura.acceptance-barrier.state.v1",
        "protocol": BUILD_MARKER,
        "marker": designation.marker,
        "boundary": boundary.as_str(),
        "request_id": designation.request_id.to_string(),
        "command_invocation_id": designation.command_invocation_id,
        "state": state,
        "blocked_at_ms": blocked_at_ms,
        // Only a real release has a release time: while the request is still held this stays null, so
        // the document cannot be read as "released at the moment it was written".
        "released_at_ms": if state == "released" { json!(now) } else { Value::Null },
        "hold_ms": if state == "released" { json!(now.saturating_sub(blocked_at_ms)) } else { Value::Null },
        "fallback_release": fallback_release,
        "pid": std::process::id(),
    });
    if let Err(error) = write_json(&directory.join(STATE_FILE), &document) {
        // A state document that was not written is a real failure: the driver must not be left
        // reading the previous scenario's state as this hold's outcome.
        write_error(
            &format!("the barrier state document could not be written: {error}"),
            Some(&designation.marker),
        );
    }
}

/// Hold at the real dispatch boundary, or return at once when this request is not the designated one.
///
/// Called AFTER authorization, the command claim, the ledger reservation and the router decision, and
/// BEFORE the real handler: nothing about how the request was admitted or authorized changes, and the
/// handler that runs afterwards is the production one producing the production result.
pub async fn hold_processing(
    method: &str,
    params: &Value,
    correlation_id: CorrelationId,
    command_invocation_id: Option<aura_domain_common::CommandInvocationId>,
) {
    if control_directory().is_none() {
        return;
    }
    // Identity first, and for ANY boundary: when the hold itself sits at the reply boundary this is
    // still the only place that sees the request before its handler, and it is where the
    // CommandInvocationId the Main really sent is still available.
    match install_match(None, method, params) {
        InstallMatch::Match(install) => remember_designation(
            &install.marker,
            correlation_id,
            command_invocation_id.map(|id| id.to_string()),
        ),
        InstallMatch::Invalid(reason) => {
            write_error(&reason, None);
            return;
        }
        InstallMatch::NoMatch => {}
    }
    hold(Boundary::Processing, method, params, correlation_id).await;
}

/// Hold the real reply, or return at once when this request is not the designated one.
///
/// Called AFTER the handler returned and after the confirmed outcome was recorded, and BEFORE the
/// frame is written: the effect really happened exactly once, and only its announcement is late.
pub async fn hold_reply(method: &str, params: &Value, correlation_id: CorrelationId) {
    hold(Boundary::Reply, method, params, correlation_id).await;
}

/// Record that the real handler AND the real broker transaction finished for the designated request.
///
/// Called after `broker::run` returned: authentication, authorization, the command claim, the ledger
/// reservation and the journal evidence have all already run, and the effect (if any) is committed.
/// Nothing is recorded for a request no install designated, and the entry never carries request
/// parameters.
///
/// `terminal` is the terminal of THAT SAME dispatch, and it is copied from the value the dispatcher
/// really produced - `IpcRequestTerminal::Succeeded` when it returned `Ok`, otherwise the `terminal`
/// of the `IpcErrorEnvelope` it really returned. It is not a guess, not a default and not a second
/// source: it is the same contract value the reply would carry. `committed` stays exactly what it has
/// always been (`outcome.is_ok()`), because a reader that only knows the handler returned a value must
/// keep seeing that; the terminal is an additional, more specific fact.
pub fn record_effect(
    method: &str,
    correlation_id: CorrelationId,
    command_invocation_id: Option<aura_domain_common::CommandInvocationId>,
    committed: bool,
    terminal: IpcRequestTerminal,
) {
    if control_directory().is_none() {
        return;
    }
    let Some(designation) = designation_for(correlation_id) else {
        return;
    };
    let invocation = command_invocation_id
        .map(|id| id.to_string())
        .or_else(|| designation.command_invocation_id.clone());
    let document = json!({
        "schema": "aura.acceptance-barrier.effect.v1",
        "protocol": BUILD_MARKER,
        "event": "effect",
        "marker": designation.marker,
        "method": method,
        "request_id": correlation_id.to_string(),
        "command_invocation_id": invocation,
        "committed": committed,
        "terminal": terminal,
        "at_ms": now_ms(),
        "pid": std::process::id(),
    });
    if let Some(reason) = append_effect(&document) {
        write_error(&reason, Some(&designation.marker));
    }
}

async fn hold(boundary: Boundary, method: &str, params: &Value, correlation_id: CorrelationId) {
    if control_directory().is_none() {
        return;
    }
    let install = match install_match(Some(boundary), method, params) {
        InstallMatch::Match(install) => install,
        InstallMatch::Invalid(reason) => {
            write_error(&reason, None);
            return;
        }
        InstallMatch::NoMatch => return,
    };
    let known_invocation =
        designation_for(correlation_id).and_then(|known| known.command_invocation_id);
    let designation = Designation {
        command_invocation_id: known_invocation.clone(),
        marker: install.marker.clone(),
        request_id: correlation_id,
    };
    remember_designation(&install.marker, correlation_id, known_invocation);
    // The owner releases the slot when this future ends for ANY reason, including cancellation.
    let Some(_owner) = claim(&designation) else {
        // Another request already owns this install; this one is not the designated request.
        return;
    };
    let blocked_at = Instant::now();
    let blocked_at_ms = now_ms();
    write_state(boundary, &designation, "blocked", blocked_at_ms, false);
    let mut fallback_release = false;
    loop {
        if released(&designation.marker) {
            break;
        }
        // Monotonic: the fallback budget is a duration, not a wall-clock comparison.
        if blocked_at.elapsed() >= Duration::from_millis(MAX_HOLD_MS) {
            fallback_release = true;
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    write_state(boundary, &designation, "released", blocked_at_ms, fallback_release);
}
