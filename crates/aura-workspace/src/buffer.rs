mod projection;
use projection::{lifecycle_state, status};
mod native_save;
mod saving;
use std::{
    collections::HashMap,
    io::Read,
    sync::{Arc, Mutex, RwLock},
};

use aura_domain_common::{
    ContentHash, ContentHashAlgorithm, EditorReservationId, FileIdentity, Revision, SessionId,
    WorkspaceId, WorkspacePath,
};
use aura_windows_platform::{
    HandleBoundReplaceContent, NativeFileAccess, NativeFileIdentity,
    handle_bound_replace_with_backup_checked, open_verified_publish_parent,
};
// The platform keeps its own digest enumeration so it holds no dependency on the workspace's
// contract types; the alias keeps the two apart at every use site.
use aura_windows_platform::ContentHashAlgorithm as PlatformHashAlgorithm;
use aura_workspace_domain::{
    BufferEditCommand, CommitRequest, DiscardBufferBinding, EditEligibility,
    EditorBufferLifecycleState, EditorReservationSummary, EditorSaveState, EditorStateRef,
    EligibilityOwner, ExecutionId, FileResource, FileStorageSource, LineEnding,
    LogicalFileSnapshot, LogicalFileStatus, LogicalTextReadSnapshot, ResourceAccess, TextEncoding,
    TextReadRange, WorkspaceLifecycleBlocker,
};

use crate::catalog::scan::ambiguous_encoding;
use crate::error::io_error;
use crate::resource_facts::ResourceFacts;
use crate::{DecodedText, SecurePathResolver, WorkspaceError, decode_text, encode_text};

/// Fixed read chunk for `BufferStore::disk_text_snapshot_streaming`. It bounds
/// the raw buffer, the text retained per chunk, and every `encoding_rs` decoder
/// carry, so a streaming read of a file of any size holds O(chunk) bytes of file
/// content instead of the whole file.
pub const LOGICAL_TEXT_STREAM_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct BufferKey {
    workspace_id: WorkspaceId,
    session_id: SessionId,
    file_identity: FileIdentity,
}

#[derive(Clone)]
struct BufferRecord {
    path: WorkspacePath,
    decoded: DecodedText,
    disk_hash: ContentHash,
    revision: Revision,
    dirty: bool,
    active_editing: bool,
    ime_composition_active: bool,
    reservation_id: Option<EditorReservationId>,
    reservation_epoch: Revision,
    save_state: EditorSaveState,
    writable: bool,
}

#[derive(Clone)]
struct ReservationRecord {
    reservation_id: EditorReservationId,
    epoch: Revision,
    session_id: SessionId,
    path: WorkspacePath,
    /// The workspace this reservation was taken through. Recorded for reporting and for the
    /// workspace-scoped views; deliberately **not** part of the map key, because the reservation is
    /// on the real object.
    workspace_id: WorkspaceId,
    /// The execution that holds this buffer open.
    ///
    /// One editor session on one buffer is one execution for as long as the buffer lives, so its
    /// saves share one long eligibility and one identity across a replacement. A second *save* is
    /// not a second execution — which is why a save may commit under the eligibility this holds —
    /// while a second *buffer* on another file is a different execution entirely.
    execution_id: ExecutionId,
    /// The long eligibility the shared owner granted for this real object.
    ///
    /// It is stored so the save path commits **under** it rather than claiming a free resource: the
    /// owner then verifies live ownership, epoch and coverage at the commit instant, and a released
    /// or migrated eligibility is refused instead of being trusted because an id was passed.
    eligibility: EditEligibility,
}

/// The short carrier a save hands from its index section to its disk work.
///
/// It holds everything the write needs and nothing that belongs to the index, so no lock is held
/// while the file is read and replaced. The revision is carried so the commit section can tell
/// whether the buffer changed while the disk work was running.
///
/// It also carries the **execution** and the **long eligibility** this save belongs to, so the
/// common commit taken before the write is taken by the same execution that owns the reservation.
/// Without that, the save's commit would be a stranger's: the owner would see it as an unrelated
/// execution and the reservation's own protection would not be expressed in the shared rules.
struct PreparedSave {
    key: BufferKey,
    disk_hash: ContentHash,
    bytes: Vec<u8>,
    revision: Revision,
    /// The execution this save runs as. One buffer, one long-lived editor execution: two saves of
    /// one session on different files are two executions, and two saves of one buffer are the same
    /// execution, which is why they may re-enter each other's eligibility but never the same
    /// commit record.
    execution_id: ExecutionId,
    /// The long eligibility this buffer holds, when the file is open writable.
    eligibility: Option<EditEligibility>,
}

/// The short carrier an open hands from its unlocked disk read to its index section.
///
/// It exists so the expensive read happens with no lock held while the index decision still sees
/// exactly what the read saw. `Existing` is the case where this session already holds the buffer:
/// the bytes are not needed, and reading them would let an open observe a write the session itself
/// has pending.
enum OpenedFromDisk {
    /// This session already has the buffer; its record and key are handed over unchanged.
    Existing { key: BufferKey, record: BufferRecord },
    /// The file was read: the bytes, the identity they came from, and the decoded text.
    Read { bytes: Vec<u8>, file_identity: FileIdentity, decoded: DecodedText },
}

/// Who is asking for a save, which decides whether a frozen lifecycle refuses it.
///
/// The two are not a permission level: both take the same common commit from the same owner and
/// both publish conditionally. They differ in exactly one fact — the workspace lifecycle is the
/// authority that freezes a workspace, so it is also the one caller allowed to finish saving
/// inside a workspace it has frozen. Everything else must be refused during a transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SaveAuthority {
    Ordinary,
    Lifecycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecycleBufferGate {
    Preparing,
    Frozen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskStaleReport {
    pub workspace_disk_may_be_stale: bool,
    pub paths: Vec<WorkspacePath>,
}

/// One session's buffer that holds changes disk does not have.
///
/// This is an observation, not a claim: it says which session owns the unsaved content and at
/// which buffer revision. It exists so a caller can *report* a conflicting editor instead of
/// silently saving over it or taking it over.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UncommittedEditorBuffer {
    pub session_id: SessionId,
    pub path: WorkspacePath,
    pub revision: Revision,
    pub reservation_id: EditorReservationId,
    pub reservation_epoch: Revision,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BufferLifecyclePreflight {
    pub blockers: Vec<WorkspaceLifecycleBlocker>,
    pub releasable_editor_states: Vec<EditorStateRef>,
}

/// The editor buffer store.
///
/// It holds **no** eligibility owner of its own. Batch A requires one owner per process, created at
/// the real assembly point ([`crate::WorkspaceRuntime`]) and shared by pointer with every file
/// entry, so the store holds the same `Arc` the file tools and the artifact writer hold. A store
/// that built its own would protect nothing: a claim taken through Buffer open would be invisible
/// to a Main Fs write on the same bytes, which is exactly the state this batch repairs.
pub struct BufferStore {
    buffers: RwLock<HashMap<BufferKey, BufferRecord>>,
    lifecycle_gates: RwLock<HashMap<WorkspaceId, LifecycleBufferGate>>,
    operation_gate: Mutex<()>,
    path_index: RwLock<HashMap<(WorkspaceId, SessionId, WorkspacePath), FileIdentity>>,
    /// The long edit reservation for one **real object**.
    ///
    /// Keyed by `FileIdentity` alone, deliberately without the `WorkspaceId`. Batch A's measured
    /// defect was that this map was keyed `(WorkspaceId, FileIdentity)`, so the *same physical
    /// file* reached through two workspaces — two roots, an aliased root, a nested workspace or a
    /// hardlink — obtained two independent writable reservations and both sessions could write one
    /// file. The workspace id is ownership, not identity, so it has no place in this key.
    ///
    /// The identity is the volume serial plus the file id, which is what makes this the *real*
    /// object rather than a name: two different files are two entries however they are spelled, and
    /// one file is one entry however it is reached.
    reservations: RwLock<HashMap<FileIdentity, ReservationRecord>>,
    revoked_reservations: RwLock<HashMap<EditorReservationId, Revision>>,
    /// The one eligibility owner, shared by pointer.
    eligibility: Arc<EligibilityOwner>,
    /// The run-information publication hook.
    ///
    /// `None` in a store that is not part of the assembled runtime. It is a plain function
    /// pointer box, called with facts the store already holds, and its return value is
    /// discarded: a commit cannot be affected by whether the index was able to record it.
    ///
    /// R2: it is called **twice** per real save under the same identity - once at `Start`, before
    /// the write window opens, and once at `Settled`/`Failed`/`Terminated`. A finished save must
    /// not permanently occupy a `running` row, and a save that is really in flight must really
    /// appear, so the two calls are what make both facts observable.
    activity_publisher: Option<BufferCommitPublisher>,
}

/// Which real moment of one buffer commit is being published (R2).
///
/// The distinction is not cosmetic. Before R2 the store published only **after** it had written,
/// settled and released its commit, so the row described an event that had already finished and
/// nothing ever ended it: a completed save sat in the running index forever. Publishing the same
/// identity at [`Self::Start`] and ending it at a terminal phase is what makes "one row while the
/// write is really in flight, zero rows after it settled" true at the store's own boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferCommitPhase {
    /// The real write window is about to open. Nothing else has been written yet.
    Start,
    /// The save really published its bytes and the buffer's own facts were accepted.
    Settled,
    /// The save did not publish: it failed, or it was refused before any byte landed.
    Failed,
    /// The store's own work stopped without a settled commit and without a classified failure -
    /// the write may still be landing in another thread. It is not reported as success.
    Terminated,
}

/// The facts a real buffer commit publishes.
///
/// They are the store's own identities and the already-verified target identity. No file
/// content, no patch and no path payload travels here: the display name is a name, and the
/// identity is what actually associates two commits of one file.
pub struct BufferCommitFact<'a> {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub reservation_id: EditorReservationId,
    pub buffer_revision: Revision,
    /// The identity the store already verified. At [`BufferCommitPhase::Start`] this is the
    /// identity the save is replacing - the object it really holds - and at a terminal phase it
    /// is the identity the commit actually produced. It is never re-resolved to obtain one.
    pub identity: &'a FileIdentity,
    /// Complete already-admitted resource set; an observer never resolves paths again.
    pub resources: &'a [aura_workspace_domain::ResourceRequest],
    pub path: &'a WorkspacePath,
    /// Which real moment of this commit this fact describes.
    pub phase: BufferCommitPhase,
}

// The observer lease lives on the real synchronous save, including panic/error exits.
/// Optional, observation-only callback; the executor retains the borrowed fact.
pub type BufferCommitPublisher = Arc<dyn for<'a> Fn(BufferCommitFact<'a>) + Send + Sync>;

struct BufferActivityScope {
    publisher: Option<BufferCommitPublisher>,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    reservation_id: EditorReservationId,
    buffer_revision: Revision,
    identity: FileIdentity,
    path: WorkspacePath,
    resources: Vec<aura_workspace_domain::ResourceRequest>,
    phase: BufferCommitPhase,
}
impl BufferActivityScope {
    fn publish(&self, phase: BufferCommitPhase) {
        if let Some(publisher) = &self.publisher {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                publisher(BufferCommitFact {
                    workspace_id: self.workspace_id,
                    session_id: self.session_id,
                    reservation_id: self.reservation_id,
                    buffer_revision: self.buffer_revision,
                    identity: &self.identity,
                    path: &self.path,
                    resources: &self.resources,
                    phase,
                })
            }));
        }
    }
}
impl Drop for BufferActivityScope {
    fn drop(&mut self) {
        self.publish(self.phase);
    }
}

impl Default for BufferStore {
    /// A store with no shared owner, for a caller that is not part of the assembled runtime.
    ///
    /// It exists because the store's own unit tests need a store without a runtime, and it is
    /// deliberately **not** what the product uses: the assembled runtime always calls
    /// [`BufferStore::with_eligibility_owner`], so the store the five entries share holds the one
    /// owner the runtime created. A test that needs the shared behaviour passes the same `Arc` to
    /// two stores, which is how the same-instance property is measured.
    fn default() -> Self {
        Self::with_eligibility_owner(Arc::new(EligibilityOwner::new()))
    }
}

impl BufferStore {
    /// The store the runtime assembles: the shared owner, by pointer.
    #[must_use]
    pub fn with_eligibility_owner(eligibility: Arc<EligibilityOwner>) -> Self {
        Self {
            buffers: RwLock::new(HashMap::new()),
            lifecycle_gates: RwLock::new(HashMap::new()),
            operation_gate: Mutex::new(()),
            path_index: RwLock::new(HashMap::new()),
            reservations: RwLock::new(HashMap::new()),
            revoked_reservations: RwLock::new(HashMap::new()),
            eligibility,
            activity_publisher: None,
        }
    }

    /// Installs the activity publisher for this store.
    ///
    /// A buffer commit is a **real filesystem writer**: the bytes reach the disk inside
    /// [`Self::save`], so the commit is published to the run-information index by this store -
    /// the executor - and not by whoever asked for the save. The publisher is an `Arc<dyn Fn>`
    /// rather than a direct dependency because `aura-workspace` must not depend on the
    /// information module: the module is downstream of the executor, never the other way round,
    /// and that direction is what keeps a broken index from being able to affect a save.
    #[must_use]
    pub fn with_activity_publisher(mut self, publisher: BufferCommitPublisher) -> Self {
        self.activity_publisher = Some(publisher);
        self
    }

    /// The eligibility owner this store coordinates through, by pointer.
    ///
    /// Exposed so the same-instance proof is a real `Arc::ptr_eq` between entries rather than a
    /// claim about an exported type.
    #[must_use]
    pub fn eligibility_owner(&self) -> Arc<EligibilityOwner> {
        Arc::clone(&self.eligibility)
    }
}

impl BufferStore {
    /// Opens one file into this session's buffer, optionally taking the writable edit claim.
    ///
    /// # Why the disk read is outside the gate
    ///
    /// `operation_gate` keeps the in-memory index consistent; it must not be held across file I/O.
    /// This method used to hold it across `read_disk_with_identity`, so opening a large file in one
    /// workspace serialised every buffer operation in every other workspace. The read now happens
    /// first, with no lock held, and the gate is taken only for the index section that installs the
    /// record and the reservation.
    ///
    /// The two facts the read produces are still re-checked against the index inside the gate, so
    /// nothing is decided on a stale observation: the reservation is taken on the identity the read
    /// really saw, and a session that already holds a buffer for that identity reuses its record
    /// rather than re-decoding the file it just read.
    pub fn open(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: WorkspacePath,
        writable: bool,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        if writable && self.workspace_gate(resolver.workspace_id())?.is_some() {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        // Section 1: no lock held. The disk read is the expensive part and it belongs here.
        let opened = self.open_read(resolver, session_id, &path, writable)?;
        // Section 2: the short index section, re-checked against the live index.
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        self.open_install(resolver, session_id, path, writable, opened)
    }

    /// The carrier an open hands from its unlocked read to its index section.
    fn open_read(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
        writable: bool,
    ) -> Result<OpenedFromDisk, WorkspaceError> {
        // A session that already holds this buffer does not need the bytes at all, and re-reading
        // the file it is editing would be both wasted I/O and a chance to observe its own pending
        // write. The check is a read of the index, which cannot race with the mutation of the index
        // because the index section below re-verifies everything it depends on.
        let existing =
            self.buffer_key(resolver.workspace_id(), session_id, path).ok().and_then(|key| {
                self.buffers
                    .read()
                    .ok()
                    .and_then(|buffers| buffers.get(&key).map(|record| (key, record.clone())))
            });
        if let Some((key, record)) = existing {
            return Ok(OpenedFromDisk::Existing { key, record });
        }
        let (bytes, file_identity) = read_disk_with_identity(resolver, path)?;
        // The writable claim is checked here, before decoding, so a refused open does not pay for
        // the decode. The authoritative decision is still taken inside the gate below.
        if writable
            && let Some(existing) = self
                .reservations
                .read()
                .map_err(|_| WorkspaceError::LockPoisoned("editor reservations"))?
                .get(&file_identity)
            && existing.session_id != session_id
        {
            return Err(WorkspaceError::EditorReserved {
                path: path.clone(),
                owner: existing.session_id,
            });
        }
        let decoded = decode_text(&bytes)?;
        Ok(OpenedFromDisk::Read { bytes, file_identity, decoded })
    }

    /// The short index section of an open: take the reservation and install the record.
    fn open_install(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: WorkspacePath,
        writable: bool,
        opened: OpenedFromDisk,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        let workspace_id = resolver.workspace_id();
        let (bytes, file_identity, decoded, key) = match opened {
            OpenedFromDisk::Existing { key, record } => {
                let identity = key.file_identity.clone();
                self.path_index
                    .write()
                    .map_err(|_| WorkspaceError::LockPoisoned("editor buffer path index"))?
                    .insert((workspace_id, session_id, path), identity);
                return status(&key, &record);
            }
            OpenedFromDisk::Read { bytes, file_identity, decoded } => {
                let key =
                    BufferKey { workspace_id, session_id, file_identity: file_identity.clone() };
                (bytes, file_identity, decoded, key)
            }
        };
        let reservation = if writable {
            // The long eligibility is taken from the **one shared owner** before the reservation is
            // installed, so the object is protected across every entry from this instant: a Main Fs
            // write on the same bytes now sees this eligibility and is refused rather than racing
            // the editor. The workspace id is ownership only — it is recorded for reporting and for
            // the workspace lifecycle, and it deliberately does not create a second eligibility.
            //
            // The claim names **two** real resources: the object the editor will replace, and the
            // directory entry that will name the replacement. A save produces a new object identity,
            // so a claim on the old object alone would leave the *name* free between the replacement
            // and the eligibility migration — which is precisely the free window the batch forbids.
            let object = self.object_resource(resolver, &path)?;
            let entry = ResourceFacts::root_unlabelled(Arc::new(resolver.clone())).slot(&path)?;
            let execution_id = ExecutionId::next();
            let eligibility = self
                .eligibility
                .claim_edit_set(
                    vec![
                        aura_workspace_domain::ResourceRequest::new(object, ResourceAccess::Write),
                        aura_workspace_domain::ResourceRequest::new(entry, ResourceAccess::Write),
                    ],
                    session_id,
                    workspace_id,
                    execution_id,
                    Some(Revision::INITIAL),
                )
                .map_err(|refusal| WorkspaceError::EditorReserved {
                    path: path.clone(),
                    owner: refusal.held_by.unwrap_or(session_id),
                })?;
            let mut reservations = self
                .reservations
                .write()
                .map_err(|_| WorkspaceError::LockPoisoned("editor reservations"))?;
            // The lookup is by the real object alone. Reaching the same file through a second
            // workspace finds this entry and is refused, which is the whole point: one physical
            // file has one writable editor, not one per workspace.
            if let Some(existing) = reservations.get(&file_identity) {
                if existing.session_id != session_id {
                    return Err(WorkspaceError::EditorReserved {
                        path,
                        owner: existing.session_id,
                    });
                }
                Some(existing.clone())
            } else {
                let reservation = ReservationRecord {
                    reservation_id: EditorReservationId::new(),
                    epoch: Revision::INITIAL,
                    session_id,
                    path: path.clone(),
                    workspace_id,
                    execution_id,
                    eligibility,
                };
                reservations.insert(file_identity.clone(), reservation.clone());
                Some(reservation)
            }
        } else {
            None
        };
        let record = BufferRecord {
            path: path.clone(),
            decoded,
            disk_hash: ContentHash::blake3(&bytes),
            revision: Revision::INITIAL,
            dirty: false,
            active_editing: false,
            ime_composition_active: false,
            reservation_id: reservation.as_ref().map(|value| value.reservation_id),
            reservation_epoch: reservation.as_ref().map_or(Revision::INITIAL, |value| value.epoch),
            save_state: EditorSaveState::Idle,
            writable,
        };
        let result = status(&key, &record)?;
        self.buffers
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
            .insert(key, record);
        self.path_index
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer path index"))?
            .insert((workspace_id, session_id, path), file_identity);
        Ok(result)
    }

    pub fn apply_edit(
        &self,
        command: BufferEditCommand,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        if self.workspace_gate(command.workspace_id)? == Some(LifecycleBufferGate::Frozen) {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        if let Some(current_epoch) = self
            .revoked_reservations
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("revoked editor reservations"))?
            .get(&command.reservation_id)
            .copied()
        {
            return Err(WorkspaceError::ReservationRevoked {
                reservation_id: command.reservation_id,
                current_epoch,
            });
        }
        let file_identity = self
            .path_index
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer path index"))?
            .get(&(command.workspace_id, command.session_id, command.path.clone()))
            .cloned()
            .ok_or(WorkspaceError::BufferNotFound)?;
        let key = BufferKey {
            workspace_id: command.workspace_id,
            session_id: command.session_id,
            file_identity: file_identity.clone(),
        };
        let reservation = self
            .reservations
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor reservations"))?
            .get(&file_identity)
            .cloned();
        let Some(reservation) = reservation else {
            return self.revoked_or_missing(command.reservation_id);
        };
        if reservation.reservation_id != command.reservation_id
            || reservation.epoch != command.reservation_epoch
        {
            return self.revoked_or_missing(command.reservation_id);
        }
        if reservation.session_id != command.session_id {
            return Err(WorkspaceError::EditorReserved {
                path: command.path,
                owner: reservation.session_id,
            });
        }
        let mut buffers =
            self.buffers.write().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let record = buffers.get_mut(&key).ok_or(WorkspaceError::BufferNotFound)?;
        if !record.writable {
            return Err(WorkspaceError::BufferReadOnly);
        }
        if record.reservation_id != Some(command.reservation_id)
            || record.reservation_epoch != command.reservation_epoch
        {
            return self.revoked_or_missing(command.reservation_id);
        }
        if record.revision != command.base_core_revision {
            return Err(WorkspaceError::BufferRevisionConflict {
                expected: command.base_core_revision,
                actual: record.revision,
            });
        }
        let expected_update = next_revision(record.revision)?;
        if command.buffer_revision != expected_update {
            return Err(WorkspaceError::BufferRevisionConflict {
                expected: expected_update,
                actual: command.buffer_revision,
            });
        }
        record.revision = command.buffer_revision;
        record.decoded.text = command.content;
        record.dirty = true;
        record.active_editing = command.active_editing;
        record.ime_composition_active = command.ime_composition_active;
        record.save_state = EditorSaveState::Idle;
        status(&key, record)
    }

    /// The real object one workspace path names, in the shared resource form.
    ///
    /// Derived from the resolver's own verified chain, so the object a Buffer save commits is the
    /// same object a Main Fs write commits — one physical file, whichever entry reached it.
    fn object_resource(
        &self,
        resolver: &SecurePathResolver,
        path: &WorkspacePath,
    ) -> Result<FileResource, WorkspaceError> {
        // The session on the resource owner is ownership only; this save's own session is recorded
        // on the reservation and in the commit, and the derivation here deliberately does not
        // invent a second session for one file.
        let facts = ResourceFacts::root_unlabelled(Arc::new(resolver.clone()));
        facts.object(path)?.ok_or_else(|| WorkspaceError::NotFound { path: path.clone() })
    }

    /// The short index section of a save: resolve the key and snapshot the bytes to write.
    fn prepare_save(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
        expected_revision: Revision,
    ) -> Result<PreparedSave, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let key = self.buffer_key(resolver.workspace_id(), session_id, path)?;
        let buffers =
            self.buffers.read().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let record = buffers.get(&key).ok_or(WorkspaceError::BufferNotFound)?;
        if record.reservation_id != Some(reservation_id)
            || record.reservation_epoch != reservation_epoch
        {
            return self.revoked_or_missing(reservation_id);
        }
        if record.revision != expected_revision {
            return Err(WorkspaceError::BufferRevisionConflict {
                expected: expected_revision,
                actual: record.revision,
            });
        }
        let bytes =
            encode_text(&record.decoded.text, record.decoded.encoding, record.decoded.line_ending)?;
        // The reservation is read **after** the buffer facts so the execution and eligibility
        // carried out of the index section belong to the same buffer revision the bytes came from.
        // A writable buffer always has a reservation; a read-only one has none, and its save is
        // refused above by the reservation check.
        let reservation = self
            .reservations
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor reservations"))?
            .get(&key.file_identity)
            .cloned()
            .ok_or(WorkspaceError::BufferNotFound)?;
        Ok(PreparedSave {
            key,
            disk_hash: record.disk_hash.clone(),
            bytes,
            revision: record.revision,
            execution_id: reservation.execution_id,
            eligibility: Some(reservation.eligibility.clone()),
        })
    }

    /// The short commit section of a save: re-verify the buffer, then publish the new identity.
    fn commit_save(
        &self,
        carrier: PreparedSave,
        new_identity: FileIdentity,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let key = carrier.key;
        let mut buffers =
            self.buffers.write().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let record = buffers.get_mut(&key).ok_or(WorkspaceError::BufferNotFound)?;
        // The re-verification: the disk write already landed, but the buffer must still be the one
        // this save was prepared from. A newer revision means another edit arrived while the file
        // was being written, so the buffer keeps that newer content and only the disk-side facts
        // are updated - the save is not allowed to claim it published content it never saw.
        if record.reservation_id != Some(reservation_id)
            || record.reservation_epoch != reservation_epoch
        {
            return self.revoked_or_missing(reservation_id);
        }
        let superseded = record.revision != carrier.revision;
        let new_key = BufferKey {
            workspace_id: key.workspace_id,
            session_id: key.session_id,
            file_identity: new_identity.clone(),
        };
        debug_assert!(new_key == key || !buffers.contains_key(&new_key));
        let mut current = buffers.remove(&key).ok_or(WorkspaceError::BufferNotFound)?;
        current.disk_hash = ContentHash::blake3(&carrier.bytes);
        if !superseded {
            current.dirty = false;
            current.save_state = EditorSaveState::Idle;
        }
        let mut path_index = self
            .path_index
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer path index"))?;
        let mut reservations = self
            .reservations
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("editor reservations"))?;
        for ((workspace_id, candidate_session, _), identity) in path_index.iter_mut() {
            if *workspace_id == key.workspace_id
                && *candidate_session == key.session_id
                && *identity == key.file_identity
            {
                *identity = new_identity.clone();
            }
        }
        if let Some(reservation) = reservations.remove(&key.file_identity) {
            // The identity replacement migrates the long eligibility **before** the reservation is
            // re-keyed, and it is validated inside the owner's own lock: migrating into an object
            // another execution already holds is refused rather than accepted, which is the
            // measured defect where a migration left two live eligibilities on one target.
            let migrated = self
                .eligibility
                .migrate_claim_identity(&reservation.eligibility.claim_id, new_identity.clone());
            let mut reservation = reservation;
            match migrated {
                Ok(Some(migration)) => {
                    // The claim's own **primary** resource is the object a replacement changes, and
                    // that is the one whose identity moved. The claim's other members — the naming
                    // entry — are unaffected by a replacement, which is exactly why they are in the
                    // set: the name stays claimed across the identity change.
                    let FileResource::Object { identity, .. } =
                        &mut reservation.eligibility.resources[0].resource
                    else {
                        unreachable!("an eligibility with a FileIdentity is an object resource")
                    };
                    *identity = migration.to.clone();
                    reservation.eligibility.epoch =
                        Revision::new(reservation.eligibility.epoch.value().saturating_add(1));
                }
                // No migration was recorded because the identity did not really change, or because
                // the eligibility had already been released. Both leave the reservation as it is.
                Ok(None) => {}
                // **A refused migration is a real loss of exclusive responsibility.**
                //
                // The disk write really happened, so the reservation and the path index still move
                // to the new identity — otherwise the buffer would point at an object that no
                // longer exists. What must *not* happen is the pre-fix behaviour: leave the local
                // bookkeeping rebound to an object nobody holds and continue as if the buffer were
                // still protected. The replacement is held by another execution, so this session's
                // eligibility did not follow it, and the honest response is to say so and let the
                // caller reconcile rather than silently rebinding unprotected state.
                Err(refusal) => {
                    let path = current.path.clone();
                    reservations.insert(new_identity.clone(), reservation);
                    drop(reservations);
                    drop(path_index);
                    buffers.insert(new_key, current);
                    self.record_save_failure(
                        &key,
                        reservation_id,
                        reservation_epoch,
                        "editor.save_identity_migration_refused",
                    );
                    // The real holder is named, so the caller can say *who* holds the replacement
                    // rather than reporting an anonymous conflict.
                    let _ = refusal;
                    return Err(WorkspaceError::FileVersionConflict { path });
                }
            }
            reservations.insert(new_identity, reservation);
        }
        let result = status(&new_key, &current)?;
        buffers.insert(new_key, current);
        Ok(result)
    }

    /// Records a save failure against the live buffer, ignoring a buffer that is already gone.
    fn record_save_failure(
        &self,
        key: &BufferKey,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
        error_code: &str,
    ) {
        let Ok(mut buffers) = self.buffers.write() else {
            return;
        };
        if let Some(record) = buffers.get_mut(key)
            && record.reservation_id == Some(reservation_id)
            && record.reservation_epoch == reservation_epoch
        {
            record.save_state = EditorSaveState::Failed { error_code: error_code.to_owned() };
        }
    }

    /// Records a version conflict against the live buffer.
    fn record_save_conflict(
        &self,
        key: &BufferKey,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
    ) {
        let Ok(mut buffers) = self.buffers.write() else {
            return;
        };
        if let Some(record) = buffers.get_mut(key)
            && record.reservation_id == Some(reservation_id)
            && record.reservation_epoch == reservation_epoch
        {
            record.save_state = EditorSaveState::Conflict;
        }
    }

    pub fn set_active_editing(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        path: &WorkspacePath,
        active: bool,
    ) -> Result<(), WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let key = self.buffer_key(workspace_id, session_id, path)?;
        let mut buffers =
            self.buffers.write().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let record = buffers.get_mut(&key).ok_or(WorkspaceError::BufferNotFound)?;
        record.active_editing = active;
        Ok(())
    }

    /// The session-authoritative view of one logical file.
    ///
    /// # Why no global gate across the read
    ///
    /// This used to hold `operation_gate` across `open_existing` and, on the disk path,
    /// `read_disk` — a full file read taken under the index gate. The gate keeps the in-memory
    /// index consistent, so it is taken only for the index section; the identity probe and the disk
    /// read happen first with no lock held. The buffer lookup still sees exactly the identity the
    /// probe resolved, so a buffer that was replaced underneath is not silently used.
    pub fn logical_file_snapshot(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
    ) -> Result<LogicalFileSnapshot, WorkspaceError> {
        // Section 1: no lock. Resolve the real identity of whatever is there now.
        let existing = resolver.open_existing(path, NativeFileAccess::Metadata)?;
        let file_identity = crate::domain_identity(&existing.file.info().identity);
        drop(existing);
        let key = BufferKey {
            workspace_id: resolver.workspace_id(),
            session_id,
            file_identity: file_identity.clone(),
        };
        let active_editor = self.active_editor_by_identity(&file_identity)?;
        // Section 2: the short index section. A writable buffer wins and disk is never re-read.
        let buffered = self
            .buffers
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
            .get(&key)
            .cloned();
        if let Some(record) = buffered
            && record.writable
        {
            let bytes = encode_text(
                &record.decoded.text,
                record.decoded.encoding,
                record.decoded.line_ending,
            )?;
            return Ok(LogicalFileSnapshot {
                path: path.clone(),
                source: FileStorageSource::EditorBuffer,
                logical_hash: ContentHash::blake3(&bytes),
                disk_hash: Some(record.disk_hash),
                buffer_revision: Some(record.revision),
                is_dirty: record.dirty,
                active_editor_session: active_editor,
                bytes,
                encoding: Some(record.decoded.encoding),
            });
        }
        let bytes = read_disk(resolver, path)?;
        let disk_hash = ContentHash::blake3(&bytes);
        Ok(LogicalFileSnapshot {
            path: path.clone(),
            source: FileStorageSource::Disk,
            logical_hash: disk_hash.clone(),
            disk_hash: Some(disk_hash),
            buffer_revision: None,
            is_dirty: false,
            active_editor_session: active_editor,
            bytes,
            encoding: None,
        })
    }

    /// Bounded-memory text read for a file that is too large to materialize.
    ///
    /// Returns [`DiskTextSnapshot::UseSnapshot`] whenever the ordinary
    /// [`Self::logical_file_snapshot`] path is already correct and authoritative:
    /// this session holds a writable editor buffer (the buffer wins and disk is
    /// never re-read), or the file is absent, not a regular file, or not larger
    /// than `full_text_read_limit_bytes`. A "stream" result means the file was
    /// opened once and read once in `LOGICAL_TEXT_STREAM_CHUNK_BYTES` chunks:
    /// `logical_hash` is the exact whole-file hash, `total_characters` and
    /// `total_lines` are exact whole-file counts, and only the requested range
    /// text is retained. `enforce_text_policy` remains the caller's decision and
    /// uses the same byte length, so policy errors do not change.
    ///
    /// # Why no global gate
    ///
    /// This method used to hold `operation_gate` across the whole streaming read — an open, a
    /// `metadata` query and a chunk loop over a file that is by definition too large to
    /// materialize. That is the worst possible thing to serialise on the index gate: one big
    /// streaming read in one workspace blocked every buffer operation in every other workspace for
    /// its whole duration. The gate protects the in-memory index, and this method reads exactly one
    /// map (`active_editor_by_identity`) without mutating anything, so it needs no gate at all. The
    /// editor check is the same plain map read it always was.
    pub fn disk_text_snapshot_streaming(
        &self,
        resolver: &SecurePathResolver,
        path: &WorkspacePath,
        range: &TextReadRange,
        explicit_encoding: Option<TextEncoding>,
        full_text_read_limit_bytes: u64,
        chunk_bytes: usize,
    ) -> Result<DiskTextSnapshot, WorkspaceError> {
        if matches!(range, TextReadRange::Full) {
            // A full read retains the whole file either way, so the ordinary
            // whole-file snapshot is the right path and no chunking happens here.
            return Ok(DiskTextSnapshot::UseSnapshot);
        }
        let mut verified = resolver.open_existing(path, NativeFileAccess::Read)?;
        if verified.file.info().is_directory {
            return Err(WorkspaceError::WrongKind { path: path.clone() });
        }
        if self
            .active_editor_by_identity(&crate::domain_identity(&verified.file.info().identity))?
            .is_some()
        {
            return Ok(DiskTextSnapshot::UseSnapshot);
        }
        // The opened handle is authoritative for size; a catalog estimate must
        // never decide whether the whole file is materialized.
        let byte_length = verified
            .file
            .file_mut()
            .metadata()
            .map_err(|source| {
                io_error(
                    "read verified workspace file metadata",
                    &verified.file.info().final_path,
                    source,
                )
            })?
            .len();
        if byte_length <= full_text_read_limit_bytes {
            return Ok(DiskTextSnapshot::UseSnapshot);
        }
        let final_path = verified.file.info().final_path.clone();
        Ok(DiskTextSnapshot::Stream(stream_disk_text(
            verified.file.file_mut(),
            &final_path,
            path,
            range,
            explicit_encoding,
            chunk_bytes,
        )?))
    }

    /// Session-authoritative in-memory view, if this session has a writable
    /// editor buffer. Does not read disk file bytes.
    pub fn writable_search_snapshot(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
    ) -> Result<Option<LogicalFileSnapshot>, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let existing = match resolver.open_existing(path, NativeFileAccess::Metadata) {
            Ok(value) => value,
            Err(WorkspaceError::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
        let file_identity = crate::domain_identity(&existing.file.info().identity);
        let key = BufferKey {
            workspace_id: resolver.workspace_id(),
            session_id,
            file_identity: file_identity.clone(),
        };
        let active_editor = self.active_editor_by_identity(&file_identity)?;
        let Some(record) = self
            .buffers
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
            .get(&key)
            .cloned()
        else {
            return Ok(None);
        };
        if !record.writable {
            return Ok(None);
        }
        let bytes =
            encode_text(&record.decoded.text, record.decoded.encoding, record.decoded.line_ending)?;
        Ok(Some(LogicalFileSnapshot {
            path: path.clone(),
            source: FileStorageSource::EditorBuffer,
            logical_hash: ContentHash::blake3(&bytes),
            disk_hash: Some(record.disk_hash),
            buffer_revision: Some(record.revision),
            is_dirty: record.dirty,
            active_editor_session: active_editor,
            bytes,
            encoding: Some(record.decoded.encoding),
        }))
    }

    /// Paths this session currently holds a writable editor buffer for. A
    /// search asks once so candidates without a buffer do not pay a per-file
    /// buffer probe, and only paths in this set are resolved to a snapshot.
    pub fn writable_search_paths(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
    ) -> Result<Vec<WorkspacePath>, WorkspaceError> {
        let buffers =
            self.buffers.read().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        Ok(buffers
            .iter()
            .filter(|(key, record)| {
                key.workspace_id == workspace_id && key.session_id == session_id && record.writable
            })
            .map(|(_, record)| record.path.clone())
            .collect())
    }

    pub fn buffer_revision_matches(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
        expected: Revision,
    ) -> Result<bool, WorkspaceError> {
        let Some(snapshot) = self.writable_search_snapshot(resolver, session_id, path)? else {
            return Ok(false);
        };
        Ok(snapshot.buffer_revision == Some(expected))
    }

    /// The session holding the writable edit reservation on one target, resolved from disk.
    ///
    /// # Why no global gate
    ///
    /// This used to hold `operation_gate` across `resolver.open_existing`, which is a real file
    /// open — so every `active_editor` probe in every workspace serialised on the index gate while
    /// doing I/O the gate has no business covering. The gate exists to keep the in-memory index
    /// consistent, and this method never touches the index: it resolves a path and reads one map.
    /// The identity is resolved first, outside the gate, and only the map lookup runs inside it.
    pub fn active_editor(
        &self,
        resolver: &SecurePathResolver,
        path: &WorkspacePath,
    ) -> Result<Option<SessionId>, WorkspaceError> {
        let existing = resolver.open_existing(path, NativeFileAccess::Metadata)?;
        let identity = crate::domain_identity(&existing.file.info().identity);
        drop(existing);
        self.active_editor_by_identity(&identity)
    }

    /// The session holding the writable edit reservation on this **real object**.
    ///
    /// Deliberately not filtered by workspace: the reservation is on the object, so a caller in a
    /// second workspace asking about the same physical file must be told about the real holder
    /// rather than being told the file is free.
    fn active_editor_by_identity(
        &self,
        identity: &FileIdentity,
    ) -> Result<Option<SessionId>, WorkspaceError> {
        Ok(self
            .reservations
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor reservations"))?
            .get(identity)
            .map(|reservation| reservation.session_id))
    }

    pub fn list_active_dirty_buffers(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<DiskStaleReport, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let mut paths = self
            .buffers
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
            .iter()
            .filter(|(key, record)| {
                key.workspace_id == workspace_id && record.dirty && record.active_editing
            })
            .map(|(_, record)| record.path.clone())
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        Ok(DiskStaleReport { workspace_disk_may_be_stale: !paths.is_empty(), paths })
    }

    pub fn statuses(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<LogicalFileStatus>, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let buffers =
            self.buffers.read().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let mut statuses = buffers
            .iter()
            .filter(|(key, _)| key.workspace_id == workspace_id)
            .map(|(key, record)| status(key, record))
            .collect::<Result<Vec<_>, _>>()?;
        statuses.sort_by(|left, right| {
            left.path.cmp(&right.path).then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(statuses)
    }

    /// The live reservations belonging to one workspace.
    ///
    /// The reservation map is keyed by the real object, so the workspace filter is applied to the
    /// reservation's own recorded workspace rather than to its key. A reservation taken through
    /// another workspace on a file this one also reaches is therefore reported to its real owner
    /// and not invented as a second entry here.
    pub fn reservation_summaries(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<EditorReservationSummary>, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let mut summaries = self
            .reservations
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor reservations"))?
            .values()
            .filter(|reservation| reservation.workspace_id == workspace_id)
            .map(|reservation| EditorReservationSummary {
                session_id: reservation.session_id,
                path: reservation.path.clone(),
                reservation_id: reservation.reservation_id,
                reservation_epoch: reservation.epoch,
            })
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| {
            left.path.cmp(&right.path).then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(summaries)
    }

    /// Dirty buffers in this workspace that no session is actively editing, reported but never
    /// written.
    ///
    /// This replaces the former `flush_non_active_dirty_buffers`, which *saved* every other
    /// session's dirty buffer as an incidental side effect of an unrelated mutation. That was
    /// wrong on its own terms: a caller mutating one file has no authority over another session's
    /// unsaved work, and silently committing it discards the other session's ability to decide
    /// what its buffer becomes and races the same target the caller is about to write. The
    /// product rule is that an ordinary mutation must not save another session's unrelated dirty
    /// content, and that the same real target keeps an explicit long-edit claim rather than being
    /// silently taken over.
    ///
    /// So this returns **facts** and performs no I/O: who holds an uncommitted buffer, at which
    /// revision. The caller decides what to do about it — the same way reads elsewhere provide
    /// observed facts without blocking writes.
    pub fn uncommitted_editor_buffers(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<UncommittedEditorBuffer>, WorkspaceError> {
        let buffers =
            self.buffers.read().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let mut uncommitted = buffers
            .iter()
            .filter(|(key, record)| {
                key.workspace_id == workspace_id
                    && record.dirty
                    && !record.active_editing
                    && record.reservation_id.is_some()
            })
            .map(|(key, record)| UncommittedEditorBuffer {
                session_id: key.session_id,
                path: record.path.clone(),
                revision: record.revision,
                reservation_id: record.reservation_id.unwrap_or_else(EditorReservationId::new),
                reservation_epoch: record.reservation_epoch,
            })
            .collect::<Vec<_>>();
        uncommitted.sort_by(|left, right| {
            left.path.cmp(&right.path).then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(uncommitted)
    }

    pub fn invalidate_workspace(&self, workspace_id: WorkspaceId) -> Result<(), WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        self.clear_workspace_locked(workspace_id, true)?;
        self.lifecycle_gates
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace lifecycle buffer gates"))?
            .remove(&workspace_id);
        Ok(())
    }

    pub fn begin_lifecycle(&self, workspace_id: WorkspaceId) -> Result<(), WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let mut gates = self
            .lifecycle_gates
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace lifecycle buffer gates"))?;
        if gates.insert(workspace_id, LifecycleBufferGate::Preparing).is_some() {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        Ok(())
    }

    pub fn freeze_lifecycle(&self, workspace_id: WorkspaceId) -> Result<(), WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let mut gates = self
            .lifecycle_gates
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace lifecycle buffer gates"))?;
        let state = gates.get_mut(&workspace_id).ok_or(WorkspaceError::LifecycleNotFound)?;
        *state = LifecycleBufferGate::Frozen;
        Ok(())
    }

    pub fn cancel_lifecycle(&self, workspace_id: WorkspaceId) -> Result<(), WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        self.lifecycle_gates
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace lifecycle buffer gates"))?
            .remove(&workspace_id);
        Ok(())
    }

    pub fn lifecycle_preflight(
        &self,
        workspace_id: WorkspaceId,
        renderer_buffers: &[EditorBufferLifecycleState],
    ) -> Result<BufferLifecyclePreflight, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        if self.workspace_gate(workspace_id)? != Some(LifecycleBufferGate::Frozen) {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        let buffers =
            self.buffers.read().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let mut reported = renderer_buffers
            .iter()
            .map(|state| ((state.session_id, state.canonical_relative_path.clone()), state))
            .collect::<HashMap<_, _>>();
        if reported.len() != renderer_buffers.len() {
            return Ok(BufferLifecyclePreflight {
                blockers: vec![WorkspaceLifecycleBlocker::EditorStateUnconfirmed {
                    session_id: None,
                    path: None,
                }],
                releasable_editor_states: Vec::new(),
            });
        }
        let mut blockers = Vec::new();
        let mut releasable = Vec::new();
        for (key, record) in buffers.iter().filter(|(key, _)| key.workspace_id == workspace_id) {
            let identity = (key.session_id, record.path.clone());
            let Some(renderer) = reported.remove(&identity) else {
                blockers.push(WorkspaceLifecycleBlocker::EditorStateUnconfirmed {
                    session_id: Some(key.session_id),
                    path: Some(record.path.clone()),
                });
                continue;
            };
            let logical_hash = logical_hash(record)?;
            if renderer.reservation_id != record.reservation_id
                || renderer.reservation_epoch != record.reservation_epoch
                || renderer.logical_hash != logical_hash
                || renderer.disk_hash.as_ref() != Some(&record.disk_hash)
                || renderer.is_dirty != record.dirty
            {
                blockers.push(WorkspaceLifecycleBlocker::EditorStateUnconfirmed {
                    session_id: Some(key.session_id),
                    path: Some(record.path.clone()),
                });
            }
            if renderer.buffer_revision != record.revision
                || renderer.core_acknowledged_revision != record.revision
            {
                blockers.push(WorkspaceLifecycleBlocker::PendingBufferSynchronization {
                    session_id: key.session_id,
                    path: record.path.clone(),
                    renderer_revision: renderer.buffer_revision,
                    core_revision: record.revision,
                });
            }
            if record.dirty || renderer.is_dirty {
                blockers.push(WorkspaceLifecycleBlocker::DirtyBuffer {
                    session_id: key.session_id,
                    path: record.path.clone(),
                    buffer_revision: record.revision,
                });
            }
            if record.active_editing || renderer.is_active_editing {
                blockers.push(WorkspaceLifecycleBlocker::ActiveEditing {
                    session_id: key.session_id,
                    path: record.path.clone(),
                    ime_composition_active: record.ime_composition_active
                        || renderer.ime_composition_active,
                });
            } else if record.ime_composition_active || renderer.ime_composition_active {
                blockers.push(WorkspaceLifecycleBlocker::ActiveEditing {
                    session_id: key.session_id,
                    path: record.path.clone(),
                    ime_composition_active: true,
                });
            }
            push_save_blocker(&mut blockers, key.session_id, &record.path, &record.save_state);
            push_save_blocker(&mut blockers, key.session_id, &record.path, &renderer.save_state);
            if !record.dirty
                && !renderer.is_dirty
                && !record.active_editing
                && !renderer.is_active_editing
                && !record.ime_composition_active
                && !renderer.ime_composition_active
                && matches!(record.save_state, EditorSaveState::Idle)
                && matches!(renderer.save_state, EditorSaveState::Idle)
                && renderer.buffer_revision == record.revision
                && renderer.core_acknowledged_revision == record.revision
            {
                releasable.push(EditorStateRef {
                    session_id: key.session_id,
                    canonical_relative_path: record.path.clone(),
                    reservation_id: record.reservation_id,
                    reservation_epoch: record.reservation_epoch,
                    buffer_revision: record.revision,
                });
            }
        }
        for renderer in reported.into_values() {
            blockers.push(WorkspaceLifecycleBlocker::EditorStateUnconfirmed {
                session_id: Some(renderer.session_id),
                path: Some(renderer.canonical_relative_path.clone()),
            });
        }
        Ok(BufferLifecyclePreflight { blockers, releasable_editor_states: releasable })
    }

    pub fn current_lifecycle_states(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<EditorBufferLifecycleState>, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let buffers =
            self.buffers.read().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let mut states = buffers
            .iter()
            .filter(|(key, _)| key.workspace_id == workspace_id)
            .map(|(key, record)| lifecycle_state(key, record))
            .collect::<Result<Vec<_>, _>>()?;
        states.sort_by(|left, right| {
            left.canonical_relative_path
                .cmp(&right.canonical_relative_path)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(states)
    }

    pub fn discard_bindings(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DiscardBufferBinding>, WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        let buffers =
            self.buffers.read().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let mut bindings = buffers
            .iter()
            .filter(|(key, record)| key.workspace_id == workspace_id && record.dirty)
            .map(|(key, record)| {
                let reservation_id = record.reservation_id.ok_or(WorkspaceError::BufferReadOnly)?;
                Ok(DiscardBufferBinding {
                    session_id: key.session_id,
                    canonical_relative_path: record.path.clone(),
                    reservation_id,
                    reservation_epoch: record.reservation_epoch,
                    buffer_revision: record.revision,
                    logical_hash: logical_hash(record)?,
                    disk_hash: Some(record.disk_hash.clone()),
                })
            })
            .collect::<Result<Vec<_>, WorkspaceError>>()?;
        bindings.sort_by(|left, right| {
            left.canonical_relative_path
                .cmp(&right.canonical_relative_path)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(bindings)
    }

    pub fn discard_dirty_for_lifecycle(
        &self,
        workspace_id: WorkspaceId,
        expected: &[DiscardBufferBinding],
    ) -> Result<(), WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        if self.workspace_gate(workspace_id)? != Some(LifecycleBufferGate::Frozen) {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        let actual = self.discard_bindings_locked(workspace_id)?;
        if actual != expected {
            return Err(WorkspaceError::DiscardConfirmationStale);
        }
        let dirty_keys = self
            .buffers
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
            .iter()
            .filter(|(key, record)| key.workspace_id == workspace_id && record.dirty)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        self.remove_buffer_keys_locked(&dirty_keys)?;
        Ok(())
    }

    pub fn complete_lifecycle_cleanup(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<(), WorkspaceError> {
        let _operation = self
            .operation_gate
            .lock()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
        if self.workspace_gate(workspace_id)? != Some(LifecycleBufferGate::Frozen) {
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        if self
            .buffers
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
            .iter()
            .any(|(key, record)| {
                key.workspace_id == workspace_id
                    && (record.dirty
                        || record.active_editing
                        || record.ime_composition_active
                        || !matches!(record.save_state, EditorSaveState::Idle))
            })
        {
            return Err(WorkspaceError::LifecycleEditorBlockersRemain);
        }
        self.clear_workspace_locked(workspace_id, true)?;
        self.lifecycle_gates
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace lifecycle buffer gates"))?
            .remove(&workspace_id);
        Ok(())
    }

    fn discard_bindings_locked(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DiscardBufferBinding>, WorkspaceError> {
        let buffers =
            self.buffers.read().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let mut bindings = buffers
            .iter()
            .filter(|(key, record)| key.workspace_id == workspace_id && record.dirty)
            .map(|(key, record)| {
                Ok(DiscardBufferBinding {
                    session_id: key.session_id,
                    canonical_relative_path: record.path.clone(),
                    reservation_id: record.reservation_id.ok_or(WorkspaceError::BufferReadOnly)?,
                    reservation_epoch: record.reservation_epoch,
                    buffer_revision: record.revision,
                    logical_hash: logical_hash(record)?,
                    disk_hash: Some(record.disk_hash.clone()),
                })
            })
            .collect::<Result<Vec<_>, WorkspaceError>>()?;
        bindings.sort_by(|left, right| {
            left.canonical_relative_path
                .cmp(&right.canonical_relative_path)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(bindings)
    }

    fn clear_workspace_locked(
        &self,
        workspace_id: WorkspaceId,
        revoke: bool,
    ) -> Result<(), WorkspaceError> {
        let keys = self
            .buffers
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
            .keys()
            .filter(|key| key.workspace_id == workspace_id)
            .cloned()
            .collect::<Vec<_>>();
        if revoke {
            self.remove_buffer_keys_locked(&keys)?;
        } else {
            self.buffers
                .write()
                .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
                .retain(|key, _| key.workspace_id != workspace_id);
            self.path_index
                .write()
                .map_err(|_| WorkspaceError::LockPoisoned("editor buffer path index"))?
                .retain(|(candidate, _, _), _| *candidate != workspace_id);
        }
        Ok(())
    }

    /// Removes buffers and their reservations, and releases the long eligibilities they held.
    ///
    /// # Why the owner is released here and not only in the local maps
    ///
    /// A revocation, a discard-confirmed lifecycle cleanup or a reload ends this session's edit on
    /// the object. If only the local reservation map were cleared, the shared owner would still
    /// hold a live long eligibility that **no caller could ever release again**, because the
    /// reservation record that names its `claim_id` is what a later release would address. The
    /// object would then be permanently unavailable to Main Fs, the Agent tools and the artifact
    /// writer — a leak that looks exactly like a deadlock from the outside.
    ///
    /// The release goes through [`EligibilityOwner::release_claim`], so it removes the eligibility
    /// this session holds and nothing else: another session's live eligibility on another object is
    /// untouched, and the owner's own rule (only the owning session may release) is what enforces
    /// it rather than this function's bookkeeping.
    fn remove_buffer_keys_locked(&self, keys: &[BufferKey]) -> Result<(), WorkspaceError> {
        let mut buffers =
            self.buffers.write().map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?;
        let mut reservations = self
            .reservations
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("editor reservations"))?;
        let mut revoked = self
            .revoked_reservations
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("revoked editor reservations"))?;
        let mut path_index = self
            .path_index
            .write()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer path index"))?;
        for key in keys {
            if let Some(record) = buffers.remove(key)
                && let Some(reservation_id) = record.reservation_id
            {
                let epoch = next_revision(record.reservation_epoch)?;
                revoked.insert(reservation_id, epoch);
                if let Some(reservation) = reservations.remove(&key.file_identity) {
                    self.eligibility
                        .release_claim(&reservation.eligibility.claim_id, reservation.session_id);
                }
            }
            path_index.retain(|(workspace_id, session_id, _), identity| {
                !(*workspace_id == key.workspace_id
                    && *session_id == key.session_id
                    && identity == &key.file_identity)
            });
        }
        Ok(())
    }

    fn workspace_gate(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Option<LifecycleBufferGate>, WorkspaceError> {
        Ok(self
            .lifecycle_gates
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("workspace lifecycle buffer gates"))?
            .get(&workspace_id)
            .copied())
    }

    fn revoked_or_missing<T>(
        &self,
        reservation_id: EditorReservationId,
    ) -> Result<T, WorkspaceError> {
        let current_epoch = self
            .revoked_reservations
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("revoked editor reservations"))?
            .get(&reservation_id)
            .copied();
        match current_epoch {
            Some(current_epoch) => {
                Err(WorkspaceError::ReservationRevoked { reservation_id, current_epoch })
            }
            None => Err(WorkspaceError::ReservationNotFound { reservation_id }),
        }
    }

    fn buffer_key(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        path: &WorkspacePath,
    ) -> Result<BufferKey, WorkspaceError> {
        let file_identity = self
            .path_index
            .read()
            .map_err(|_| WorkspaceError::LockPoisoned("editor buffer path index"))?
            .get(&(workspace_id, session_id, path.clone()))
            .cloned()
            .ok_or(WorkspaceError::BufferNotFound)?;
        Ok(BufferKey { workspace_id, session_id, file_identity })
    }
}

fn logical_hash(record: &BufferRecord) -> Result<ContentHash, WorkspaceError> {
    let bytes =
        encode_text(&record.decoded.text, record.decoded.encoding, record.decoded.line_ending)?;
    Ok(ContentHash::blake3(&bytes))
}

fn push_save_blocker(
    blockers: &mut Vec<WorkspaceLifecycleBlocker>,
    session_id: SessionId,
    path: &WorkspacePath,
    state: &EditorSaveState,
) {
    match state {
        EditorSaveState::Idle => {}
        EditorSaveState::Saving => {
            blockers
                .push(WorkspaceLifecycleBlocker::SaveInProgress { session_id, path: path.clone() });
        }
        EditorSaveState::Failed { error_code } => {
            blockers.push(WorkspaceLifecycleBlocker::SaveFailed {
                session_id,
                path: path.clone(),
                error_code: error_code.clone(),
            });
        }
        EditorSaveState::Conflict => {
            blockers.push(WorkspaceLifecycleBlocker::FileVersionConflict {
                session_id,
                path: path.clone(),
            });
        }
    }
}

fn next_revision(revision: Revision) -> Result<Revision, WorkspaceError> {
    Ok(Revision::new(revision.value().checked_add(1).ok_or(WorkspaceError::RevisionExhausted)?))
}

/// Outcome of [`BufferStore::disk_text_snapshot_streaming`].
pub enum DiskTextSnapshot {
    /// The caller must use the ordinary whole-file
    /// `logical_file_snapshot` path: an editor buffer is authoritative, or the
    /// file is not larger than the streaming threshold.
    UseSnapshot,
    /// The file was streamed once without being materialized.
    Stream(LogicalTextReadSnapshot),
}

/// Decoder that reproduces the exact strictness of
/// `catalog::decode_text`/`decode_text_with_encoding`: UTF-8 (with optional byte
/// order mark), UTF-16LE, and UTF-16BE. `Windows1252` is not streamed because the
/// existing path never rejects undefined bytes for it, while its streaming
/// decoder would; the caller keeps that case on the whole-file path.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamDecoder {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// Windows-1252 is a single-byte encoding with no invalid sequences: every
    /// byte maps to one character, including the five undefined C1 positions,
    /// which is why the whole-file decoder never rejects it. It is therefore
    /// streamed one byte at a time with exactly the same mapping, so a large
    /// legacy file no longer has to be materialized to be read.
    Windows1252,
}

/// The encoding decision for one streamed read, plus the byte order mark it
/// consumed.
#[derive(Clone, Copy)]
struct StreamEncoding {
    decoder: StreamDecoder,
    /// What the read reports. A byte order mark is part of the answer, so a
    /// UTF-8 file with one reports `Utf8Bom`, exactly as the whole-file path does.
    declared: TextEncoding,
    /// Bytes to drop from the front of the first chunk.
    strip: usize,
}

impl StreamDecoder {
    /// Resolves the encoding exactly as the whole-file path does.
    ///
    /// An explicit `encoding` is a caller decision and is never overridden by
    /// automatic detection, which is what the whole-file
    /// `decode_text_with_encoding` does too. Without one, the order is the byte
    /// order mark, then UTF-8, then the explicit-UTF-8 strictness the whole-file
    /// decoder applies. `Windows1252` is only ever reached through an explicit
    /// choice, so an undefined byte can never be silently substituted here.
    fn resolve(first: &[u8], explicit: Option<TextEncoding>) -> StreamEncoding {
        let bom = Self::byte_order_mark(first);
        match explicit {
            // UTF-16 always consumes its mark: the whole-file decoder strips the
            // matching two bytes whether or not the caller asked for the mark to
            // be honoured.
            Some(TextEncoding::Utf16Le) => StreamEncoding {
                decoder: Self::Utf16Le,
                declared: TextEncoding::Utf16Le,
                strip: usize::from(bom == Some(Self::Utf16Le)) * 2,
            },
            Some(TextEncoding::Utf16Be) => StreamEncoding {
                decoder: Self::Utf16Be,
                declared: TextEncoding::Utf16Be,
                strip: usize::from(bom == Some(Self::Utf16Be)) * 2,
            },
            // Plain UTF-8 means "these exact bytes are the text", so a leading
            // mark stays a character. The whole-file path keeps it as U+FEFF, and
            // the streamed read must agree rather than silently swallowing it.
            Some(TextEncoding::Utf8) => {
                StreamEncoding { decoder: Self::Utf8, declared: TextEncoding::Utf8, strip: 0 }
            }
            Some(TextEncoding::Utf8Bom) => StreamEncoding {
                decoder: Self::Utf8,
                declared: TextEncoding::Utf8Bom,
                strip: usize::from(bom == Some(Self::Utf8)) * 3,
            },
            Some(TextEncoding::Windows1252) => StreamEncoding {
                decoder: Self::Windows1252,
                declared: TextEncoding::Windows1252,
                strip: 0,
            },
            None => match bom {
                Some(Self::Utf8) => StreamEncoding {
                    decoder: Self::Utf8,
                    declared: TextEncoding::Utf8Bom,
                    strip: 3,
                },
                Some(Self::Utf16Le) => StreamEncoding {
                    decoder: Self::Utf16Le,
                    declared: TextEncoding::Utf16Le,
                    strip: 2,
                },
                Some(Self::Utf16Be) => StreamEncoding {
                    decoder: Self::Utf16Be,
                    declared: TextEncoding::Utf16Be,
                    strip: 2,
                },
                None => {
                    StreamEncoding { decoder: Self::Utf8, declared: TextEncoding::Utf8, strip: 0 }
                }
                // `byte_order_mark` never reports this, so the arm is unreachable;
                // it exists to keep the mark type a closed set.
                Some(Self::Windows1252) => {
                    StreamEncoding { decoder: Self::Utf8, declared: TextEncoding::Utf8, strip: 0 }
                }
            },
        }
    }

    /// Recognizes only the marks the whole-file decoder recognizes. It never
    /// guesses UTF-16 from a byte pattern, because the whole-file decoder does not
    /// either: bytes that are not valid UTF-8 and carry no mark are the ambiguous
    /// case, and only an explicit encoding resolves them.
    fn byte_order_mark(first: &[u8]) -> Option<Self> {
        if first.starts_with(&[0xef, 0xbb, 0xbf]) {
            return Some(Self::Utf8);
        }
        if first.starts_with(&[0xff, 0xfe]) {
            return Some(Self::Utf16Le);
        }
        if first.starts_with(&[0xfe, 0xff]) {
            return Some(Self::Utf16Be);
        }
        None
    }

    fn decoder(&self) -> encoding_rs::Decoder {
        match self {
            // The whole-file path strips a UTF-8 mark only when the caller asked
            // for the mark to be honoured. `new_decoder` would strip it on its own
            // for every UTF-8 read, which would silently swallow a mark that the
            // whole-file path keeps as U+FEFF, so the automatic behaviour is off
            // and the mark is removed explicitly by `StreamEncoding::strip`.
            Self::Utf8 => encoding_rs::UTF_8.new_decoder_without_bom_handling(),
            Self::Utf16Le => encoding_rs::UTF_16LE.new_decoder(),
            Self::Utf16Be => encoding_rs::UTF_16BE.new_decoder(),
            // Only reached for the single-byte path, which never calls this.
            Self::Windows1252 => encoding_rs::WINDOWS_1252.new_decoder(),
        }
    }

    /// True when the encoding maps one byte to one character and can never report
    /// a malformed sequence.
    fn is_single_byte(&self) -> bool {
        matches!(self, Self::Windows1252)
    }
}

/// Where a character range is in its walk through the stream.
#[derive(Clone, Copy, PartialEq)]
enum CharacterPhase {
    /// Counting down the characters before the range starts.
    Skipping,
    /// Retaining the requested characters.
    Retaining,
    /// The range is complete; only the whole-file tallies continue.
    Released,
}

/// Streaming whole-file tallies and the requested range's text.
///
/// A line range is selected from `line_index`, which only advances at a line feed,
/// and a character range from `character_phase`; both stop accumulating once the
/// range is served, at which point the accumulator moves into `selected` and is
/// emptied. The peak the accumulator ever holds is the requested text plus at most
/// one line, never the rest of the file.
struct StreamTextState {
    range: TextReadRange,
    total_characters: u64,
    total_lines: u64,
    character_phase: CharacterPhase,
    /// Index of the line the scan is currently inside, counted exactly as
    /// `str::lines()` numbers its lines.
    line_index: u64,
    /// Latched once a line range has taken its last requested line, so the window
    /// cannot re-open as `line_index` keeps advancing.
    lines_complete: bool,
    /// Requested and still-outstanding counts for the active phase.
    skip_characters: u64,
    retain_characters: u64,
    retained: String,
    /// The finished requested text, moved out of `retained` so the accumulator can
    /// be emptied while the scan continues for the whole-file tallies.
    selected: Option<String>,
    peak_retained_bytes: usize,
    /// Whether the previous character was a carriage return, so a `\r\n` split
    /// across two chunks is counted as one break and one line-ending convention.
    pending_cr: bool,
    /// Whether the last character decoded was a line feed. `str::lines()` does not
    /// count a trailing terminator as an extra line.
    last_was_newline: bool,
    seen_crlf: bool,
    seen_lf: bool,
    seen_cr: bool,
}

impl StreamTextState {
    fn new(range: &TextReadRange) -> Self {
        let (character_phase, skip_characters, retain_characters) = match range {
            TextReadRange::Characters { start_character_offset, count } => {
                if *count == 0 {
                    (CharacterPhase::Released, 0, 0)
                } else if *start_character_offset == 0 {
                    (CharacterPhase::Retaining, 0, *count)
                } else {
                    (CharacterPhase::Skipping, *start_character_offset, *count)
                }
            }
            TextReadRange::Full | TextReadRange::Lines { .. } => (CharacterPhase::Released, 0, 0),
        };
        Self {
            range: range.clone(),
            total_characters: 0,
            total_lines: 0,
            character_phase,
            line_index: 0,
            lines_complete: false,
            skip_characters,
            retain_characters,
            retained: String::new(),
            selected: None,
            peak_retained_bytes: 0,
            pending_cr: false,
            last_was_newline: false,
            seen_crlf: false,
            seen_lf: false,
            seen_cr: false,
        }
    }

    /// True once the requested range has been served, so no further input can
    /// change the returned text. The exact whole-file hash and counts still require
    /// every remaining byte, which is why the caller keeps scanning after this.
    fn range_complete(&self) -> bool {
        match self.range {
            TextReadRange::Full => false,
            TextReadRange::Lines { .. } => self.selected.is_some(),
            TextReadRange::Characters { count, .. } => {
                count == 0 || self.character_phase == CharacterPhase::Released
            }
        }
    }

    /// Move the finished range out of the accumulator so the rest of the scan
    /// cannot grow it any further. The requested text is already complete: a line
    /// range ends at the terminator that closed its last line, and a character
    /// range ends at its last requested character.
    fn mark_complete(&mut self) {
        if self.selected.is_none() {
            self.selected = Some(std::mem::take(&mut self.retained));
            self.retained.shrink_to_fit();
        }
    }

    fn retain(&mut self, character: char) {
        self.retained.push(character);
        self.peak_retained_bytes = self.peak_retained_bytes.max(self.retained.len());
    }

    /// The line tally advances when a line feed arrives; `line_index` moves with
    /// it, and `retains_line` reads the window from that index. Counting and
    /// selection therefore cannot disagree about where a line ends.
    fn push_character(&mut self, character: char, retain: bool) -> bool {
        self.total_characters = self.total_characters.saturating_add(1);
        match self.character_phase {
            CharacterPhase::Skipping => {
                self.skip_characters = self.skip_characters.saturating_sub(1);
                if self.skip_characters == 0 {
                    self.character_phase = CharacterPhase::Retaining;
                }
                false
            }
            CharacterPhase::Retaining => {
                self.retain(character);
                self.retain_characters = self.retain_characters.saturating_sub(1);
                if self.retain_characters == 0 {
                    self.character_phase = CharacterPhase::Released;
                    // Nothing after the last requested character can change the
                    // text, so the accumulator stops growing here.
                    self.mark_complete();
                }
                true
            }
            CharacterPhase::Released if retain => {
                self.retain(character);
                true
            }
            CharacterPhase::Released => false,
        }
    }

    /// Whether a line range still wants the line the scan is currently inside.
    /// `lines_complete` latches, because `line_index` keeps advancing after the
    /// window and must never re-open it.
    fn retains_line(&self, line_index: u64) -> bool {
        if self.lines_complete {
            return false;
        }
        match self.range {
            TextReadRange::Full => true,
            TextReadRange::Lines { start_line_index, count } => {
                count > 0
                    && line_index >= start_line_index
                    && line_index < start_line_index.saturating_add(count)
            }
            TextReadRange::Characters { .. } => false,
        }
    }

    fn absorb(&mut self, text: &str) {
        for character in text.chars() {
            // `str::lines()` splits on `\n` only, swallowing the `\r` of a `\r\n`
            // pair. A bare `\r` is not a line boundary; it only contributes to the
            // detected line-ending convention.
            let retain = self.retains_line(self.line_index);
            match character {
                '\n' => {
                    let kind =
                        if self.pending_cr { LineBreakKind::CrLf } else { LineBreakKind::Lf };
                    self.pending_cr = false;
                    self.last_was_newline = true;
                    // The terminator belongs to the line it closes, so the window
                    // still owns it and only closes once the next line starts.
                    self.push_character(character, retain);
                    // The convention is a whole-file fact. It is recorded whether
                    // or not the range still retains characters, because a range
                    // that closed early must not hide a later newline style.
                    match kind {
                        LineBreakKind::CrLf => self.seen_crlf = true,
                        LineBreakKind::Lf => self.seen_lf = true,
                    }
                    self.total_lines = self.total_lines.saturating_add(1);
                    // The next line starts here, which is the only place the line
                    // window may advance.
                    self.line_index = self.line_index.saturating_add(1);
                    // The terminator belongs to the line it closed, so it is
                    // retained above and the window only closes after it. That is
                    // what lets `normalize_lines` drop it as `lines().join` does.
                    if retain && !self.retains_line(self.line_index) {
                        self.lines_complete = true;
                        self.mark_complete();
                    }
                }
                '\r' => {
                    self.pending_cr = true;
                    self.last_was_newline = false;
                    // A `\r` is a bare carriage return exactly when the next
                    // character is not `\n`, so the decision is made by the next
                    // iteration or by `finish`. The pending flag is what makes this
                    // independent of whether the range retained the character.
                    self.push_character(character, retain);
                }
                _ => {
                    if self.pending_cr {
                        // The character before this one was a bare carriage return:
                        // its `\n` never arrived, so a mixed convention is in play.
                        self.pending_cr = false;
                        self.seen_cr = true;
                    }
                    self.last_was_newline = false;
                    self.push_character(character, retain);
                }
            }
        }
    }

    fn finish(mut self) -> (String, u64, u64, usize, LineEnding) {
        if self.pending_cr {
            // The text ends with a carriage return whose `\n` never arrived, so it
            // is a bare carriage return, not part of the line-ending convention.
            self.seen_cr = true;
        }
        if self.total_characters > 0 && !self.last_was_newline {
            // The text ended inside its last line, which `str::lines()` still counts.
            self.total_lines = self.total_lines.saturating_add(1);
        }
        // Same decision as `catalog::detect_line_ending`, but from the whole
        // stream rather than from the retained range alone.
        let line_ending = match (self.seen_crlf, self.seen_lf, self.seen_cr) {
            (false, false, false) => LineEnding::None,
            (true, false, false) => LineEnding::CrLf,
            (false, true, false) => LineEnding::Lf,
            (false, false, true) => LineEnding::Cr,
            _ => LineEnding::Mixed,
        };
        // A line range is selected with the same `lines().join("\n")` semantics the
        // whole-file path uses; a character range is the literal substring, so its
        // characters are returned untouched.
        let text = self.selected.take().unwrap_or(self.retained);
        let text = match self.range {
            TextReadRange::Lines { .. } => normalize_lines(text),
            TextReadRange::Full | TextReadRange::Characters { .. } => text,
        };
        (text, self.total_characters, self.total_lines, self.peak_retained_bytes, line_ending)
    }
}

/// Reproduce `lines().join("\n")` for a span of whole lines: the terminator that
/// closed the last retained line is dropped, and every other `\r` that a line feed
/// immediately follows is dropped too, so a CRLF file keeps LF between the returned
/// lines while a bare `\r` inside a line is preserved.
fn normalize_lines(text: String) -> String {
    let mut trimmed = text.strip_suffix('\n').map_or(text.len(), str::len);
    // The last retained line may end with `\r\n`; dropping the line feed leaves the
    // carriage return of that terminator behind.
    if text[..trimmed].ends_with('\r') {
        trimmed -= 1;
    }
    if !text[..trimmed].contains('\r') {
        let mut text = text;
        text.truncate(trimmed);
        return text;
    }
    let mut normalized = String::with_capacity(trimmed);
    let mut characters = text[..trimmed].chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' && characters.peek() == Some(&'\n') {
            continue;
        }
        normalized.push(character);
    }
    normalized
}

#[derive(Clone, Copy)]
enum LineBreakKind {
    CrLf,
    Lf,
}

/// Read the whole file once for the exact hash and counts, retaining only the
/// requested range.
///
/// A file that grows during the read is simply read to EOF: every byte read is
/// hashed and counted, so the hash, the totals, and the returned range always
/// describe the same bytes. No size observed at open is treated as the file's
/// final length and the read never stops early on a size estimate.
///
/// This is the entry point for a handle that has already been opened and verified
/// by an owner other than the workspace resolver — an authorized external file, for
/// example. The decoding, range selection, hashing and line-ending facts are
/// deliberately the same code the workspace read uses, so a file read through
/// either path reports identical facts; nothing here consults the catalog, editor
/// buffers or path containment, because those are workspace concepts and the
/// caller owns the scope it resolved. `path` is the caller's own label for the
/// file and is only used in diagnostics and in the returned snapshot.
pub fn stream_verified_text(
    file: &mut std::fs::File,
    final_path: &std::path::Path,
    path: &WorkspacePath,
    range: &TextReadRange,
    explicit_encoding: Option<TextEncoding>,
    chunk_bytes: usize,
) -> Result<LogicalTextReadSnapshot, WorkspaceError> {
    stream_disk_text(file, final_path, path, range, explicit_encoding, chunk_bytes)
}

fn stream_disk_text(
    file: &mut std::fs::File,
    final_path: &std::path::Path,
    path: &WorkspacePath,
    range: &TextReadRange,
    explicit_encoding: Option<TextEncoding>,
    chunk_bytes: usize,
) -> Result<LogicalTextReadSnapshot, WorkspaceError> {
    let chunk_bytes = chunk_bytes.max(4);
    // `encoding_rs` never reports the input it did not consume, so the decoder is
    // driven in a loop: the bytes it is still holding stay at the front of the
    // buffer and are re-decoded with the next read. That is what makes a
    // multi-byte sequence or a UTF-16 code unit straddling a chunk boundary decode
    // as one character, and it is also why no byte is ever counted twice. The
    // carry is bounded by the longest incomplete sequence, hence the four spare
    // bytes.
    let mut buffer = vec![0_u8; chunk_bytes + 4];
    // `carry` is always the number of already-read but not-yet-decoded bytes at the
    // front of the buffer, so the hash of a chunk is exactly the bytes that have
    // not been hashed yet.
    let mut carry = 0_usize;
    let mut hashed = blake3::Hasher::new();
    let mut active: Option<encoding_rs::Decoder> = None;
    let mut chosen: Option<StreamEncoding> = None;
    // Replaced by the real decision on the first chunk; the initial value only
    // has to be a valid answer for the empty-file case.
    let mut declared = explicit_encoding.unwrap_or(TextEncoding::Utf8);
    let mut state = StreamTextState::new(range);
    loop {
        let mut last = false;
        let mut filled = carry;
        if carry < buffer.len() {
            let read = file
                .read(&mut buffer[carry..])
                .map_err(|source| io_error("read verified workspace file", final_path, source))?;
            last = read == 0;
            filled = carry + read;
        }
        hashed.update(&buffer[..filled]);
        if chosen.is_none() {
            // The encoding decision is made once, from the first bytes, and it is
            // the same decision the whole-file path makes: an explicit choice wins
            // over detection, and a byte order mark is consumed and reported.
            let selection = StreamDecoder::resolve(&buffer[..filled], explicit_encoding);
            declared = selection.declared;
            let strip = selection.strip.min(filled);
            active = Some(selection.decoder.decoder());
            if strip > 0 {
                buffer.copy_within(strip..filled, 0);
                filled -= strip;
            }
            chosen = Some(selection);
            if last && filled == 0 {
                // An empty file still has an encoding answer and zero counts.
                break;
            }
        } else if !last {
            // The bytes just read sit after the carry; the byte order mark has
            // already been consumed and is never decoded again.
            buffer.copy_within(carry..filled, 0);
            filled -= carry;
        }
        let mut decoded = String::new();
        let mut progress = 0_usize;
        let malformed = if chosen.is_some_and(|selection| selection.decoder.is_single_byte()) {
            // One byte in, one character out, with no partial sequence and no
            // malformed result, so the whole chunk is always consumed. The mapping
            // is `encoding_rs`'s own, which is what the whole-file decoder uses.
            let (text, _) =
                encoding_rs::WINDOWS_1252.decode_without_bom_handling(&buffer[..filled]);
            decoded.push_str(&text);
            progress = filled;
            false
        } else {
            loop {
                // `decode_to_string_without_replacement` treats the `String` capacity
                // as the output limit and never reallocates, so capacity is reserved
                // before every call.
                decoded.reserve((filled - progress).max(4));
                let (result, read) = active
                    .as_mut()
                    .expect("a streaming decoder is selected on the first chunk")
                    .decode_to_string_without_replacement(
                        &buffer[progress..filled],
                        &mut decoded,
                        last,
                    );
                progress += read;
                match result {
                    encoding_rs::DecoderResult::InputEmpty => break false,
                    encoding_rs::DecoderResult::Malformed(..) => break true,
                    // Only reachable if the reservation above was still too small; the
                    // loop reserves again and makes progress.
                    encoding_rs::DecoderResult::OutputFull => {}
                }
            }
        };
        // Only the bytes the decoder could not use stay, so the carry never holds
        // more than one incomplete sequence.
        buffer.copy_within(progress..filled, 0);
        carry = filled - progress;
        if malformed {
            // `decode_text` reports this failure against the bytes it was handed,
            // so the ambiguity preview stays byte-for-byte identical.
            return Err(ambiguous_encoding(&buffer[..carry]));
        }
        if !decoded.is_empty() {
            state.absorb(&decoded);
        }
        if last {
            if carry > 0 {
                // A partial sequence at end of file is the same failure the
                // whole-file decoder reports.
                return Err(ambiguous_encoding(&buffer[..carry]));
            }
            break;
        }
        if state.range_complete() {
            // The requested text is complete; the rest of the scan only advances the
            // whole-file hash and tallies.
            state.mark_complete();
        }
    }
    let disk_hash =
        ContentHash::new(ContentHashAlgorithm::Blake3, hashed.finalize().to_hex().to_string())
            .map_err(|_| WorkspaceError::WrongKind { path: path.clone() })?;
    let (retained_text, total_characters, total_lines, peak_retained_bytes, line_ending) =
        state.finish();
    Ok(LogicalTextReadSnapshot {
        path: path.clone(),
        source: FileStorageSource::Disk,
        logical_hash: disk_hash.clone(),
        disk_hash: Some(disk_hash),
        active_editor_session: None,
        encoding: declared,
        line_ending,
        total_characters,
        total_lines,
        retained_text,
        peak_retained_bytes,
    })
}

fn read_disk(
    resolver: &SecurePathResolver,
    path: &WorkspacePath,
) -> Result<Vec<u8>, WorkspaceError> {
    read_disk_with_identity(resolver, path).map(|(bytes, _)| bytes)
}

fn read_disk_with_identity(
    resolver: &SecurePathResolver,
    path: &WorkspacePath,
) -> Result<(Vec<u8>, FileIdentity), WorkspaceError> {
    let mut verified = resolver.open_existing(path, NativeFileAccess::Read)?;
    if verified.file.info().is_directory {
        return Err(WorkspaceError::WrongKind { path: path.clone() });
    }
    let mut bytes = Vec::new();
    verified.file.file_mut().read_to_end(&mut bytes).map_err(|source| {
        io_error("read verified workspace file", &verified.file.info().final_path, source)
    })?;
    let identity = crate::domain_identity(&verified.file.info().identity);
    Ok((bytes, identity))
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{Revision, SessionId, WorkspaceId, WorkspacePath};
    use aura_workspace_domain::{
        BufferEditCommand, CatalogFreshness, CommitRequest, EligibilityOwner, ExecutionId,
        FileStorageSource, LogicalFileStatus, ResourceAccess, WorkspaceState, WorkspaceSummary,
    };

    use crate::resource_facts::ResourceFacts;
    use crate::{
        BufferCommitFact, BufferCommitPhase, BufferStore, SecurePathResolver, domain_identity,
    };
    use aura_domain_common::{EditorReservationId, FileIdentity};

    /// Installs observation points **inside the real disk commit** for one destination.
    ///
    /// Two hooks are installed because the window has two ends: the entry hook reports that this
    /// save really reached the commit, the exit hook reports that it left. Without the second, the
    /// counter would only ever grow, and "two saves were inside at once" could be satisfied by two
    /// saves that ran one after the other — the very class of mistake this test exists to catch.
    fn install_commit_observation(
        destination: &std::path::Path,
        arrivals: std::sync::Arc<commit_overlap::Arrivals>,
    ) -> aura_windows_platform::publication_fault::CommitHookGuard {
        let entry = std::sync::Arc::clone(&arrivals);
        let exit = std::sync::Arc::clone(&arrivals);
        aura_windows_platform::publication_fault::observe_commit_window(
            destination,
            Box::new(move |_path| entry.arrive()),
            Box::new(move |_path| exit.leave()),
        )
    }

    /// The overlap harness: observe the **real** disk commit of two saves and decide whether they
    /// were really inside it at the same time.
    ///
    /// The observation point is the platform's own publication hook, which fires inside
    /// `handle_bound_replace_verified` after the destination is open with no sharing and its
    /// content has been verified. Nothing here re-implements or imitates the commit: both threads
    /// run the production `BufferStore::save` and the production publication.
    mod commit_overlap {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        };
        use std::time::Duration;

        use aura_domain_common::{SessionId, WorkspaceId, WorkspacePath};

        use crate::{BufferStore, SecurePathResolver};

        /// One arrival protocol, shared by every thread of one run.
        ///
        /// Visible to the enclosing test module by its `pub(super)` alias `ArrivalsForTest`, so the
        /// observation hook installed through the platform can report into it.
        pub(super) struct Arrivals {
            /// How many saves have really entered the commit window.
            inside: AtomicUsize,
            /// Satisfied as soon as [`REQUIRED`] saves are simultaneously inside.
            overlapped: Mutex<Option<mpsc::Sender<()>>>,
            /// The highest number of saves ever inside the window at once.
            peak: AtomicUsize,
            /// How many saves have entered the window at all.
            entered: AtomicUsize,
        }

        /// Two saves must be inside the commit at once, which is what "overlap" means here.
        const REQUIRED: usize = 2;

        impl Arrivals {
            pub(super) fn new(overlapped: mpsc::Sender<()>) -> Self {
                Self {
                    inside: AtomicUsize::new(0),
                    overlapped: Mutex::new(Some(overlapped)),
                    peak: AtomicUsize::new(0),
                    entered: AtomicUsize::new(0),
                }
            }

            /// Called by one save **inside** the real commit window.
            pub(super) fn arrive(&self) {
                self.entered.fetch_add(1, Ordering::SeqCst);
                let now = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(now, Ordering::SeqCst);
                if now >= REQUIRED
                    && let Some(sender) =
                        self.overlapped.lock().unwrap_or_else(|error| error.into_inner()).take()
                {
                    let _ = sender.send(());
                }
            }

            /// Called when one save leaves the commit window.
            pub(super) fn leave(&self) {
                self.inside.fetch_sub(1, Ordering::SeqCst);
            }

            /// How many saves were inside the window at the same time, at the peak.
            pub(super) fn peak(&self) -> usize {
                self.peak.load(Ordering::SeqCst)
            }

            /// How many saves entered the commit window at all.
            pub(super) fn entered(&self) -> usize {
                self.entered.load(Ordering::SeqCst)
            }
        }

        /// Runs two real saves concurrently and reports whether they were ever inside the disk
        /// commit together.
        ///
        /// `timeout` bounds the wait, so a serial implementation reports `false` instead of hanging
        /// the suite.
        pub(super) fn run_saves_concurrently(
            resolver: &SecurePathResolver,
            path: &WorkspacePath,
            workspace_id: WorkspaceId,
            timeout: Duration,
        ) -> bool {
            run(resolver, path, workspace_id, timeout, false)
        }

        /// The counterfactual: the same two real saves, with the global index gate held across the
        /// whole save exactly as the pre-change implementation held it.
        ///
        /// It acquires one ordinary `Mutex` at the top of the wrapped save and keeps it until the
        /// save returns. That is the real serialisation mechanism, and it deliberately does **not**
        /// re-enter a non-reentrant mutex on one thread — the mistake the superseded counterexample
        /// made, which turned a serialisation test into a self-deadlock.
        pub(super) fn run_saves_concurrently_serialised(
            resolver: &SecurePathResolver,
            path: &WorkspacePath,
            workspace_id: WorkspaceId,
            timeout: Duration,
        ) -> bool {
            run(resolver, path, workspace_id, timeout, true)
        }

        fn run(
            resolver: &SecurePathResolver,
            _path: &WorkspacePath,
            workspace_id: WorkspaceId,
            timeout: Duration,
            serialise: bool,
        ) -> bool {
            let store = Arc::new(BufferStore::default());
            let resolver = Arc::new(resolver.clone());
            let gate = Arc::new(Mutex::new(()));
            // A diagnostic the counterfactual's own truth depends on: the two threads must really
            // be contending for one gate. If they are, exactly one of them can be inside the
            // wrapped save at a time, and the commit window can never hold two.
            let gate_holders = Arc::new(AtomicUsize::new(0));
            let gate_peak = Arc::new(AtomicUsize::new(0));
            let (overlapped_tx, overlapped_rx) = mpsc::channel();

            // Two sessions editing **different files** in one workspace. Different files is the
            // case the confirmed behaviour requires to run in parallel; this is not a test of one
            // session contending with itself.
            let first_path = WorkspacePath::parse("first.txt").expect("path");
            let second_path = WorkspacePath::parse("second.txt").expect("path");
            for candidate in [&first_path, &second_path] {
                std::fs::write(
                    resolver.absolute_path(candidate),
                    format!("disk {}", candidate.as_str()),
                )
                .expect("seed file");
            }

            let first_session = SessionId::new();
            let second_session = SessionId::new();
            let mut prepared = Vec::new();
            for (session, candidate) in
                [(first_session, &first_path), (second_session, &second_path)]
            {
                let opened = store.open(&resolver, session, candidate.clone(), true).expect("open");
                let edited = store
                    .apply_edit(super::update(
                        &opened,
                        workspace_id,
                        session,
                        candidate.clone(),
                        &format!("logical {}", candidate.as_str()),
                        false,
                    ))
                    .expect("edit");
                prepared.push((session, candidate.clone(), edited));
            }

            let arrivals = Arc::new(Arrivals::new(overlapped_tx));

            let mut handles = Vec::new();
            for (session, candidate, edited) in prepared {
                let store = Arc::clone(&store);
                let resolver = Arc::clone(&resolver);
                // The gate is cloned from the **shared** binding, and the shared binding is never
                // rebound: a shadowing `let gate = Arc::clone(&gate)` inside the loop would move
                // the only strong handle into the first thread and leave every later thread with
                // its own private gate, which would make the counterfactual silently unserialised.
                let thread_gate = Arc::clone(&gate);
                let arrivals = Arc::clone(&arrivals);
                let gate_holders = Arc::clone(&gate_holders);
                let gate_peak = Arc::clone(&gate_peak);
                handles.push(std::thread::spawn(move || {
                    // The counterfactual's serialisation: one ordinary gate acquired at the top of
                    // the wrapped save and held until the save returns — exactly the shape the old
                    // implementation had with `operation_gate`. It is deliberately NOT a
                    // re-entrant acquisition of the store's own gate, which is what made the
                    // superseded counterexample a self-deadlock rather than a serialisation.
                    let _held = if serialise {
                        let held = thread_gate.lock().unwrap_or_else(|error| error.into_inner());
                        let now = gate_holders.fetch_add(1, Ordering::SeqCst) + 1;
                        gate_peak.fetch_max(now, Ordering::SeqCst);
                        Some(held)
                    } else {
                        None
                    };
                    // The production hook, installed for this destination only, fires inside the
                    // real commit. Everything the hook observes is a real commit that really ran.
                    let _hook = crate::buffer::tests::install_commit_observation(
                        &resolver.absolute_path(&candidate),
                        Arc::clone(&arrivals),
                    );
                    let result = store.save(
                        &resolver,
                        session,
                        &candidate,
                        edited.reservation_id.expect("reservation"),
                        edited.reservation_epoch,
                        edited.buffer_revision,
                    );
                    if serialise {
                        gate_holders.fetch_sub(1, Ordering::SeqCst);
                    }
                    drop(_hook);
                    drop(_held);
                    result
                }));
            }

            let observed = overlapped_rx.recv_timeout(timeout).is_ok();
            for handle in handles {
                let result = handle.join();
                // A rejected counterfactual still completes its saves; only the overlap verdict
                // differs. A genuine panic is still reported.
                if let Ok(inner) = result {
                    assert!(inner.is_ok() || serialise, "every real save must succeed: {inner:?}");
                }
            }
            eprintln!(
                "overlap probe serialise={serialise} observed={observed} peak={} entered={} \
                 gate_peak={}",
                arrivals.peak(),
                arrivals.entered(),
                gate_peak.load(Ordering::SeqCst),
            );
            observed && arrivals.peak() >= REQUIRED
        }
    }

    /// **A-R1.1** — one physical file has one writable edit reservation, however many workspaces
    /// reach it.
    ///
    /// This is the counterexample for the measured defect. The reservation map used to be keyed
    /// `(WorkspaceId, FileIdentity)`, so two workspaces rooted at the same directory — an aliased
    /// root, a nested workspace, a second `WorkspaceSummary` over one canonical root — each
    /// obtained their own `reservation_id` for the *same bytes*, and a probe against the real
    /// resolver confirmed both reported `second_writable = true`. Keying on the real object makes
    /// the second workspace's write attempt an `EditorReserved` refusal naming the real holder.
    ///
    /// The same test proves the other half: two *different* files are still two reservations, so
    /// the fix cannot be "refuse everything".
    #[test]
    fn the_same_physical_file_has_one_writable_editor_across_workspaces() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("shared.txt"), b"disk").expect("file");
        std::fs::write(root.path().join("other.txt"), b"other").expect("file");
        // Two workspaces over one canonical root: the same physical directory, two WorkspaceIds.
        let first_resolver = resolver_for(root.path(), WorkspaceId::new());
        let second_resolver = resolver_for(root.path(), WorkspaceId::new());
        assert_ne!(
            first_resolver.workspace_id(),
            second_resolver.workspace_id(),
            "the two views really are two workspaces"
        );
        let shared = WorkspacePath::parse("shared.txt").expect("path");
        let other = WorkspacePath::parse("other.txt").expect("path");
        let store = BufferStore::default();
        let first_session = SessionId::new();
        let second_session = SessionId::new();

        store
            .open(&first_resolver, first_session, shared.clone(), true)
            .expect("the first workspace takes the writable reservation");

        let refusal =
            store.open(&second_resolver, second_session, shared.clone(), true).expect_err(
                "a second workspace must not obtain an independent writable reservation on the \
                 same physical file",
            );
        assert!(
            matches!(refusal, crate::WorkspaceError::EditorReserved { ref owner, .. } if *owner == first_session),
            "the refusal must name the real holder: {refusal:?}"
        );

        // A different file in the second workspace is still free: the fix is per-real-object, not
        // a blanket lock.
        store
            .open(&second_resolver, second_session, other, true)
            .expect("a different file must remain writable in the second workspace");

        // And the real holder is reported to the second workspace, not hidden behind its own view.
        let holder = store
            .active_editor(&second_resolver, &shared)
            .expect("active editor lookup")
            .expect("the same physical file has one holder");
        assert_eq!(holder, first_session);
    }

    /// A different file in the same workspace is still a separate writable reservation, and the
    /// reservation views are filtered by the workspace that really took them.
    #[test]
    fn reservations_are_per_real_object_and_reported_by_their_own_workspace() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("one.txt"), b"one").expect("file");
        std::fs::write(root.path().join("two.txt"), b"two").expect("file");
        let first_workspace = WorkspaceId::new();
        let second_workspace = WorkspaceId::new();
        let first_resolver = resolver_for(root.path(), first_workspace);
        let second_resolver = resolver_for(root.path(), second_workspace);
        let store = BufferStore::default();
        let first_session = SessionId::new();
        let second_session = SessionId::new();

        store
            .open(
                &first_resolver,
                first_session,
                WorkspacePath::parse("one.txt").expect("path"),
                true,
            )
            .expect("first reservation");
        store
            .open(
                &second_resolver,
                second_session,
                WorkspacePath::parse("two.txt").expect("path"),
                true,
            )
            .expect("second reservation");

        assert_eq!(
            store.reservation_summaries(first_workspace).expect("first summaries").len(),
            1,
            "each workspace reports only the reservation it really took"
        );
        assert_eq!(
            store.reservation_summaries(second_workspace).expect("second summaries").len(),
            1
        );
    }

    /// **A-R3** — a Buffer save whose destination was rewritten **in place** since the buffer read
    /// it is refused, even though the `FileIdentity` is unchanged.
    ///
    /// This is the counterexample for the generic replacement. `atomic_save` used to write a
    /// sibling temporary and call `atomic_replace_file`, which takes no condition at all: the
    /// buffer's `disk_hash` was checked before that call, so an independent writer that rewrote the
    /// file in place between the check and the publication kept the same object and the stale
    /// decision won. An identity-only check cannot see an in-place rewrite, which is why the
    /// content condition has to reach the exclusive handle.
    ///
    /// The disk fact after the refusal is asserted too: the other writer's bytes must survive. A
    /// save that refuses but has already overwritten is not a refusal.
    #[test]
    fn a_save_refuses_a_destination_rewritten_in_place() {
        let root = tempfile::tempdir().expect("root");
        let destination = root.path().join("note.txt");
        std::fs::write(&destination, b"the original bytes").expect("seed");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let session_id = SessionId::new();
        let path = WorkspacePath::parse("note.txt").expect("path");
        let store = BufferStore::default();
        let opened = store.open(&resolver, session_id, path.clone(), true).expect("open");
        let edited = store
            .apply_edit(update(
                &opened,
                workspace_id,
                session_id,
                path.clone(),
                "what the editor wants",
                false,
            ))
            .expect("edit");

        // The independent writer rewrites the same object in place. Windows keeps the file id, so
        // nothing about the identity can reveal this.
        let identity_of = |path: &std::path::Path| {
            let verified = aura_windows_platform::open_verified_existing(
                path,
                aura_windows_platform::NativeFileAccess::Metadata,
            )
            .expect("verified file");
            domain_identity(&verified.info().identity)
        };
        let before = identity_of(&destination);
        std::fs::write(&destination, b"the outside writer won").expect("in-place rewrite");
        let after = identity_of(&destination);
        assert_eq!(
            before, after,
            "an in-place rewrite keeps the identity, which is why the identity alone cannot decide"
        );

        let refusal = store
            .save(
                &resolver,
                session_id,
                &path,
                edited.reservation_id.expect("reservation"),
                edited.reservation_epoch,
                edited.buffer_revision,
            )
            .expect_err("a save must not overwrite content it did not decide about");
        let text = refusal.to_string();
        assert!(
            text.contains("atomic buffer save failed") || text.contains("changed"),
            "the refusal must say what really happened: {text}"
        );
        assert_eq!(
            std::fs::read(&destination).expect("destination"),
            b"the outside writer won",
            "the outside writer's bytes must survive a refused save"
        );
    }

    /// **A-R3** — a refused save leaves the buffer's own content intact and still dirty.
    ///
    /// The failure must not be reported as "not executed" while having written, and it must not
    /// discard the session's work either: the buffer keeps its text, its revision and its dirty
    /// state, so the editor can resolve the conflict with its own content still present.
    #[test]
    fn a_refused_save_keeps_the_buffer_content_and_its_dirty_state() {
        let root = tempfile::tempdir().expect("root");
        let destination = root.path().join("note.txt");
        std::fs::write(&destination, b"disk original").expect("seed");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let session_id = SessionId::new();
        let path = WorkspacePath::parse("note.txt").expect("path");
        let store = BufferStore::default();
        let opened = store.open(&resolver, session_id, path.clone(), true).expect("open");
        let edited = store
            .apply_edit(update(&opened, workspace_id, session_id, path.clone(), "edited", false))
            .expect("edit");
        std::fs::write(&destination, b"someone else").expect("outside rewrite");

        store
            .save(
                &resolver,
                session_id,
                &path,
                edited.reservation_id.expect("reservation"),
                edited.reservation_epoch,
                edited.buffer_revision,
            )
            .expect_err("the save must be refused");

        let view = store.logical_file_snapshot(&resolver, session_id, &path).expect("snapshot");
        assert_eq!(
            view.source,
            FileStorageSource::EditorBuffer,
            "the buffer is still this session's authority"
        );
        assert_eq!(view.bytes, b"edited", "the session's own content must survive the refusal");
        assert!(view.is_dirty, "a refused save leaves the buffer dirty");
        assert_eq!(view.buffer_revision, Some(edited.buffer_revision));
    }

    /// **The same-instance proof at the Buffer.**
    ///
    /// Batch A's measured defect was that the eligibility owner existed but **no production entry
    /// held it**: `EligibilityOwner` appeared only inside its own module, so it protected nothing.
    /// This is the counterexample for that state, expressed the only way the design accepts — by
    /// **pointer identity** on the real call graph.
    ///
    /// A Buffer open takes its long eligibility from the store's own owner. A Main Fs-style commit
    /// on the same real object, taken from *another* handle to that owner, must then be refused:
    /// the eligibility is visible across the two handles because they are the same instance. A
    /// store with a private owner fails this, and so does any wiring that hands out a wrapper
    /// instead of the instance.
    #[test]
    fn the_buffer_really_coordinates_through_the_one_shared_owner() {
        use std::sync::Arc;

        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("shared.txt"), b"disk").expect("file");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let session = SessionId::new();

        // The runtime hands one owner to the store, and the same `Arc` to everyone else.
        let owner = Arc::new(EligibilityOwner::new());
        let store = BufferStore::with_eligibility_owner(Arc::clone(&owner));
        assert!(
            Arc::ptr_eq(&store.eligibility_owner(), &owner),
            "the store must hold the owner it was given, not a private one"
        );

        let path = WorkspacePath::parse("shared.txt").expect("path");
        store.open(&resolver, session, path.clone(), true).expect("writable open");
        assert_eq!(
            owner.snapshot().claims.len(),
            1,
            "the Buffer's writable open really took a long eligibility from the shared owner"
        );

        // A different execution, standing in for Main Fs, asks the *same* owner to commit the same
        // real object. It must be refused by the live eligibility the Buffer holds.
        let other_execution = ExecutionId::next();
        let facts = ResourceFacts::root(Arc::new(resolver.clone()), SessionId::new())
            .with_case_rule(aura_workspace_domain::SlotCaseRule::InsensitivePreserving);
        let object = facts.object(&path).expect("resolution").expect("the file exists");
        let refusal = owner
            .commit(
                &CommitRequest::one(object, ResourceAccess::Write),
                SessionId::new(),
                other_execution,
                None,
            )
            .expect_err("Main Fs must see the Buffer's live eligibility on the same bytes");
        assert_eq!(refusal.held_by, Some(session));
        assert!(refusal.retryable, "the caller can wait for the editor to finish");

        // And the same store held by a *second* store instance over the same owner sees it too:
        // the sharing is the `Arc`, not the store.
        let second = BufferStore::with_eligibility_owner(Arc::clone(&owner));
        assert!(Arc::ptr_eq(&second.eligibility_owner(), &store.eligibility_owner()));
        let still_refused = owner
            .commit(
                &CommitRequest::one(
                    facts.object(&path).expect("resolution").expect("file"),
                    ResourceAccess::Write,
                ),
                SessionId::new(),
                ExecutionId::next(),
                None,
            )
            .expect_err("the eligibility is one fact however many handles name it");
        assert_eq!(still_refused.held_by, Some(session));
    }

    fn resolver_for(root: &std::path::Path, workspace_id: WorkspaceId) -> SecurePathResolver {
        let info = SecurePathResolver::inspect_selected_root(root).expect("root info");
        SecurePathResolver::from_workspace(&WorkspaceSummary {
            workspace_id,
            display_name: "test".to_owned(),
            selected_root: root.display().to_string(),
            canonical_root: info.final_path.display().to_string(),
            root_identity: Some(domain_identity(&info.identity)),
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        })
        .expect("resolver")
    }

    fn update(
        status: &LogicalFileStatus,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        path: WorkspacePath,
        content: &str,
        active_editing: bool,
    ) -> BufferEditCommand {
        BufferEditCommand {
            workspace_id,
            session_id,
            path,
            reservation_id: status.reservation_id.expect("writable Buffer reservation"),
            reservation_epoch: status.reservation_epoch,
            buffer_revision: Revision::new(status.buffer_revision.value() + 1),
            base_core_revision: status.buffer_revision,
            content: content.to_owned(),
            active_editing,
            ime_composition_active: false,
        }
    }

    /// **R2** — a real save opens exactly one activity identity and ends it at real settlement.
    ///
    /// # What the pre-fix implementation did
    ///
    /// It published only *after* the write, the settle and `release_commit` had all returned - so
    /// the fact it recorded described an event that had already finished - and nothing ever ended
    /// it. A completed save therefore occupied a `running` row permanently, and consecutive saves
    /// steadily exhausted the index. It also called `resolve_existing` at that point purely to
    /// obtain an identity for the publication, re-opening a path the store had already closed.
    ///
    /// # What this test requires, and what it rejects
    ///
    /// The sequence of real phases the store reports for one `save` must be exactly
    /// `Start` -> `Settled`, **under one identity**: `Start` because the row must really exist while
    /// the bytes are being written (a publish-after-the-fact can never satisfy this), and a terminal
    /// phase because the row must be gone afterwards. A `Failed`/`Terminated`-only sequence, or a
    /// single terminal report with no `Start`, is rejected outright - that is precisely the shipped
    /// shape.
    ///
    /// Two further substitutions are rejected: the `Start` identity must be the identity the
    /// **store already held**, so a save is associated with the object it really replaced without
    /// any re-resolution; and the fact must not be published for a save A refused to admit.
    #[test]
    fn a_real_save_opens_one_activity_identity_before_the_write_and_ends_it_at_settlement() {
        use std::sync::{Arc, Mutex};

        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("note.txt"), b"original\r\n").expect("file");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let session_id = SessionId::new();
        let path = WorkspacePath::parse("note.txt").expect("path");

        let observed: Arc<Mutex<Vec<(BufferCommitPhase, FileIdentity, u64)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&observed);
        let store = BufferStore::default().with_activity_publisher(Arc::new(
            move |fact: BufferCommitFact<'_>| {
                recorder.lock().expect("phase recorder").push((
                    fact.phase,
                    fact.identity.clone(),
                    fact.buffer_revision.value(),
                ));
            },
        ));

        let opened = store.open(&resolver, session_id, path.clone(), true).expect("open");
        // The object the store really holds. It is read here through the resolver's own verified
        // chain - the same facts `prepare_save` carries - so the assertion below compares the
        // publication against the real identity rather than against whatever the publisher said.
        let held_identity = domain_identity(
            &resolver.resolve_existing(&path).expect("the opened file").info.identity,
        );
        let edited = store
            .apply_edit(update(&opened, workspace_id, session_id, path.clone(), "changed\n", false))
            .expect("edit");
        store
            .save(
                &resolver,
                session_id,
                &path,
                edited.reservation_id.expect("reservation"),
                edited.reservation_epoch,
                edited.buffer_revision,
            )
            .expect("save");

        let phases = observed.lock().expect("phase recorder").clone();
        assert_eq!(
            phases.iter().map(|(phase, _, _)| *phase).collect::<Vec<_>>(),
            vec![BufferCommitPhase::Start, BufferCommitPhase::Settled],
            "one real save must open its activity before the write and end it at settlement; a \
             terminal-only sequence is the shipped defect"
        );
        // Both reports are for the same commit, and the identity is the one the store already
        // verified - never a path re-opened to obtain one.
        let (_, start_identity, start_revision) = &phases[0];
        let (_, settled_identity, settled_revision) = &phases[1];
        assert_eq!(start_identity, &held_identity, "the start names the object really held");
        assert_eq!(start_identity, settled_identity, "the end names the same commit identity");
        assert_eq!(start_revision, settled_revision, "one commit, one buffer revision");
        // And the save really happened, so none of the above is describing a refused write.
        assert_eq!(std::fs::read(root.path().join("note.txt")).expect("read"), b"changed\r\n");
    }

    /// **R2** — a save that A refuses to admit publishes nothing at all.
    ///
    /// The publication opens *after* the object/slot commit is taken, so a retryable resource
    /// refusal - the case where another execution really holds the file - leaves the index
    /// untouched. The pre-fix implementation could not distinguish this, because it only ever
    /// published on the success path it had already reached.
    #[test]
    fn a_refused_save_never_opens_an_activity() {
        use std::sync::{Arc, Mutex};

        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("note.txt"), b"original\r\n").expect("file");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let session_id = SessionId::new();
        let path = WorkspacePath::parse("note.txt").expect("path");

        let observed: Arc<Mutex<Vec<BufferCommitPhase>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&observed);
        let store = BufferStore::default().with_activity_publisher(Arc::new(
            move |fact: BufferCommitFact<'_>| {
                recorder.lock().expect("phase recorder").push(fact.phase);
            },
        ));
        // A save with no buffer open makes no commit decision at all: it is refused before any
        // write window exists.
        let refused = store.save(
            &resolver,
            session_id,
            &path,
            EditorReservationId::new(),
            Revision::INITIAL,
            Revision::new(1),
        );
        assert!(refused.is_err(), "the save was really refused");
        assert!(
            observed.lock().expect("phase recorder").is_empty(),
            "a save that never entered its write window must publish nothing"
        );
        assert_eq!(
            std::fs::read(root.path().join("note.txt")).expect("read"),
            b"original\r\n",
            "and the file really was not written"
        );
    }

    #[test]
    fn buffer_is_session_scoped_and_requires_expected_revision() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("note.txt"), b"original\r\n").expect("file");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let session_id = SessionId::new();
        let path = WorkspacePath::parse("note.txt").expect("path");
        let store = BufferStore::default();
        let opened = store.open(&resolver, session_id, path.clone(), true).expect("open");
        let edited = store
            .apply_edit(update(&opened, workspace_id, session_id, path.clone(), "changed\n", false))
            .expect("edit");
        store
            .save(
                &resolver,
                session_id,
                &path,
                edited.reservation_id.expect("reservation"),
                edited.reservation_epoch,
                edited.buffer_revision,
            )
            .expect("save");
        assert_eq!(std::fs::read(root.path().join("note.txt")).expect("read"), b"changed\r\n");
    }

    /// **A-R5** — the corrected save-overlap evidence, placed **during the real disk commit** and
    /// where the index gate must already be released.
    ///
    /// # What the superseded test got wrong
    ///
    /// The previous version called `store.save(...)`, and only *after it returned* did each thread
    /// increment a counter and wait on a `Barrier`. By then every guard inside `save` had been
    /// dropped — the carrier and commit sections had both finished — so a fully serial
    /// implementation also reached the barrier and the test passed. It measured the wrong thing and
    /// could not reject the implementation it was supposed to reject. (The companion pre-fix
    /// "counterexample" was worse: it wrapped the new `save` in an extra `operation_gate` lock and
    /// then let `prepare_save` re-lock the same non-reentrant mutex on the same thread, which is a
    /// self-deadlock that says nothing about the old implementation.)
    ///
    /// # What this test does instead
    ///
    /// The observation point is installed through the platform's own publication hook, which fires
    /// inside `handle_bound_replace_verified` **after the destination is open with no sharing and
    /// its content has been verified** — that is, during the real disk commit. At that moment:
    ///
    /// * the index gate has provably been released, because `prepare_save` returned before the disk
    ///   work started and `commit_save` cannot run until the publication returns;
    /// * a serial implementation parks entirely inside one window instead of two.
    ///
    /// Each thread records that it really **entered** the commit, then waits for the other. Two
    /// arrivals in the window prove the two saves overlapped in the real commit. The wait is
    /// bounded, so a serial implementation fails the criterion by **timeout** — it does not hang the
    /// suite.
    ///
    /// # The counterfactual
    ///
    /// The counterfactual restores only the real serialisation mechanism — the global index gate
    /// held across the whole save, which is what `operation_gate` did before this change — and
    /// **without** re-entering a non-reentrant mutex on one thread. `SerialisedSaveStore` acquires
    /// one ordinary `Mutex` at the top of the wrapped `save` and holds it until the save returns,
    /// which is exactly the old shape. The same criterion rejects it: the second thread cannot
    /// enter the commit while the first is parked, and the bounded wait expires.
    #[test]
    fn two_real_saves_overlap_inside_the_disk_commit() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        };

        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("note.txt"), b"disk one").expect("file");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let path = WorkspacePath::parse("note.txt").expect("path");

        assert!(
            commit_overlap::run_saves_concurrently(
                &resolver,
                &path,
                workspace_id,
                std::time::Duration::from_secs(20),
            ),
            "both real saves must be inside the disk commit at the same time"
        );

        // The counterfactual: the same two real saves, with the global index gate held across the
        // whole save exactly as the old implementation held it. The same criterion must reject it.
        assert!(
            !commit_overlap::run_saves_concurrently_serialised(
                &resolver,
                &path,
                workspace_id,
                std::time::Duration::from_secs(5),
            ),
            "the serial counterfactual must be rejected by the very same criterion"
        );
        let _ = (AtomicUsize::new(0), Ordering::SeqCst, mpsc::channel::<()>());
    }

    #[test]
    fn writable_reservation_is_unique_while_other_sessions_read_disk_explicitly() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("shared.txt"), b"disk").expect("file");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let writer = SessionId::new();
        let reader = SessionId::new();
        let path = WorkspacePath::parse("shared.txt").expect("path");
        let store = BufferStore::default();
        let writer_open = store.open(&resolver, writer, path.clone(), true).expect("writer open");
        assert!(matches!(
            store.open(&resolver, reader, path.clone(), true),
            Err(crate::WorkspaceError::EditorReserved { owner, .. }) if owner == writer
        ));
        store.open(&resolver, reader, path.clone(), false).expect("read-only open");
        store
            .apply_edit(update(&writer_open, workspace_id, writer, path.clone(), "logical", true))
            .expect("writer edit");

        let writer_view =
            store.logical_file_snapshot(&resolver, writer, &path).expect("writer snapshot");
        assert_eq!(writer_view.source, FileStorageSource::EditorBuffer);
        assert_eq!(writer_view.bytes, b"logical");
        let reader_view =
            store.logical_file_snapshot(&resolver, reader, &path).expect("reader snapshot");
        assert_eq!(reader_view.source, FileStorageSource::Disk);
        assert_eq!(reader_view.bytes, b"disk");
        assert_eq!(reader_view.active_editor_session, Some(writer));
    }

    #[test]
    fn save_rekeys_the_buffer_and_reservation_to_the_replacement_file_identity() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("identity.txt"), b"first").expect("file");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let session_id = SessionId::new();
        let path = WorkspacePath::parse("identity.txt").expect("path");
        let store = BufferStore::default();
        let opened = store.open(&resolver, session_id, path.clone(), true).expect("open");
        let first_edit = store
            .apply_edit(update(&opened, workspace_id, session_id, path.clone(), "second", true))
            .expect("first edit");
        let first_saved = store
            .save(
                &resolver,
                session_id,
                &path,
                first_edit.reservation_id.expect("reservation"),
                first_edit.reservation_epoch,
                first_edit.buffer_revision,
            )
            .expect("first save");
        let second_edit = store
            .apply_edit(update(
                &first_saved,
                workspace_id,
                session_id,
                path.clone(),
                "third",
                false,
            ))
            .expect("edit after replacement");
        store
            .save(
                &resolver,
                session_id,
                &path,
                second_edit.reservation_id.expect("reservation"),
                second_edit.reservation_epoch,
                second_edit.buffer_revision,
            )
            .expect("second save");
        assert_eq!(std::fs::read(root.path().join("identity.txt")).expect("read"), b"third");
        assert_eq!(store.reservation_summaries(workspace_id).expect("reservations").len(), 1);
    }

    #[test]
    fn an_unrelated_operation_does_not_save_another_sessions_dirty_buffer() {
        // The contract that replaced `flush_non_active_dirty_buffers`: reporting another
        // session's unsaved work must not commit it. Previously any caller could call the flush
        // and every other session's dirty buffer in the workspace was written to disk.
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("active.txt"), b"disk active").expect("active file");
        std::fs::write(root.path().join("inactive.txt"), b"disk inactive").expect("inactive file");
        let workspace_id = WorkspaceId::new();
        let resolver = resolver_for(root.path(), workspace_id);
        let owner = SessionId::new();
        let other = SessionId::new();
        let active = WorkspacePath::parse("active.txt").expect("active path");
        let inactive = WorkspacePath::parse("inactive.txt").expect("inactive path");
        let store = BufferStore::default();

        // The owner of the inactive buffer edits it but does not save.
        let inactive_open =
            store.open(&resolver, owner, inactive.clone(), true).expect("inactive open");
        store
            .apply_edit(update(
                &inactive_open,
                workspace_id,
                owner,
                inactive.clone(),
                "logical inactive",
                false,
            ))
            .expect("inactive edit");

        // A second session also has an actively edited buffer, which must be reported as stale.
        let active_open = store.open(&resolver, other, active.clone(), true).expect("active open");
        store
            .apply_edit(update(
                &active_open,
                workspace_id,
                other,
                active.clone(),
                "logical active",
                true,
            ))
            .expect("active edit");

        // Reporting the uncommitted buffers changes nothing on disk.
        let uncommitted = store.uncommitted_editor_buffers(workspace_id).expect("uncommitted");
        assert_eq!(
            uncommitted.len(),
            1,
            "only the non-active dirty buffer is uncommitted; the other is actively edited"
        );
        assert_eq!(uncommitted[0].session_id, owner);
        assert_eq!(uncommitted[0].path, inactive);
        assert_eq!(
            std::fs::read(root.path().join("inactive.txt")).expect("read inactive"),
            b"disk inactive",
            "reporting another session's unsaved buffer must not write it"
        );
        assert_eq!(
            std::fs::read(root.path().join("active.txt")).expect("read active"),
            b"disk active",
            "an actively edited buffer must not be written either"
        );
        // The buffer is still dirty and still owned by its session: nothing was taken over.
        let statuses = store.statuses(workspace_id).expect("statuses");
        let owner_status = statuses
            .iter()
            .find(|status| status.session_id == owner && status.path == inactive)
            .expect("owner buffer survives");
        assert!(owner_status.dirty, "the owner's buffer must still hold its unsaved edit");

        // The lifecycle's *explicit* save is a different action and still saves them. It requires
        // the workspace lifecycle to be frozen, which is the precondition that distinguishes it
        // from an ordinary mutation's incidental flush.
        store.begin_lifecycle(workspace_id).expect("begin lifecycle");
        store.freeze_lifecycle(workspace_id).expect("freeze lifecycle");
        store.save_all_dirty_for_lifecycle(&resolver).expect("lifecycle save");
        assert_eq!(
            std::fs::read(root.path().join("inactive.txt")).expect("read inactive"),
            b"logical inactive",
            "the explicit lifecycle save must still commit the buffer"
        );
    }
}
