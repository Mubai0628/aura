//! Core-side temporary file access. Each change is journaled before applying
//! it to a retained object; a foreign ACL is preserved for explicit repair.
use anyhow::{Result, ensure};
use aura_windows_platform::ExecutionFileSecurity;
use aura_windows_user_contract::ExecutionFileBinding;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::GrantWork;

const MAX_OBJECTS: usize = 100_000;
mod ancestors;
mod control;

pub use control::{GrantControl, GrantOutcome, GrantSignal, GrantStop, GrantVerdict};

#[derive(Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    ProcessBoundary {
        installation: String,
        nonce: String,
        boot: String,
        #[serde(default)]
        order: u64,
    },
    Scope {
        root: ExecutionFileBinding,
        account: String,
        write_sid: Option<String>,
    },
    /// Durable proof that one scope's walk finished: every object under the
    /// root at walk time has an intent, so an object without one appeared
    /// later. Journals from before this record existed cannot prove that;
    /// their remaining objects are never treated as new output.
    ScopeEnumerated {
        root: ExecutionFileBinding,
    },
    /// An object the walk saw but refused to touch (extra links or a reparse
    /// point). Seen is not absent: its identity joins the same proof set as
    /// the intents, so a link-count change after the walk cannot reclassify a
    /// pre-existing object as attempt output.
    Skipped {
        path: PathBuf,
        volume: u64,
        id: [u8; 16],
        reason: String,
    },
    /// Durable proof that this scope's pass did not complete. The launch
    /// dispatches only after every grant completes, so a scope carrying this
    /// record was never followed by an Execute: nothing under it can be
    /// attempt output, and the lease settles once its intents are restored.
    ScopeHalted {
        root: ExecutionFileBinding,
        reason: String,
    },
    Intent {
        path: PathBuf,
        volume: u64,
        id: [u8; 16],
        before: String,
        after: String,
    },
    /// Durable proof that one recorded object no longer exists, and *why*. Two
    /// facts can write it, and both are observations rather than inferences: the
    /// lease's own retained handle reports the object delete-pending, or recovery
    /// asked the volume for the recorded identity and the volume answered that it
    /// has no such object. Nothing else — an access denial, a sharing conflict, an
    /// unsupported volume — is allowed to write this, because each of those can
    /// also describe an object that is still there.
    ///
    /// It is written per object, before the pass settles, so a crash in the middle
    /// of recovery repeats nothing: the second pass skips the identities this
    /// record already accounts for.
    ObjectGone {
        path: PathBuf,
        volume: u64,
        id: [u8; 16],
    },
    ObjectsRestored,
    Released,
}

struct Held {
    object: Arc<ExecutionFileSecurity>,
    before: String,
    after: String,
}
struct Scope {
    root: ExecutionFileBinding,
    account: String,
    write_sid: Option<String>,
    /// Set only when this scope's walk ran to completion: the intent set then
    /// covers everything under the root, which is what makes "absent from the
    /// intents" mean "created afterwards". A partial walk proves nothing
    /// about the objects it never reached.
    enumerated: bool,
    /// Set when this scope's pass ended without completing. With the dispatch
    /// order (Execute only after every grant completes) that proves nothing
    /// under the root was ever executed against, so there is no output to
    /// retire even though the enumeration stayed partial.
    halted: bool,
}
pub struct FileLease {
    journal: File,
    held: Vec<Held>,
    scopes: Vec<Scope>,
    restored: bool,
    objects_restored: bool,
    process_bound: bool,
    root_handles: BTreeMap<PathBuf, Arc<ExecutionFileSecurity>>,
    ancestor_pins: Vec<aura_windows_platform::VerifiedFile>,
    /// Identities the walks saw but refused to touch; part of the same
    /// seen-set as the intents when new output is separated from pre-existing
    /// files.
    skipped: std::collections::BTreeSet<(u64, [u8; 16])>,
    /// Paths whose object this lease proved gone. A delete-pending object cannot
    /// be reopened by name, so the name is the only thing a later walk can match,
    /// and it must be matched instead of reopened.
    gone: Vec<PathBuf>,
    /// Every durable record this lease wrote, so a single grant pass can report the
    /// journal work it caused instead of guessing from the object count.
    records_written: u64,
}

/// Whether two recorded paths name the same object, compared the way the
/// filesystem compares them.
fn same_path(left: &Path, right: &Path) -> bool {
    aura_windows_platform::windows_ordinal_compare_ignore_case(left.as_os_str(), right.as_os_str())
        .is_ok_and(|ordering| ordering == std::cmp::Ordering::Equal)
}

fn path_is_gone(gone: &[PathBuf], path: &Path) -> bool {
    gone.iter().any(|recorded| same_path(recorded, path))
}

impl FileLease {
    /// Leave the durable intents for recovery when the process tree cannot be
    /// confirmed stopped. Never publish a successful cleanup receipt.
    pub fn defer_recovery(&mut self) {
        self.restored = true;
    }
    pub fn create(journal: &Path) -> Result<Self> {
        let journal = std::fs::OpenOptions::new().create_new(true).write(true).open(journal)?;
        Ok(Self {
            journal,
            held: Vec::new(),
            scopes: Vec::new(),
            restored: false,
            objects_restored: false,
            process_bound: false,
            root_handles: BTreeMap::new(),
            ancestor_pins: Vec::new(),
            skipped: std::collections::BTreeSet::new(),
            gone: Vec::new(),
            records_written: 0,
        })
    }
    /// Reuse the retained no-delete root handle across serialized runtime
    /// leases. Reopening MAXIMUM_ALLOWED would request DELETE and conflict
    /// with that very root pin; changing sharing would weaken the scope lock.
    pub(crate) fn share_roots_from(&mut self, previous: &Self) {
        self.root_handles.extend(previous.root_handles.iter().map(|(p, h)| (p.clone(), h.clone())));
    }
    /// Durable before Job creation, scratch allocation, grants or process launch.
    /// Drop must leave this lease for observed-stop recovery, even on unwind.
    pub fn bind_process(&mut self, installation: &str, nonce: &str, boot: &str) -> Result<()> {
        self.bind_runtime_process(installation, nonce, boot, 0)
    }
    pub fn bind_runtime_process(
        &mut self,
        installation: &str,
        nonce: &str,
        boot: &str,
        order: u64,
    ) -> Result<()> {
        ensure!(
            self.journal.metadata()?.len() == 0 && !self.process_bound,
            "runner.lease_already_bound"
        );
        self.record(&Record::ProcessBoundary {
            installation: installation.into(),
            nonce: nonce.into(),
            boot: boot.into(),
            order,
        })?;
        self.process_bound = true;
        Ok(())
    }
    fn record(&mut self, record: &Record) -> Result<()> {
        serde_json::to_writer(&mut self.journal, record)?;
        self.journal.write_all(b"\n")?;
        self.journal.sync_all()?;
        self.records_written = self.records_written.saturating_add(1);
        Ok(())
    }
    /// Read roots are trusted Core scope facts. Never follow a reparse point or
    /// grant an aliased file. Root handles prevent replacing the scope itself.
    ///
    /// Returns what the pass cost, because this walk is the launch path's largest
    /// piece of work and the only honest way to explain a slow launch is to count it.
    ///
    /// `control` is checked inside the walk, not only before it: the traversal is
    /// synchronous and no outside abort can interrupt it, so a launch that is cancelled
    /// or that has spent its preparation budget has to be able to stop *during* the walk
    /// — otherwise the only thing a deadline does is describe how long the work already
    /// took. The signal it polls is the launch's live watch state, never a copy made
    /// when the pass began. The check is cheap (one instant comparison and one relaxed
    /// load) and runs between objects, never in the middle of one, so a stop leaves each
    /// object either untouched or fully captured and applied.
    pub fn grant_tree(
        &mut self,
        root: &ExecutionFileBinding,
        account: &str,
        write_sid: Option<&str>,
        control: &GrantControl,
    ) -> GrantOutcome {
        let records_before = self.records_written;
        let mut work = GrantWork::default();
        let mut failure = None;
        {
            let mut step = || -> Result<()> {
                let root_handle = super::file_binding::verify(root)?;
                ensure!(root_handle.info().is_directory, "runner.scope_not_directory");
                self.record(&Record::Scope {
                    root: root.clone(),
                    account: account.into(),
                    write_sid: write_sid.map(str::to_owned),
                })?;
                self.scopes.push(Scope {
                    root: root.clone(),
                    account: account.into(),
                    write_sid: write_sid.map(str::to_owned),
                    enumerated: false,
                    halted: false,
                });
                self.walk(root, root_handle, account, write_sid, control, &mut work)?;
                Ok(())
            };
            if let Err(source) = step() {
                failure = Some(source);
            }
        }
        // A pass that did not complete proves the launch never dispatched:
        // Execute is sent only after every grant completes. That fact is
        // durable, so both the in-process settle and journal replay can tell
        // "halted before any execution" apart from "crashed or legacy, where
        // execution is unknowable".
        if failure.is_some() || work.stopped.is_some() {
            let reason = match work.stopped {
                Some(stop) => stop.code(),
                None => "failed",
            };
            match self.record(&Record::ScopeHalted { root: root.clone(), reason: reason.into() }) {
                Ok(()) => {
                    if let Some(scope) = self.scopes.last_mut()
                        && scope.root == *root
                    {
                        scope.halted = true;
                    }
                }
                Err(source) => failure = Some(source),
            }
        }
        work.journal_records = self.records_written.saturating_sub(records_before);
        let verdict = match (&failure, work.stopped) {
            (Some(_), _) => GrantVerdict::Failed,
            (None, Some(reason)) => GrantVerdict::Stopped(reason),
            (None, None) => GrantVerdict::Complete,
        };
        GrantOutcome { work, verdict, failure }
    }

    /// The synchronous walk itself. Split out so every way of failing still returns the
    /// counts of what it did before it failed.
    fn walk(
        &mut self,
        root: &ExecutionFileBinding,
        root_handle: aura_windows_platform::VerifiedFile,
        account: &str,
        write_sid: Option<&str>,
        control: &GrantControl,
        work: &mut GrantWork,
    ) -> Result<()> {
        let canonical = root_handle.info().final_path.clone();
        let mut pending = vec![canonical.clone()];
        let mut stopped = None;
        let start = self.held.len();
        while let Some(path) = pending.pop() {
            if let Some(reason) = control.stop_reason() {
                stopped = Some(reason);
                break;
            }
            ensure!(
                self.held.len() + self.skipped.len() < MAX_OBJECTS,
                "runner.scope_object_limit"
            );
            work.objects_scanned = work.objects_scanned.saturating_add(1);
            let info = aura_windows_platform::inspect_existing_path(&path, false)?;
            if info.is_reparse_point || (!info.is_directory && info.number_of_links != 1) {
                // Skipped is not absent: the identity joins the durable
                // seen-set, so a link-count change after the walk can never
                // reclassify this pre-existing object as attempt output.
                let reason = if info.is_reparse_point { "reparse" } else { "links" };
                self.record(&Record::Skipped {
                    path: info.final_path.clone(),
                    volume: info.identity.volume_serial_number,
                    id: info.identity.file_id,
                    reason: reason.into(),
                })?;
                self.skipped.insert((info.identity.volume_serial_number, info.identity.file_id));
                work.objects_skipped = work.objects_skipped.saturating_add(1);
                continue;
            }
            ensure!(
                aura_windows_platform::final_path_within(root_handle.info(), &info),
                "runner.scope_escape"
            );
            let object = if path == canonical {
                if let Some(held) = self.root_handles.get(&path) {
                    held.clone()
                } else {
                    let held = Arc::new(ExecutionFileSecurity::open(&path, true)?);
                    self.root_handles.insert(path.clone(), held.clone());
                    held
                }
            } else {
                Arc::new(ExecutionFileSecurity::open(&path, false)?)
            };
            ensure!(
                object.info().identity == info.identity
                    && aura_windows_platform::final_path_within(root_handle.info(), object.info()),
                "runner.scope_changed"
            );
            let before = object.capture()?;
            let after = object.plan_for_attempt(account, write_sid)?;
            self.record(&Record::Intent {
                path: object.info().final_path.clone(),
                volume: info.identity.volume_serial_number,
                id: info.identity.file_id,
                before: before.clone(),
                after: after.clone(),
            })?;
            // Retain even a partially applied object: Drop retries exact
            // restoration, and an unresolved journal blocks subsequent work.
            self.held.push(Held { object, before, after });
            if info.is_directory {
                work.directories_scanned = work.directories_scanned.saturating_add(1);
                for child in std::fs::read_dir(&path)? {
                    pending.push(child?.path());
                }
            }
        }
        if stopped.is_none() {
            // Only a finished enumeration makes the intent set a complete
            // inventory of the root; that fact is what later separates new
            // output from pre-existing files, so it is durable. An apply that
            // stops or fails afterwards does not revoke it.
            self.record(&Record::ScopeEnumerated { root: root.clone() })?;
            if let Some(scope) = self.scopes.last_mut() {
                scope.enumerated = true;
            }
        }
        // Capture/journal every existing object's original ACL before touching
        // its parent. Temporary protection prevents parent grants from silently
        // changing another object's prepared ACL during the batch. The batch
        // honours the same control as the walk: a stop between applications
        // leaves every object either fully rewritten or not yet touched, and
        // each successful rewrite is counted where it happens — a failure on
        // object nine must not lose the eight that already applied.
        if stopped.is_none() {
            for held in &self.held[start..] {
                if let Some(reason) = control.stop_reason() {
                    stopped = Some(reason);
                    break;
                }
                held.object.apply_exact(&held.before, &held.after)?;
                work.acl_rewrites = work.acl_rewrites.saturating_add(1);
            }
        }
        work.stopped = stopped;
        Ok(())
    }
    pub fn restore(&mut self) -> Result<()> {
        self.restore_existing()?;
        self.finish_restore()
    }
    pub(crate) fn restore_existing(&mut self) -> Result<()> {
        if self.objects_restored || self.restored {
            return Ok(());
        }
        let mut failure = None;
        // Objects this pass proved gone, recorded after the sweep so the fact is
        // durable even when another object's restoration fails: a delete-pending
        // object has no ACL left to restore and no name a later walk could open,
        // and the lease's own handle is the only thing still holding its name.
        let mut gone = Vec::new();
        // Parents first: when child inheritance is restored it sees the
        // original parent, not the still-active temporary grant.
        for held in &self.held {
            let result = (|| {
                if held.object.deleted()? {
                    gone.push(held.object.info().final_path.clone());
                    return Ok(());
                }
                if !ExecutionFileSecurity::sddl_equivalent(&held.object.capture()?, &held.before) {
                    held.object.apply_exact(&held.after, &held.before)?;
                }
                Ok::<_, anyhow::Error>(())
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        }
        for path in gone {
            let info = self
                .held
                .iter()
                .find(|held| same_path(&held.object.info().final_path, &path))
                .map(|held| held.object.info().identity.clone())
                .ok_or_else(|| anyhow::anyhow!("runner.lease_gone_object_missing"))?;
            self.record(&Record::ObjectGone {
                path: path.clone(),
                volume: info.volume_serial_number,
                id: info.file_id,
            })?;
            self.gone.push(path);
        }
        if let Some(error) = failure {
            return Err(error);
        }
        self.record(&Record::ObjectsRestored)?;
        self.objects_restored = true;
        Ok(())
    }
    pub(crate) fn finish_restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        ensure!(self.objects_restored, "runner.lease_objects_not_restored");
        let mut existing = self
            .held
            .iter()
            .map(|v| {
                (v.object.info().identity.volume_serial_number, v.object.info().identity.file_id)
            })
            .collect::<std::collections::BTreeSet<_>>();
        existing.extend(self.skipped.iter().copied());
        retire_new_files(&self.scopes, &existing, &self.gone)?;
        // A scope settles when its walk finished (every object under the root
        // has a durable identity, so what is absent is new output) or when its
        // pass halted before completion (no Execute ever followed it, so there
        // is no output at all). Anything else — a legacy journal or a crash
        // mid-walk — proves nothing: while intents remain outstanding the
        // journal stays for a recovery boundary instead of publishing a
        // cleanup receipt.
        let enumeration_unproven =
            self.scopes.iter().any(|scope| !scope.enumerated && !scope.halted)
                && !existing.is_empty();
        ensure!(!enumeration_unproven, "runner.lease_enumeration_unproven");
        self.record(&Record::Released)?;
        self.restored = true;
        self.held.clear();
        self.root_handles.clear();
        self.ancestor_pins.clear();
        Ok(())
    }
}
impl Drop for FileLease {
    fn drop(&mut self) {
        if !self.process_bound {
            let _ = self.restore();
        }
    }
}

/// Released journals need no process action. An unreleased legacy journal has
/// no stop proof and remains an explicit repair/reconciliation requirement.
///
/// Every fact a recovery pass needs from one journal, from a single read: the
/// captured binding already carries the content hash, the boundary record is
/// validated for position, and the scope roots are collected as they parse.
/// Reading one journal four times (boundary, order, roots, receipt hash) used
/// to cost four full reads and three full parses of every historical journal.
pub(crate) struct JournalScan {
    pub binding: ExecutionFileBinding,
    pub size: u64,
    pub order: u64,
    pub boundary: Option<(String, String, String)>,
    pub roots: Vec<ExecutionFileBinding>,
    pub released: bool,
    pub records: u64,
    /// The volume's change generation for this journal, read through the same
    /// handle that hashed it. `None` when the volume would not describe the file,
    /// which keeps the journal out of any reuse index: a claim nobody can check
    /// later is not worth caching.
    pub change_usn: Option<u64>,
}

pub(crate) fn scan_journal(path: &Path) -> Result<JournalScan> {
    use std::io::{BufRead, BufReader, Seek};
    let (binding, mut retained) = super::file_binding::capture(path, true)?;
    let size = retained.file().metadata()?.len();
    ensure!(size <= 256 * 1024 * 1024, "runner.lease_journal_limit");
    // `capture(contents)` opens the journal with `FILE_SHARE_READ` only, so no
    // writer can hold it while these bytes are read and this generation is taken:
    // the hash and the generation describe one state of one file.
    let change_usn = aura_windows_platform::change_generation(&retained);
    retained.file_mut().rewind()?;
    let mut reader = BufReader::new(retained.file());
    let mut line = String::new();
    let mut boundary = None;
    let mut order = 0;
    let mut roots = Vec::new();
    let mut released = false;
    let mut records = 0_u64;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        // The same guards the separate passes enforced: a bounded line count,
        // and Released only ever as the final record.
        ensure!(records <= MAX_OBJECTS as u64 + 1024 && !released, "runner.lease_journal_invalid");
        let text =
            line.strip_suffix('\n').map(|v| v.strip_suffix('\r').unwrap_or(v)).unwrap_or(&line);
        match serde_json::from_str::<Record>(text)? {
            Record::ProcessBoundary { installation, nonce, boot, order: sequence } => {
                ensure!(records == 0, "runner.lease_process_binding_order");
                boundary = Some((installation, nonce, boot));
                order = sequence;
            }
            Record::Scope { root, .. } => roots.push(root),
            Record::Released => released = true,
            _ => {}
        }
        records += 1;
    }
    Ok(JournalScan { binding, size, order, boundary, roots, released, records, change_usn })
}

/// Recovery never dispatches a command. Missing or moved objects require
/// explicit reconciliation; they are not reopened by name and overwritten.
pub fn recover(path: &Path) -> Result<()> {
    recover_all(&[path.to_owned()])
}

/// Restore every old parent ACL first. Retiring inherited grants earlier would
/// re-inherit an older live lease. Durable phase markers make crash resumption
/// safe after any earlier layer has already reached its original descriptor.
pub(crate) fn recover_all(paths: &[PathBuf]) -> Result<()> {
    let pending = paths.iter().map(|p| recover_existing(p)).collect::<Result<Vec<_>>>()?;
    for value in pending.into_iter().flatten() {
        // A scope settles when its walk finished (its intent set is a complete
        // inventory, so what is absent is new output) or when its pass halted
        // before completion (the launch dispatches only after every grant
        // completes, so no Execute ever followed it and there is no output at
        // all). A scope with intents but neither fact — a legacy journal or a
        // crash mid-walk — proves nothing about its remaining objects: the
        // intents are already restored, and the journal stays unreleased for
        // explicit reconciliation instead of publishing a cleanup receipt. A
        // journal without scopes (an ancestor-metadata lease) has nothing to
        // retire and settles normally.
        let enumeration_unproven =
            value.scopes.iter().any(|scope| !scope.enumerated && !scope.halted)
                && !value.existing.is_empty();
        retire_new_files(&value.scopes, &value.existing, &value.gone)?;
        ensure!(!enumeration_unproven, "runner.lease_enumeration_unproven");
        let mut journal = std::fs::OpenOptions::new().append(true).open(value.path)?;
        serde_json::to_writer(&mut journal, &Record::Released)?;
        journal.write_all(b"\n")?;
        journal.sync_all()?;
    }
    Ok(())
}
struct PendingRecovery {
    path: PathBuf,
    scopes: Vec<Scope>,
    existing: std::collections::BTreeSet<(u64, [u8; 16])>,
    /// Paths this pass proved gone, so the retirement walk matches the names it
    /// cannot reopen instead of failing on them.
    gone: Vec<PathBuf>,
}

/// Where one recorded intent's object was found.
enum IntentObject {
    /// The object itself, opened under the recorded identity — at its recorded
    /// path, or wherever the volume says that identity lives now.
    Object(ExecutionFileSecurity),
    /// The volume answered that it has no object with this identity.
    Gone,
}

/// Find the object one intent wrote about, without ever touching a replacement.
///
/// The recorded path is tried first, and accepted only when the object behind it
/// still carries the recorded identity: a same-named replacement is somebody
/// else's object and never receives the recorded ACL. When the path does not
/// resolve — deleted, renamed, or held in a state that refuses to open — the
/// volume is asked for the identity itself. Only an answer of "no such object"
/// settles the intent as gone; a refusal to answer leaves it for the next
/// recovery boundary, because an access denial is also what a delete-pending
/// object produces.
fn resolve_intent_object(path: &Path, volume: u64, id: [u8; 16]) -> Result<IntentObject> {
    match ExecutionFileSecurity::open(path, false) {
        Ok(object) => {
            if (object.info().identity.volume_serial_number, object.info().identity.file_id)
                == (volume, id)
            {
                return Ok(IntentObject::Object(object));
            }
            // A replacement owns the recorded name now. It is not this intent's
            // object, so its ACL is not ours to write; the identity lookup below
            // still has to account for the object that was recorded.
        }
        Err(_) => {
            // Deleted, renamed, delete-pending, or unreadable: the identity
            // lookup is the only thing that can tell those apart.
        }
    }
    match aura_windows_platform::resolve_file_identity(path, volume, id)? {
        aura_windows_platform::IdentityLookup::Found(file) => {
            Ok(IntentObject::Object(ExecutionFileSecurity::from_verified(*file)?))
        }
        aura_windows_platform::IdentityLookup::Absent => Ok(IntentObject::Gone),
        aura_windows_platform::IdentityLookup::Unresolved => {
            Err(anyhow::anyhow!("runner.lease_recovery_object_unresolved"))
        }
    }
}

fn recover_existing(path: &Path) -> Result<Option<PendingRecovery>> {
    use std::io::{BufRead, BufReader, Seek};
    let (_, mut retained) = super::file_binding::capture(path, true)?;
    ensure!(retained.file().metadata()?.len() <= 256 * 1024 * 1024, "runner.lease_journal_limit");
    retained.file_mut().rewind()?;
    let records = BufReader::new(retained.file())
        .lines()
        .map(|line| Ok::<Record, anyhow::Error>(serde_json::from_str(&line?)?))
        .collect::<Result<Vec<_>>>()?;
    if matches!(records.last(), Some(Record::Released)) {
        return Ok(None);
    }
    ensure!(
        records.len() <= MAX_OBJECTS + 1024
            && records.iter().filter(|r| matches!(r, Record::Intent { .. })).count() <= MAX_OBJECTS
            && !records.iter().any(|r| matches!(r, Record::Released)),
        "runner.lease_journal_invalid"
    );
    let mut scopes = Vec::new();
    for record in &records {
        match record {
            Record::Scope { root, account, write_sid } => scopes.push(Scope {
                root: root.clone(),
                account: account.clone(),
                write_sid: write_sid.clone(),
                enumerated: false,
                halted: false,
            }),
            Record::ScopeEnumerated { root } => {
                for scope in scopes.iter_mut().filter(|scope| scope.root == *root) {
                    scope.enumerated = true;
                }
            }
            Record::ScopeHalted { root, .. } => {
                for scope in scopes.iter_mut().filter(|scope| scope.root == *root) {
                    scope.halted = true;
                }
            }
            _ => {}
        }
    }
    let existing = records
        .iter()
        .filter_map(|r| match r {
            Record::Intent { volume, id, .. } | Record::Skipped { volume, id, .. } => {
                Some((*volume, *id))
            }
            _ => None,
        })
        .collect();
    // Identities an earlier pass — this process or a previous one — already
    // proved gone. They stay part of the intent set (their paths must not be
    // retired again) and are skipped instead of resolved a second time.
    let mut gone_identities = records
        .iter()
        .filter_map(|r| match r {
            Record::ObjectGone { volume, id, .. } => Some((*volume, *id)),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut gone = records
        .iter()
        .filter_map(|r| match r {
            Record::ObjectGone { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let objects_restored = records.iter().any(|r| matches!(r, Record::ObjectsRestored));
    let mut settled = Vec::new();
    if !objects_restored {
        for record in records.iter() {
            let Record::Intent { path, volume, id, before, after } = record else {
                continue;
            };
            if gone_identities.contains(&(*volume, *id)) {
                continue;
            }
            match resolve_intent_object(path, *volume, *id)? {
                IntentObject::Object(object) => {
                    if !ExecutionFileSecurity::sddl_equivalent(&object.capture()?, before) {
                        object.apply_exact(after, before)?;
                    }
                }
                IntentObject::Gone => {
                    gone_identities.insert((*volume, *id));
                    gone.push(path.clone());
                    settled.push(Record::ObjectGone {
                        path: path.clone(),
                        volume: *volume,
                        id: *id,
                    });
                }
            }
        }
    }
    drop(retained);
    // Each vanished object is written as its own durable record before the pass
    // marker, so a crash here re-enters with exactly these identities already
    // accounted for and restores nothing twice.
    if !settled.is_empty() || !objects_restored {
        let mut journal = std::fs::OpenOptions::new().append(true).open(path)?;
        for record in settled.iter().chain((!objects_restored).then_some(&Record::ObjectsRestored))
        {
            serde_json::to_writer(&mut journal, record)?;
            journal.write_all(b"\n")?;
        }
        journal.sync_all()?;
    }
    Ok(Some(PendingRecovery { path: path.into(), scopes, existing, gone }))
}

fn is_ephemeral_scratch(root: &ExecutionFileBinding) -> bool {
    Path::new(&root.path)
        .parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name == "execution-scratch")
}

/// Retire attempt authority from objects the scope holds but the walk never saw.
///
/// The walk meets a path by name, which a deleted object no longer has in a
/// usable state: Windows keeps a delete-pending name visible while any handle —
/// the lease's own retained handle in particular — is still open, and refuses to
/// open it. Two cases are accounted for instead of failing:
///
/// * the lease proved this exact path gone through its own handle
///   ([`Record::ObjectGone`]), so there is no object left to retire and the name
///   must not be reopened;
/// * the name is simply no longer there, which is what the walk sees once the
///   handles are gone — nothing can carry a grant that does not exist.
///
/// Every other failure still stops the pass: an access denial the lease cannot
/// explain, a sharing conflict, an alias, or a path that escapes the scope are
/// all things that could hide a live grant.
fn retire_new_files(
    scopes: &[Scope],
    existing: &std::collections::BTreeSet<(u64, [u8; 16])>,
    gone: &[PathBuf],
) -> Result<()> {
    for scope in scopes {
        // A scope whose enumeration never finished proves nothing about the
        // objects it never captured: those are pre-existing files, not new
        // output, and their ACLs — including any earlier account grant — are
        // not this lease's to rewrite.
        if !scope.enumerated {
            continue;
        }
        let Some(write_sid) = &scope.write_sid else {
            continue;
        };
        // Scratch is deleted after restore. PowerShell profile/cache files owned
        // by the dedicated account must not fail the workspace ACL retirement.
        if is_ephemeral_scratch(&scope.root) {
            continue;
        }
        let root = super::file_binding::verify(&scope.root)?;
        let mut pending = vec![root.info().final_path.clone()];
        let mut count = 0;
        while let Some(path) = pending.pop() {
            count += 1;
            ensure!(count <= MAX_OBJECTS, "runner.created_file_limit");
            let info = match aura_windows_platform::inspect_existing_path(&path, false) {
                Ok(info) => info,
                Err(_error) if path_is_gone(gone, &path) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if info.is_reparse_point {
                continue;
            }
            ensure!(
                aura_windows_platform::final_path_within(root.info(), &info),
                "runner.created_file_escape"
            );
            if !existing.contains(&(info.identity.volume_serial_number, info.identity.file_id)) {
                let object = ExecutionFileSecurity::open(&path, false)?;
                ensure!(object.info().identity == info.identity, "runner.created_file_changed");
                object.retire_created_file(&scope.account, write_sid)?;
            }
            if info.is_directory {
                for child in std::fs::read_dir(path)? {
                    pending.push(child?.path());
                }
            }
        }
    }
    Ok(())
}
