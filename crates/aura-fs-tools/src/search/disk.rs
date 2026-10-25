mod scan;
use scan::scan_disk_once;
use std::{
    cell::Cell,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};

use aura_domain_common::ContentHash;
use aura_workspace::SecurePathResolver;
use aura_workspace_domain::{FileStorageSource, FsSearchIncompleteReason, TextEncoding};
use grep_regex::RegexMatcher;
use grep_searcher::{BinaryDetection, Encoding, Searcher, SearcherBuilder};

use super::{
    budget::{ByteBudget, Grow},
    cancel::SearchCancel,
    candidates::SearchCandidate,
    classify::{ClassifiedWorkspace, classify_workspace},
    diagnostics::{StageDiagnosticsHandle, StageTimer},
    policy::{BUDGET_WAIT_POLL, LINE_HEAP_LIMIT, MAX_FILE_BYTES, PROBE_BYTES},
    sink::MatchSink,
    types::{FileOutcome, to_wire_match},
};
use crate::FsToolError;

/// An encoding the search may use because a reliable source declared it: a byte
/// order mark, or an encoding a catalog entry recorded. Content that is neither
/// valid UTF-8 nor declared is unsupported, never guessed.
#[derive(Clone, Copy)]
enum DeclaredEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

enum ProbeKind {
    Text(DeclaredEncoding),
    Binary,
    Unsupported,
}

/// Budget allowance held for one file. Every consumption for that file goes
/// through this owner, so a scan cannot bill bytes it never reserved and cannot
/// release an allowance it is still using.
struct Held {
    budget: Arc<ByteBudget>,
    ordinal: usize,
    reserved: Cell<u64>,
    used: Cell<u64>,
    /// Set once this attempt has settled, so the panic-safe drop cannot settle a
    /// second time.
    released: Cell<bool>,
}

/// Why a read could not obtain allowance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Allowance {
    /// Bytes this read may use (possibly zero for an empty request).
    Granted(usize),
    /// The pool still holds bytes, but an earlier ordinal holds the turn. The
    /// file must wait, not be reported as over budget.
    WaitingForPredecessor,
    /// This ordinal holds the turn and the unreserved pool is empty.
    Exhausted,
}

impl Held {
    fn new(budget: &Arc<ByteBudget>, ordinal: usize, reserved: u64) -> Self {
        Self {
            budget: Arc::clone(budget),
            ordinal,
            reserved: Cell::new(reserved),
            used: Cell::new(0),
            released: Cell::new(false),
        }
    }

    fn available(&self) -> u64 {
        self.reserved.get().saturating_sub(self.used.get())
    }

    /// Allowance this read may use, granted before any byte is read.
    fn take_for_read(&self, want: usize) -> Allowance {
        let want = u64::try_from(want).unwrap_or(u64::MAX);
        if want == 0 {
            return Allowance::Granted(0);
        }
        let available = self.available();
        if available > 0 {
            let allow = available.min(want);
            return match usize::try_from(allow) {
                Ok(allow) if allow > 0 => Allowance::Granted(allow),
                _ => Allowance::Granted(0),
            };
        }
        match self.budget.try_grow_for(self.ordinal, want) {
            Grow::Granted(granted) => {
                self.reserved.set(self.reserved.get().saturating_add(granted));
                match usize::try_from(granted.min(want)) {
                    Ok(allow) if allow > 0 => Allowance::Granted(allow),
                    _ => Allowance::Granted(0),
                }
            }
            Grow::WaitingForPredecessor => Allowance::WaitingForPredecessor,
            Grow::Exhausted => Allowance::Exhausted,
        }
    }

    /// Bill bytes that were actually read against this file's allowance.
    fn charge(&self, used: u64) {
        self.used.set(self.used.get().saturating_add(used));
    }

    /// Settle this *attempt*: bill only the bytes this instance actually charged
    /// and return the rest of its allowance to the pool for later candidates.
    ///
    /// Earlier attempts are reported through `FileOutcome.bytes` only, never
    /// settled again here. This does **not** finish the file's ordinal: a retry
    /// of the same ordinal still needs its place in the turn order, so finishing
    /// is the engine's job once the file has its final result.
    fn release(&self) {
        if self.released.replace(true) {
            return;
        }
        let reserved = self.reserved.replace(0);
        let used = self.used.replace(0).min(reserved);
        self.budget.release_attempt(used, reserved);
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // A panicking or otherwise unwinding scan must not strand its
        // reservation: dropping the owner settles exactly what it really read.
        // An explicit release already marked it, so nothing is settled twice.
        self.release();
    }
}

/// Bytes already consumed by earlier attempts for the same file, so a retry is
/// billed once rather than twice.
#[derive(Clone, Copy, Default)]
struct PriorBytes(u64);

impl PriorBytes {
    fn accumulate(self, extra: u64) -> Self {
        Self(self.0.saturating_add(extra))
    }

    fn get(self) -> u64 {
        self.0
    }
}

/// The terminal for a scan that stopped because the search was cancelled.
fn cancelled_outcome(candidate: &SearchCandidate, bytes: u64) -> FileOutcome {
    FileOutcome {
        ordinal: candidate.ordinal,
        path: candidate.path.clone(),
        matches: Vec::new(),
        scanned: false,
        skipped: true,
        bytes,
        reasons: Vec::new(),
    }
}

// Test-only injections. They are compiled out of production builds and are
// process-wide (not thread-scoped) because a search runs on the shared pool's
// threads, so a test that arms a fault on its own thread must still be observed
// by the worker that reads the file. Each is keyed by workspace path, so two
// tests running at once cannot see each other's injection.
#[cfg(any(test, feature = "search-test-faults"))]
pub mod faults {
    use std::{
        collections::HashMap,
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    fn retry_paths() -> &'static Mutex<Vec<String>> {
        static PATHS: std::sync::OnceLock<Mutex<Vec<String>>> = std::sync::OnceLock::new();
        PATHS.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn gates() -> &'static Mutex<HashMap<String, Arc<ScanGate>>> {
        static GATES: std::sync::OnceLock<Mutex<HashMap<String, Arc<ScanGate>>>> =
            std::sync::OnceLock::new();
        GATES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn arm_retry(path: &str) {
        retry_paths().lock().unwrap_or_else(|error| error.into_inner()).push(path.to_owned());
    }

    /// Take (consume) an armed retry fault for `path`.
    pub fn take_retry(path: &str) -> bool {
        let mut paths = retry_paths().lock().unwrap_or_else(|error| error.into_inner());
        match paths.iter().position(|armed| armed == path) {
            Some(index) => {
                paths.remove(index);
                true
            }
            None => false,
        }
    }

    /// Park every scan of `path` until [`ScanGate::release`] runs, so a test can
    /// hold one ordinal's turn for as long as it needs.
    pub fn arm_gate(path: &str) -> Arc<ScanGate> {
        let gate = Arc::new(ScanGate::default());
        gates()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(path.to_owned(), Arc::clone(&gate));
        gate
    }

    /// Park every scan of `path` until [`ScanGate::release`] runs, and ignore
    /// cancellation while parked.
    ///
    /// A real scan notices a cancelled search between reads, so cancelling is
    /// normally enough to end one. This gate models the remaining window: a worker
    /// already inside a single blocking OS call, which can observe nothing — not
    /// even the cancel flag — until that call returns. A disconnect must keep such
    /// work tracked instead of declaring it reclaimed.
    // A crate that enables `search-test-faults` calls this from its own
    // integration tests; this crate's own test build compiles it without a caller.
    #[cfg_attr(test, allow(dead_code))]
    pub fn arm_blocking_gate(path: &str) -> Arc<ScanGate> {
        let gate = Arc::new(ScanGate { blocking: true, ..ScanGate::default() });
        gates()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(path.to_owned(), Arc::clone(&gate));
        gate
    }

    pub fn gate_for(path: &str) -> Option<Arc<ScanGate>> {
        gates().lock().unwrap_or_else(|error| error.into_inner()).get(path).cloned()
    }

    #[derive(Default)]
    pub struct ScanGate {
        state: Mutex<GateState>,
        cv: Condvar,
        /// When set, [`ScanGate::wait`] ignores cancellation: the parked scan
        /// stands for a worker inside one blocking OS call.
        blocking: bool,
    }

    #[derive(Default)]
    struct GateState {
        parked: usize,
        released: bool,
    }

    impl ScanGate {
        /// Wait until the test releases the gate or `cancel` reports the search
        /// is over; returns whether the scan may continue (false = cancelled).
        pub fn wait(&self, cancel: &super::super::cancel::SearchCancel) {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.parked += 1;
            self.cv.notify_all();
            while !state.released {
                if !self.blocking && cancel.is_cancelled() {
                    state.parked = state.parked.saturating_sub(1);
                    return;
                }
                let waited = self.cv.wait_timeout(state, Duration::from_millis(5));
                state = waited.unwrap_or_else(|error| error.into_inner()).0;
            }
            state.parked = state.parked.saturating_sub(1);
        }

        /// Wait (bounded) until at least `count` scans are parked on this gate.
        #[must_use]
        pub fn wait_for_parked(&self, count: usize, timeout: Duration) -> bool {
            let deadline = std::time::Instant::now() + timeout;
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            while state.parked < count {
                if std::time::Instant::now() >= deadline {
                    return false;
                }
                let waited = self.cv.wait_timeout(state, Duration::from_millis(5));
                state = waited.unwrap_or_else(|error| error.into_inner()).0;
            }
            true
        }

        pub fn release(&self) {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.released = true;
            self.cv.notify_all();
        }

        #[must_use]
        pub fn parked(&self) -> usize {
            self.state.lock().unwrap_or_else(|error| error.into_inner()).parked
        }
    }

    /// How many scans have run this path, so a test can assert a retry happened.
    pub(crate) fn scan_counts() -> &'static Mutex<HashMap<String, Arc<AtomicUsize>>> {
        static COUNTS: std::sync::OnceLock<Mutex<HashMap<String, Arc<AtomicUsize>>>> =
            std::sync::OnceLock::new();
        COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn count_scan(path: &str) -> usize {
        scan_counts()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(path.to_owned())
            .or_default()
            .fetch_add(1, Ordering::SeqCst)
    }

    pub fn scans_for(path: &str) -> usize {
        scan_counts()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(path)
            .map_or(0, |count| count.load(Ordering::SeqCst))
    }
}

/// Park this scan if a test armed a gate for the file, and count the attempt.
///
/// The key is the workspace-relative path. Tests that arm a fault use file names
/// unique to that test, so parallel tests cannot observe each other's injection.
#[cfg(any(test, feature = "search-test-faults"))]
fn enter_test_gate(candidate: &SearchCandidate, cancel: &SearchCancel) {
    faults::count_scan(candidate.path.as_str());
    if let Some(gate) = faults::gate_for(candidate.path.as_str()) {
        gate.wait(cancel);
    }
}

#[cfg(not(any(test, feature = "search-test-faults")))]
fn enter_test_gate(_candidate: &SearchCandidate, _cancel: &SearchCancel) {}

#[cfg(any(test, feature = "search-test-faults"))]
fn take_retry_fault(candidate: &SearchCandidate) -> bool {
    faults::take_retry(candidate.path.as_str())
}

#[cfg(not(any(test, feature = "search-test-faults")))]
fn take_retry_fault(_candidate: &SearchCandidate) -> bool {
    false
}

/// One disk scan for one candidate, carrying everything the workers share.
pub(crate) struct DiskScanRequest<'a> {
    pub(crate) resolver: &'a SecurePathResolver,
    pub(crate) candidate: &'a SearchCandidate,
    pub(crate) matcher: &'a RegexMatcher,
    pub(crate) budget: &'a Arc<ByteBudget>,
    pub(crate) reserved: u64,
    pub(crate) cancel: &'a SearchCancel,
    pub(crate) match_cap: usize,
    pub(crate) stages: &'a StageDiagnosticsHandle,
}

pub(crate) fn scan_disk(request: DiskScanRequest<'_>) -> Result<FileOutcome, FsToolError> {
    let DiskScanRequest {
        resolver,
        candidate,
        matcher,
        budget,
        reserved,
        cancel,
        match_cap,
        stages,
    } = request;
    match scan_disk_once(DiskOnce {
        resolver,
        candidate,
        matcher,
        budget,
        reserved,
        cancel,
        match_cap,
        prior: PriorBytes::default(),
        stages,
    })? {
        DiskScan::Outcome(outcome) => Ok(outcome),
        DiskScan::Retry { bytes } => {
            // One retry is allowed; a second change skips the file with the
            // change recorded rather than dropping the matches of a stable
            // second read.
            let extra = candidate.byte_length.unwrap_or(MAX_FILE_BYTES).clamp(1, MAX_FILE_BYTES);
            let Some(again) = budget.try_reserve(extra) else {
                return Ok(FileOutcome::skipped(
                    candidate.ordinal,
                    candidate.path.clone(),
                    bytes,
                    vec![FsSearchIncompleteReason::ScanByteBudget],
                ));
            };
            match scan_disk_once(DiskOnce {
                resolver,
                candidate,
                matcher,
                budget,
                reserved: again,
                cancel,
                match_cap,
                prior: PriorBytes(bytes),
                stages,
            })? {
                DiskScan::Outcome(outcome) => Ok(retry_outcome(outcome)),
                DiskScan::Retry { bytes } => Ok(retry_outcome(FileOutcome::skipped(
                    candidate.ordinal,
                    candidate.path.clone(),
                    bytes,
                    vec![FsSearchIncompleteReason::FileChangedDuringSearch],
                ))),
            }
        }
    }
}

/// Result of the one allowed retry. A stable second read is the answer: its
/// matches and hash are returned as-is, and the bytes already spent on both
/// attempts are kept once. Only a second real change was already converted to a
/// bounded skip by the caller.
fn retry_outcome(outcome: FileOutcome) -> FileOutcome {
    outcome
}

enum DiskScan {
    Outcome(FileOutcome),
    Retry { bytes: u64 },
}

struct DiskOnce<'a> {
    resolver: &'a SecurePathResolver,
    candidate: &'a SearchCandidate,
    matcher: &'a RegexMatcher,
    budget: &'a Arc<ByteBudget>,
    reserved: u64,
    cancel: &'a SearchCancel,
    match_cap: usize,
    prior: PriorBytes,
    stages: &'a StageDiagnosticsHandle,
}

fn map_open_error(
    error: aura_workspace::WorkspaceError,
    candidate: &SearchCandidate,
    prior_bytes: u64,
) -> Result<DiskScan, FsToolError> {
    match classify_workspace(error) {
        ClassifiedWorkspace::Temporary => Ok(DiskScan::Outcome(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            prior_bytes,
            vec![FsSearchIncompleteReason::FileTemporarilyUnavailable],
        ))),
        ClassifiedWorkspace::Changed => Ok(DiskScan::Outcome(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            prior_bytes,
            vec![FsSearchIncompleteReason::FileChangedDuringSearch],
        ))),
        ClassifiedWorkspace::Fatal(error) => Err(error),
    }
}

fn classify_probe(probe: &[u8], catalog: Option<TextEncoding>) -> ProbeKind {
    if probe.starts_with(&[0xef, 0xbb, 0xbf]) {
        return ProbeKind::Text(DeclaredEncoding::Utf8Bom);
    }
    if probe.starts_with(&[0xff, 0xfe]) {
        return ProbeKind::Text(DeclaredEncoding::Utf16Le);
    }
    if probe.starts_with(&[0xfe, 0xff]) {
        return ProbeKind::Text(DeclaredEncoding::Utf16Be);
    }
    if probe.contains(&0) {
        return ProbeKind::Binary;
    }
    if utf8_prefix(probe) {
        return ProbeKind::Text(DeclaredEncoding::Utf8);
    }
    match catalog {
        Some(TextEncoding::Windows1252) => ProbeKind::Text(DeclaredEncoding::Windows1252),
        Some(TextEncoding::Utf8Bom) => ProbeKind::Text(DeclaredEncoding::Utf8Bom),
        Some(TextEncoding::Utf16Le) => ProbeKind::Text(DeclaredEncoding::Utf16Le),
        Some(TextEncoding::Utf16Be) => ProbeKind::Text(DeclaredEncoding::Utf16Be),
        Some(TextEncoding::Utf8) | None => ProbeKind::Unsupported,
    }
}

fn utf8_prefix(probe: &[u8]) -> bool {
    if std::str::from_utf8(probe).is_ok() {
        return true;
    }
    for trim in 1..=3 {
        if probe.len() <= trim {
            continue;
        }
        let split = probe.len() - trim;
        if std::str::from_utf8(&probe[..split]).is_ok() && is_truncated_utf8_tail(&probe[split..]) {
            return true;
        }
    }
    false
}

fn is_truncated_utf8_tail(rest: &[u8]) -> bool {
    let Some(lead) = rest.first().copied() else {
        return false;
    };
    let expected = match lead {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return false,
    };
    rest.len() < expected && rest.iter().skip(1).all(|byte| byte & 0b1100_0000 == 0b1000_0000)
}

fn disk_searcher(kind: DeclaredEncoding) -> Searcher {
    let mut builder = SearcherBuilder::new();
    builder
        .line_number(true)
        .multi_line(false)
        .binary_detection(BinaryDetection::quit(0))
        .heap_limit(Some(LINE_HEAP_LIMIT));
    match kind {
        DeclaredEncoding::Utf8 => {
            builder.encoding(None).bom_sniffing(true);
        }
        DeclaredEncoding::Utf8Bom => {
            builder.encoding(Encoding::new("utf-8").ok()).bom_sniffing(true);
        }
        DeclaredEncoding::Utf16Le => {
            builder.encoding(Encoding::new("utf-16le").ok()).bom_sniffing(true);
        }
        DeclaredEncoding::Utf16Be => {
            builder.encoding(Encoding::new("utf-16be").ok()).bom_sniffing(true);
        }
        DeclaredEncoding::Windows1252 => {
            builder.encoding(Encoding::new("windows-1252").ok()).bom_sniffing(false);
        }
    }
    builder.build()
}

enum Verify {
    Ok {
        hash: ContentHash,
        hashed_bytes: u64,
    },
    Changed {
        extra_bytes: u64,
    },
    /// The file could not be re-read because its allowance ran out. This is a
    /// budget outcome, never an I/O unavailability.
    BudgetExhausted {
        extra_bytes: u64,
    },
    Unavailable {
        extra_bytes: u64,
    },
    /// The search was cancelled while waiting for an earlier ordinal's turn.
    /// Cancellation is its own terminal, never a budget or I/O outcome.
    Cancelled {
        extra_bytes: u64,
    },
}

struct ConsistencyInput<'a> {
    candidate: &'a SearchCandidate,
    source: &'a mut aura_windows_platform::SearchSource,
    start_identity: &'a aura_windows_platform::NativeFileIdentity,
    writers_denied: bool,
    scanned_bytes: u64,
    scan_hash: &'a ContentHash,
    held: &'a Held,
    cancel: &'a SearchCancel,
}

/// Re-hash the file through the handle the scan already verified. Reads are
/// charged to the file allowance before bytes are consumed, and the per-file
/// limit is reported as unavailability rather than mistaken for a change.
fn verify_consistency(input: ConsistencyInput<'_>) -> Verify {
    let ConsistencyInput {
        candidate,
        source,
        start_identity,
        writers_denied,
        scanned_bytes,
        scan_hash,
        held,
        cancel,
    } = input;
    if take_retry_fault(candidate) {
        return Verify::Changed { extra_bytes: 0 };
    }
    if source.identity() != start_identity {
        return Verify::Changed { extra_bytes: 0 };
    }
    if writers_denied {
        return Verify::Ok { hash: scan_hash.clone(), hashed_bytes: 0 };
    }
    if source.file_mut().seek(SeekFrom::Start(0)).is_err() {
        return Verify::Unavailable { extra_bytes: 0 };
    }
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0_u8; 65_536];
    let mut hashed = 0_u64;
    // Wait for an earlier ordinal to release its allowance. "Not my turn yet"
    // is not budget exhaustion, so this waits until the turn arrives or the
    // search is cancelled, and only a real refusal ends the re-hash.
    loop {
        if cancel.is_cancelled() {
            return Verify::Cancelled { extra_bytes: hashed };
        }
        let remaining = MAX_FILE_BYTES.saturating_sub(hashed);
        if remaining == 0 {
            // The per-file limit was reached. Whether the file simply ends here
            // is decided by the comparison below, not by this branch.
            break;
        }
        let want = usize::try_from(remaining.min(buf.len() as u64)).unwrap_or(buf.len());
        // Grant before reading. "Not my turn yet" is not the same as "budget
        // gone": the first waits for earlier ordinals to settle and retries,
        // the second is reported as a budget outcome.
        match held.take_for_read(want) {
            Allowance::Granted(allow) => {
                let read = match source.file_mut().read(&mut buf[..allow]) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(_) => return Verify::Unavailable { extra_bytes: hashed },
                };
                let added = u64::try_from(read).unwrap_or(0);
                held.charge(added);
                hasher.update(&buf[..read]);
                hashed = hashed.saturating_add(added);
            }
            Allowance::WaitingForPredecessor => std::thread::sleep(BUDGET_WAIT_POLL),
            Allowance::Exhausted => return Verify::BudgetExhausted { extra_bytes: hashed },
        }
    }
    let retry_hash = ContentHash {
        algorithm: aura_domain_common::ContentHashAlgorithm::Blake3,
        hex: hasher.finalize().to_hex().to_string(),
    };
    if hashed != scanned_bytes || retry_hash != *scan_hash {
        return Verify::Changed { extra_bytes: hashed };
    }
    Verify::Ok { hash: retry_hash, hashed_bytes: hashed }
}

struct MeteredReader<'a, R> {
    inner: R,
    held: &'a Held,
    hasher: blake3::Hasher,
    read: u64,
    expected: u64,
    cancel: SearchCancel,
    exhausted: bool,
    too_large: bool,
}

impl<R: Read> Read for MeteredReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_cancelled() || self.exhausted || self.too_large {
            return Ok(0);
        }
        self.cancel.mark_started();
        if self.read >= self.expected {
            // The declared size was fully read; report EOF instead of letting
            // grep-searcher grow the allowance past the known length.
            return Ok(0);
        }
        let file_left = MAX_FILE_BYTES.min(self.expected).saturating_sub(self.read);
        if file_left == 0 {
            self.too_large = true;
            return Ok(0);
        }
        let want = usize::try_from(file_left.min(u64::try_from(buf.len()).unwrap_or(u64::MAX)))
            .unwrap_or(buf.len());
        // Grant before reading, then bill the bytes actually read, so the
        // remaining allowance shrinks with real I/O instead of being reused.
        loop {
            match self.held.take_for_read(want) {
                Allowance::Granted(allow) => {
                    let count = self.inner.read(&mut buf[..allow])?;
                    if count == 0 {
                        return Ok(0);
                    }
                    self.held.charge(u64::try_from(count).unwrap_or(0));
                    self.hasher.update(&buf[..count]);
                    self.read = self.read.saturating_add(u64::try_from(count).unwrap_or(0));
                    return Ok(count);
                }
                Allowance::WaitingForPredecessor => {
                    // "Not my turn yet" is not "the budget is gone" and not end
                    // of file: a short read here would end the scan silently and
                    // drop this file's matches. Wait until the turn arrives or
                    // the search is cancelled, then retry.
                    if self.cancel.is_cancelled() {
                        return Ok(0);
                    }
                    std::thread::sleep(BUDGET_WAIT_POLL);
                }
                Allowance::Exhausted => {
                    self.exhausted = true;
                    return Ok(0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::policy::MAX_TOTAL_BYTES;
    use super::*;
    use std::{cell::Cell, io::Cursor, rc::Rc};

    /// Reader wrapper that records how many bytes the layer below actually
    /// served, so a test can assert real I/O instead of bookkeeping.
    struct CountingReader {
        inner: Cursor<Vec<u8>>,
        served: Rc<Cell<u64>>,
    }

    impl Read for CountingReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let count = self.inner.read(buf)?;
            self.served.set(self.served.get().saturating_add(u64::try_from(count).unwrap_or(0)));
            Ok(count)
        }
    }

    fn metered_counting<'a>(
        held: &'a Held,
        bytes: Vec<u8>,
        expected: u64,
        served: Rc<Cell<u64>>,
    ) -> MeteredReader<'a, CountingReader> {
        MeteredReader {
            inner: CountingReader { inner: Cursor::new(bytes), served },
            held,
            hasher: blake3::Hasher::new(),
            read: 0,
            expected,
            cancel: SearchCancel::new(),
            exhausted: false,
            too_large: false,
        }
    }

    fn metered<'a>(
        held: &'a Held,
        bytes: Vec<u8>,
        expected: u64,
    ) -> MeteredReader<'a, Cursor<Vec<u8>>> {
        MeteredReader {
            inner: Cursor::new(bytes),
            held,
            hasher: blake3::Hasher::new(),
            read: 0,
            expected,
            cancel: SearchCancel::new(),
            exhausted: false,
            too_large: false,
        }
    }

    #[test]
    fn extra_read_after_known_size_does_not_grow_or_leak() {
        let budget = Arc::new(ByteBudget::new());
        let reserved = budget.try_reserve(17).expect("reserve");
        let held = Held::new(&budget, 0, reserved);
        let mut reader = metered(&held, vec![b'a'; 17], 17);
        let mut buf = [0_u8; 65_536];
        assert_eq!(reader.read(&mut buf).expect("read"), 17);
        assert_eq!(reader.read(&mut buf).expect("eof"), 0);
        assert_eq!(held.reserved.get(), 17);
        held.release();
        assert_eq!(budget.consumed(), 17);
        assert_eq!(budget.remaining_unreserved(), MAX_TOTAL_BYTES - 17);
    }

    #[test]
    fn probe_bytes_are_billed_against_the_same_allowance() {
        let budget = Arc::new(ByteBudget::new());
        let reserved = budget.try_reserve(8).expect("reserve");
        let held = Held::new(&budget, 0, reserved);
        held.charge(8);
        let mut reader = metered(&held, vec![b'a'; 4], 4);
        let mut buf = [0_u8; 64];
        assert_eq!(reader.read(&mut buf).expect("read"), 4);
        held.release();
        assert_eq!(budget.consumed(), 12);
        assert_eq!(budget.remaining_unreserved(), MAX_TOTAL_BYTES - 12);
    }

    #[test]
    fn repeated_reads_shrink_the_allowance_and_bill_real_io() {
        let budget = Arc::new(ByteBudget::new());
        let reserved = budget.try_reserve(10).expect("reserve");
        let held = Held::new(&budget, 0, reserved);
        let served = Rc::new(Cell::new(0));
        let mut reader = metered_counting(&held, vec![b'a'; 10], 10, Rc::clone(&served));
        let mut buf = [0_u8; 4];
        // Four short reads of 4 bytes each: only 10 bytes exist, so the third
        // read may serve at most 2 and the fourth must stop without touching
        // the file. The old code re-used the same allowance every time and
        // would have served 12 bytes.
        assert_eq!(reader.read(&mut buf).expect("first"), 4);
        assert_eq!(reader.read(&mut buf).expect("second"), 4);
        assert_eq!(reader.read(&mut buf).expect("third"), 2);
        assert_eq!(reader.read(&mut buf).expect("eof"), 0);
        assert_eq!(served.get(), 10, "layer below must never serve more than the allowance");
        assert_eq!(reader.read, 10);
        held.release();
        assert_eq!(budget.consumed(), 10);
        assert_eq!(budget.remaining_unreserved(), MAX_TOTAL_BYTES - 10);
    }

    #[test]
    fn a_probe_larger_than_the_allowance_reads_only_what_is_granted() {
        let budget = Arc::new(ByteBudget::new());
        let reserved = budget.try_reserve(3).expect("reserve");
        let held = Held::new(&budget, 0, reserved);
        // Saturate the unreserved pool so the grant cannot grow.
        let mut held_everything = Vec::new();
        while let Some(chunk) = budget.try_reserve(MAX_FILE_BYTES) {
            held_everything.push(chunk);
        }
        assert_eq!(budget.try_grow_for(0, 1), Grow::Exhausted);
        let allow = match held.take_for_read(PROBE_BYTES) {
            Allowance::Granted(allow) => allow,
            other => panic!("the held allowance must cover the probe, got {other:?}"),
        };
        assert_eq!(allow, 3, "probe must be limited to the granted allowance");
        held.release();
        for reservation in held_everything {
            budget.release_attempt(0, reservation);
        }
    }

    #[test]
    fn verification_stops_as_unavailable_when_the_allowance_runs_out() {
        let budget = Arc::new(ByteBudget::new());
        let reserved = budget.try_reserve(4).expect("reserve");
        let held = Held::new(&budget, 0, reserved);
        held.charge(4);
        // No allowance left and nothing to grow from: take_for_read refuses, so
        // the caller reports a budget outcome instead of over-reading.
        let mut pool_holder = Vec::new();
        while let Some(chunk) = budget.try_reserve(MAX_FILE_BYTES) {
            pool_holder.push(chunk);
        }
        assert_eq!(held.take_for_read(1024), Allowance::Exhausted);
        for reservation in pool_holder {
            budget.release_attempt(0, reservation);
        }
    }

    #[test]
    fn declared_encoding_is_not_guessed_from_content() {
        // Invalid UTF-8 with no declaration stays unsupported instead of being
        // decoded as windows-1252 for a higher hit rate.
        let latin = [b'c', b'a', b'f', 0xe9, b'\n'];
        assert!(matches!(classify_probe(&latin, None), ProbeKind::Unsupported));
        assert!(matches!(
            classify_probe(&latin, Some(TextEncoding::Windows1252)),
            ProbeKind::Text(DeclaredEncoding::Windows1252)
        ));
        assert!(matches!(
            classify_probe(b"plain ascii", None),
            ProbeKind::Text(DeclaredEncoding::Utf8)
        ));
        assert!(matches!(classify_probe(&[0, 1, 2], None), ProbeKind::Binary));
    }

    #[test]
    fn the_retry_policy_keeps_the_stable_second_read() {
        // A stable second attempt is the answer: its matches and hash are
        // returned, and the change reason is only for a second real change.
        let stable = FileOutcome {
            ordinal: 3,
            path: aura_domain_common::WorkspacePath::parse("a.txt").expect("path"),
            matches: vec![aura_workspace_domain::FsSearchTextMatch {
                path: aura_domain_common::WorkspacePath::parse("a.txt").expect("path"),
                line_index: 1,
                column_character_offset: 0,
                match_character_length: 4,
                excerpt: "needle".to_owned(),
                excerpt_start_character_offset: 0,
                excerpt_truncated_before: false,
                excerpt_truncated_after: false,
                source: aura_workspace_domain::FileStorageSource::Disk,
                logical_hash: ContentHash::blake3(b"needle\n"),
                buffer_revision: None,
                ignored: false,
                generated: false,
            }],
            scanned: true,
            skipped: false,
            bytes: 7,
            reasons: Vec::new(),
        };
        let kept = retry_outcome(stable.clone());
        assert_eq!(kept.matches.len(), 1, "a stable retry must keep its matches");
        assert!(kept.reasons.is_empty(), "a stable retry is not a change");
        assert_eq!(kept.bytes, stable.bytes, "bytes are accumulated once per attempt");

        // Two real changes in a row end as a bounded skip carrying the reason.
        let twice = retry_outcome(FileOutcome::skipped(
            3,
            aura_domain_common::WorkspacePath::parse("a.txt").expect("path"),
            7,
            vec![FsSearchIncompleteReason::FileChangedDuringSearch],
        ));
        assert!(twice.skipped);
        assert!(twice.matches.is_empty());
        assert_eq!(twice.reasons, vec![FsSearchIncompleteReason::FileChangedDuringSearch]);
    }

    /// Build a resolver over a real temporary workspace root so the module's
    /// production scan path can be exercised without the full service.
    fn resolver_for_root(root: &std::path::Path) -> SecurePathResolver {
        let info = SecurePathResolver::inspect_selected_root(root).expect("root info");
        let summary = aura_workspace_domain::WorkspaceSummary {
            workspace_id: aura_domain_common::WorkspaceId::new(),
            display_name: "disk test".to_owned(),
            selected_root: root.display().to_string(),
            canonical_root: info.final_path.display().to_string(),
            root_identity: Some(aura_workspace::domain_identity(&info.identity)),
            state: aura_workspace_domain::WorkspaceState::Online,
            revision: aura_domain_common::Revision::INITIAL,
            catalog_freshness: aura_workspace_domain::CatalogFreshness::Unavailable,
            authorization_epoch: aura_domain_common::Revision::INITIAL,
        };
        SecurePathResolver::from_workspace(&summary).expect("resolver")
    }

    fn candidate_for(path: &str, ordinal: usize) -> SearchCandidate {
        SearchCandidate {
            ordinal,
            path: aura_domain_common::WorkspacePath::parse(path).expect("path"),
            byte_length: None,
            ignored: false,
            generated: false,
            catalog_encoding: None,
        }
    }

    #[test]
    fn a_forced_first_change_retries_and_keeps_the_second_read() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("stable.txt"), "retry_stable_token\n").expect("write");
        let resolver = resolver_for_root(root.path());
        let candidate = candidate_for("stable.txt", 0);
        let matcher = super::super::matcher::compile_matcher(true, "retry_stable_token", true)
            .expect("matcher");
        let budget = Arc::new(ByteBudget::new());
        let reserved = budget.try_reserve(64).expect("reserve");
        let stages = Arc::new(super::super::diagnostics::StageDiagnostics::default());
        let cancel = SearchCancel::new();
        // Force the first attempt to look like a concurrent change. The hook is
        // absolute-path scoped, so no other test or search can consume it.
        faults::arm_retry("stable.txt");
        let outcome = scan_disk(DiskScanRequest {
            resolver: &resolver,
            candidate: &candidate,
            matcher: &matcher,
            budget: &budget,
            reserved,
            cancel: &cancel,
            match_cap: 10,
            stages: &stages,
        })
        .expect("scan");
        assert_eq!(outcome.matches.len(), 1, "a stable second read must keep its match");
        assert!(
            outcome.reasons.is_empty(),
            "a stable retry is not a change: {:?}",
            outcome.reasons
        );
        assert!(outcome.scanned);
        let stages = stages.snapshot();
        assert_eq!(stages.open_calls, 2, "the retry re-opens the file");
        assert_eq!(stages.verify_calls, 2, "one verification per attempt");
        assert!(stages.scanned_bytes > 0);
        // Both attempts really read the file, and the reported bytes are those
        // two real scans plus the verification pass — never a duplicated
        // settlement of one scan.
        let file_bytes = u64::try_from("retry_stable_token\n".len()).expect("len");
        // Two attempts, each doing one scan and one verification pass over the
        // same 19 bytes: 4 × 19, with no settlement counted twice.
        assert_eq!(
            (stages.scanned_bytes, outcome.bytes),
            (file_bytes * 2, file_bytes * 4),
            "each attempt scans and verifies {file_bytes} bytes once"
        );
        // The global budget was billed exactly the bytes those two attempts
        // really read — probe, scan and re-hash per attempt — and the unused
        // reservation (including the retry's extra reservation) went back.
        assert_eq!(
            budget.consumed(),
            file_bytes * 4,
            "settlement must equal the real I/O of both attempts"
        );
        assert!(
            budget.remaining_unreserved() >= super::super::policy::total_bytes() - file_bytes * 4,
            "unused reservation must return to the pool"
        );
    }
}
