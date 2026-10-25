//! The one eligibility owner: who may write which real resource, right now.
//!
//! [`crate::resource`] says what a resource *is*. This module is the decision built on it, and it
//! exists because the previous design had the decision spread over three places that could not
//! agree: a `(WorkspaceId, FileIdentity)` reservation map in the Buffer store, a
//! workspace-plus-relative-path text query in the mutation path, and the connection's request
//! lane. Each of them answered a slightly different question, so the same physical file could be
//! writable twice across two workspaces while two unrelated files in one workspace blocked each
//! other.
//!
//! # The two-part decision
//!
//! Batch A separates a **long edit eligibility** from a **short commit**, and requires the two to
//! be decided atomically for one owner:
//!
//! * an [`EditEligibility`] is a long-lived claim that one *holder* is editing one resource. It
//!   survives across many edits and is released explicitly, or when the workspace it belongs to
//!   goes away;
//! * a short commit is the admission taken immediately before a real side effect. It is granted
//!   only if no other **execution** holds a conflicting claim or commit.
//!
//! The atomicity is the point. Checking eligibility and then writing is a time window: between the
//! check and the write, another session can open an edit, a revoke can land, a lifecycle freeze
//! can start, or the object can be replaced so the identity the check was made against no longer
//! names what is on disk. So the commit is taken from the *same* owner that granted the claim, in
//! one lock, and it re-states the resource it was granted for.
//!
//! # Execution identity, not session identity
//!
//! The first version of this owner compared `SessionId`s to decide whether a holder was "self",
//! and skipped its own session. That is wrong in both directions and was measured wrong: one
//! session's *two* executions were treated as one owner, so the second commit on an object was
//! admitted while the first was still in flight, and releasing the first record silently released
//! the second. A session is a *billing and lifecycle* concept; an execution is what actually
//! performs a side effect, and the user's confirmed rule is that **two tools in the same session
//! must obey the same file-conflict protection**. So every commit and every claim is owned by an
//! [`ExecutionId`], the same session is **not** exempt from exclusivity, and a release ends
//! exactly the record it names.
//!
//! # What this module does not do
//!
//! It performs no I/O, spawns nothing and holds no permit. Waiting for admission is the caller's,
//! on its own budget, and is deliberately *not* implemented as a blocking lock here: batch A
//! requires that a wait for full resource admission does not consume the connection's synchronous
//! permits or the global tool-execution budget. A waiter therefore asks
//! [`EligibilityOwner::try_admit`], gets a typed [`AdmissionRefusal`] that says whether the holder
//! is still live, and retries on its own deadline.
//!
//! # Fault seams
//!
//! [`fault`] carries named, default-off seams. They exist so a directed test can reach a real
//! stage — the arithmetic overflow guard, or a refusal injected exactly at the point where the
//! owner would admit — without re-implementing the owner. Nothing is armed unless a caller arms
//! it, and a seam never fabricates a fact: an injection makes the owner refuse, which is a real
//! outcome the caller must handle.
use std::{
    collections::HashMap,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use aura_domain_common::{FileIdentity, Revision, SessionId, WorkspaceId, WorkspacePath};
use serde::{Deserialize, Serialize};

use crate::resource::{
    ConflictReason, FileResource, ResourceAccess, ResourceRequest, conflict_between,
};

/// One execution: the thing that really performs a side effect.
///
/// It is deliberately *not* a session. The user's confirmed rule is that two tools inside one
/// session obey the same file-conflict protection, so a session id cannot be the unit of
/// exclusivity. It is also not an operation id: the same operation may be retried as a new
/// execution, and the retry must wait for the first rather than inherit its permission.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExecutionId(u64);

impl ExecutionId {
    /// The next execution id from a process-wide monotonic counter.
    ///
    /// Monotonic and never reused, so an id that has been released can never be adopted by a later
    /// caller — which is what makes "the claim I hold is the same claim" checkable rather than
    /// hopeful.
    #[must_use]
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// The id's numeric value, for reporting and for persistence keys.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ExecutionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "exe_{:016x}", self.0)
    }
}

/// A long-lived edit eligibility on one real resource.
///
/// The eligibility is identified by [`EditEligibility::claim_id`], which is what a caller presents
/// at commit time. Presenting the resource instead would re-derive the key from a path that may
/// since have been replaced, which is the check-then-write window this replaces.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditEligibility {
    pub claim_id: EditClaimId,
    /// The resources this eligibility covers, each at its own access.
    ///
    /// Almost always one object. A Buffer's claim names **two**: the object the editor will replace
    /// and the directory entry that will name the replacement, because an atomic save produces a new
    /// object identity and a claim on the old object alone would leave the name free for exactly the
    /// window the batch forbids.
    ///
    /// [`Self::resource`] is the first of these, which is the object a replacement changes.
    pub resources: Vec<ResourceRequest>,
    pub session_id: SessionId,
    /// The workspace this eligibility was *taken through*. Ownership only: it never splits one
    /// physical object into two eligibilities, and it is what a workspace lifecycle releases.
    pub workspace_id: WorkspaceId,
    /// The execution that holds it. Two executions of one session are two holders.
    pub execution_id: ExecutionId,
    /// Which Buffer generation this eligibility agrees with, when it belongs to a Buffer. `None`
    /// for entries that hold no Buffer generation, which is every non-Buffer caller.
    pub buffer_generation: Option<Revision>,
    /// Bumped on every accepted commit and on every identity migration, so a caller can tell "the
    /// eligibility I hold is the same eligibility" from "an eligibility exists on a resource I
    /// once named".
    pub epoch: Revision,
}

/// A stable id for one long edit eligibility.
pub type EditClaimId = String;

impl EditEligibility {
    /// The object this eligibility is fundamentally about: the resource a replacement changes.
    ///
    /// A claim always names at least one resource, so this is total for a claim the owner built.
    #[must_use]
    pub fn resource(&self) -> &FileResource {
        &self
            .resources
            .first()
            .expect("an edit eligibility always names at least one resource")
            .resource
    }

    /// The object's identity, when the claim's primary resource is an object.
    #[must_use]
    pub fn object_identity(&self) -> Option<&FileIdentity> {
        match self.resource() {
            FileResource::Object { identity, .. } => Some(identity),
            _ => None,
        }
    }
}

/// Why an admission was refused.
///
/// Every refusal names the holder and the real conflict, because the user-facing behaviour is that
/// a conflict is *reported* — never silently resolved by taking the other session's work over.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRefusal {
    pub reason: ConflictReason,
    pub requested: String,
    pub held: String,
    /// Which session holds the conflicting record. `None` for a conflict with a live execution
    /// that holds no session of its own, such as a commit already in flight from a caller that
    /// had no session to name.
    pub held_by: Option<SessionId>,
    /// The execution that holds it, so a caller can tell its own two executions apart.
    pub held_by_execution: Option<ExecutionId>,
    /// Whether the conflicting holder is still live. A caller uses this to decide between waiting
    /// and reporting; a released holder is never reported as a conflict.
    pub retryable: bool,
}

impl AdmissionRefusal {
    /// Whether the refusal names *this* execution, which is never a conflict: an execution may
    /// always re-enter a record it already owns.
    #[must_use]
    pub fn is_self(&self, execution_id: ExecutionId) -> bool {
        self.held_by_execution == Some(execution_id)
    }
}

/// A short commit: proof that the complete resource set was free at the moment the caller asked,
/// and that the caller's long eligibility (if any) was the live one.
///
/// Holding it keeps **every** resource in the set committed for the duration of the real side
/// effect, so a second writer cannot slip between the admission and the write. It is released by
/// [`EligibilityOwner::release_commit`], or by dropping it after the caller's real execution or
/// recovery has finished — never earlier, because a caller that leaves while its write is still
/// landing must not hand the resource to someone else.
///
/// The record's own id is the release key and it is private to this module, so a caller cannot
/// fabricate one: a token for one commit can never address another commit's record even if it
/// happens to name the same execution, claim and resource set. That makes "release ends only its
/// own record" structural rather than a comparison somebody has to get right.
///
/// # Drop is the last-resort release, and it says so
///
/// The record is kept alive by an `Arc` shared with the owner's table, so:
///
/// * the ordinary path is [`EligibilityOwner::release_commit`], which removes the table entry and
///   reports whether it really removed one;
/// * if a caller drops the token without releasing — an early return, a panic unwinding past the
///   write, a future dropped before it finished — the `Arc` count reaches zero and the owner's own
///   `Drop` removes the entry.
///
/// What this deliberately does **not** claim is that the side effect ended. Dropping a token is not
/// evidence that bytes stopped landing, so the removed entry is reported on stderr with its
/// execution and resource set, and recovery is the caller's own responsibility: the brief's rule is
/// that a future's RAII must never be read as "the background write completed". A caller that
/// returns early while a write is still in flight therefore leaves a diagnostic, not a silent
/// handover.
#[derive(Debug)]
pub struct ResourceCommit {
    /// The record's own id, and the only thing a release may address.
    commit_id: CommitId,
    /// The execution that owns this commit.
    execution_id: ExecutionId,
    /// The claim this commit was taken under, if any.
    claim_id: Option<EditClaimId>,
    /// The resources really committed, each with the access it was committed at.
    resources: Vec<ResourceRequest>,
    /// The owner's table entry, so a dropped token still ends this record and no other.
    record: std::sync::Arc<CommitRecord>,
}

/// The owner's half of one live commit, shared by `Arc` with the caller's [`ResourceCommit`].
///
/// The record knows how to **end itself** in the owner's table. That is what makes the documented
/// `Drop` real rather than decorative: when the caller's token is the last reference, the last
/// `Drop` removes exactly this record — never another execution's — and reports that the side
/// effect was abandoned rather than that it finished.
#[derive(Debug)]
struct CommitRecord {
    commit_id: u64,
    /// Set by [`EligibilityOwner::release_commit`] before it removes the entry, so the last `Drop`
    /// can tell "the caller released this explicitly" from "the caller left without releasing".
    released_explicitly: std::sync::atomic::AtomicBool,
    /// What a diagnostic names when the token was dropped unreleased: the execution and the
    /// resources, so a reader can see which side effect was left unaccounted for.
    execution_id: ExecutionId,
    resources: Vec<ResourceRequest>,
    /// The owner's own table, so the record can remove its entry without the owner being alive.
    ///
    /// A `Weak` reference: the record must never keep the owner alive, and an owner that is already
    /// gone has already swept its table, so the upgrade simply fails.
    owner: std::sync::Weak<Mutex<EligibilityState>>,
}

impl CommitRecord {
    /// Removes this record from its owner's table if the record still owns one, and reports an
    /// abandoned write. Returns whether an entry was really removed by this call.
    ///
    /// Called only from the last `Drop` of the shared record, so it fires exactly once.
    fn end_and_report(&self) -> bool {
        let removed = self.owner.upgrade().is_some_and(|owner| {
            let mut state = owner.lock().unwrap_or_else(|error| error.into_inner());
            state.commits.remove(&self.commit_id).is_some()
        });
        if !self.released_explicitly.load(std::sync::atomic::Ordering::Acquire) {
            eprintln!(
                "aura resource commit {} for execution {} was dropped without release; the \
                 resources it held are now free and recovery for whatever it was writing is the \
                 caller's, not this owner's: {:?}",
                self.commit_id,
                self.execution_id,
                self.resources.iter().map(|request| request.resource.display()).collect::<Vec<_>>()
            );
        }
        removed
    }
}

impl Drop for EligibilityOwner {
    /// Ends every live commit when the owner itself goes away, and says so.
    ///
    /// A process shutting down with a commit still recorded is a real state — an execution was
    /// admitted and never released — and the honest report is that the resources became free while
    /// the side effect's own recovery responsibility stays where it was. Silently dropping the
    /// table would be the "future's RAII read as a completed background write" mistake.
    fn drop(&mut self) {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        for holder in state.commits.values() {
            holder.record.report_unreleased_owner_gone();
        }
    }
}

impl CommitRecord {
    /// The owner-gone variant of the report: the table is being dropped as a whole.
    fn report_unreleased_owner_gone(&self) {
        if self.released_explicitly.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        eprintln!(
            "aura resource commit {} for execution {} was still live when its owner shut down; \
             the resources are released with the owner and recovery for whatever it was writing is \
             the caller's: {:?}",
            self.commit_id,
            self.execution_id,
            self.resources.iter().map(|request| request.resource.display()).collect::<Vec<_>>()
        );
    }
}

impl ResourceCommit {
    #[must_use]
    pub fn claim_id(&self) -> Option<&str> {
        self.claim_id.as_deref()
    }

    #[must_use]
    pub fn execution_id(&self) -> ExecutionId {
        self.execution_id
    }

    /// The resources this commit holds, so a caller can report exactly what it kept.
    #[must_use]
    pub fn resources(&self) -> Vec<FileResource> {
        self.resources.iter().map(|request| request.resource.clone()).collect()
    }

    /// The same, with each resource's own access.
    #[must_use]
    pub fn requests(&self) -> &[ResourceRequest] {
        &self.resources
    }
}

impl Drop for ResourceCommit {
    fn drop(&mut self) {
        // The owner's table holds the other `Arc`, so this only fires once the record is really
        // gone. A record the owner already swept by an explicit release is flagged, so the ordinary
        // path reports nothing; anything else is a write that was abandoned, and it is both
        // **released** (the entry really goes) and **reported** (a reader can see it was not a
        // completed write).
        self.record.end_and_report();
    }
}

/// How a caller wants to use a resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitIntent {
    /// Take a long edit eligibility and hold it while the caller keeps editing. Used by Buffer open
    /// and by an interactive editor, where the eligibility must outlive the request.
    HoldEditClaim,
    /// Commit a single side effect under whatever eligibility already exists. Used by Main Fs,
    /// Agent file tools, Buffer save, the artifact writer and the external writer.
    CommitOnce,
}

/// What a successful admission produced.
///
/// `Claimed` boxes its eligibility: an [`EditEligibility`] carries a whole `FileResource` plus an
/// owner, which made this enum three hundred bytes wide for a caller that only ever matches on the
/// variant. Boxing the rare arm keeps the common `Committed` arm small without weakening anything —
/// the contained value is the same value, reached through one indirection.
#[derive(Debug)]
pub enum Admission {
    /// The complete set is free for this caller; the commit must be released when the real work
    /// ends.
    Committed(ResourceCommit),
    /// The caller now holds (or already held) a long edit eligibility on the resource.
    Claimed(Box<EditEligibility>),
}

/// A request to commit one **complete** resource set, each resource at its **own** access.
///
/// The set is admitted as a unit. That is the difference between "complete admission" and "hold
/// part while waiting for the rest": a caller that took the first target and then waited on the
/// second would deadlock against a caller doing the mirror image, and a caller that took the first
/// and gave up would have held a resource it never used.
///
/// # Why the access is per resource and not per request
///
/// The previous shape carried one `access` for the whole set, and `CommitRequest::many(resources,
/// Write)` therefore turned a **copy's source** — which the same module's documentation described as
/// a shared read — into a write claim. The consequence is not cosmetic: two copies of one source
/// file would have serialised against each other, and a copy would have looked like a writer of its
/// own source in every conflict report. Access is a property of what the caller does to *one*
/// resource, so it travels with that resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitRequest {
    pub resources: Vec<ResourceRequest>,
}

impl CommitRequest {
    /// One resource, which is the ordinary single-target case.
    #[must_use]
    pub fn one(resource: FileResource, access: ResourceAccess) -> Self {
        Self { resources: vec![ResourceRequest::new(resource, access)] }
    }

    /// Several resources, all at one access. Used where every target really is touched the same way
    /// — a delete of a directory and of its members, or a set of slots.
    #[must_use]
    pub fn many(resources: Vec<FileResource>, access: ResourceAccess) -> Self {
        Self {
            resources: resources
                .into_iter()
                .map(|resource| ResourceRequest::new(resource, access))
                .collect(),
        }
    }

    /// Several resources, each at the access the caller really needs on it.
    ///
    /// This is the form a changeset uses: its write targets are writes, its copy sources are reads,
    /// and its directory scopes are subtree writes. A caller that cannot state one resource's access
    /// has not decided what it is doing to that resource.
    #[must_use]
    pub fn per_resource(resources: Vec<ResourceRequest>) -> Self {
        Self { resources }
    }

    /// The resources alone, for reporting.
    #[must_use]
    pub fn resources(&self) -> Vec<FileResource> {
        self.resources.iter().map(|request| request.resource.clone()).collect()
    }

    pub(crate) fn requests(&self) -> impl Iterator<Item = ResourceRequest> + '_ {
        self.resources.iter().cloned()
    }
}

/// The one owner every file entry asks.
///
/// One instance is shared by Main Fs, the Agent file tools, Buffer save, the artifact writer and
/// the external writer, so a claim taken through one entry is visible to all of them. That is the
/// whole point: five entries used to keep five unrelated views of the same file.
///
/// The state lives behind an `Arc` so a live [`ResourceCommit`] can hold a `Weak` to it: that is
/// what lets a token dropped without release really remove its own record instead of leaking the
/// resource set for the life of the process.
pub struct EligibilityOwner {
    state: std::sync::Arc<Mutex<EligibilityState>>,
}

impl Default for EligibilityOwner {
    fn default() -> Self {
        Self { state: std::sync::Arc::new(Mutex::new(EligibilityState::default())) }
    }
}

impl std::fmt::Debug for EligibilityOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("EligibilityOwner").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct EligibilityState {
    /// Live long edit eligibilities, keyed by the eligibility's own id.
    claims: HashMap<EditClaimId, EditEligibility>,
    /// Short commits in flight, keyed by their own id rather than by a resource key.
    ///
    /// The previous shape was a `HashMap<String, CommitHolder>` keyed by a resource string, which
    /// is why a second commit on the same key silently *replaced* the first record: the count
    /// stayed one and the first release then cleared the second. Keying by the commit's own id
    /// makes two live commits two records, and makes "release ends only its own record" true by
    /// construction.
    commits: HashMap<u64, CommitHolder>,
    /// The next claim id. Monotonic, so a released id is never reused and a stale caller cannot
    /// accidentally adopt a new claim.
    next_claim: u64,
    /// The next commit id. Monotonic for the same reason.
    next_commit: u64,
    /// Migrations observed: the old identity, the new identity and the eligibility that moved.
    migrations: Vec<IdentityMigration>,
}

#[derive(Clone, Debug)]
struct CommitHolder {
    /// The record's own id, which is the release key.
    commit_id: u64,
    execution_id: ExecutionId,
    session_id: SessionId,
    /// The claim this commit was taken under, if any. Read by the snapshot so a caller can see
    /// which long eligibility authorised a live write.
    claim_id: Option<EditClaimId>,
    /// The **complete** resource set this commit really holds, each at its own access.
    ///
    /// The previous shape stored no resource at all, only a string key derived from one. That is
    /// why a commit holding a directory failed to catch a child slot's commit: a directory and a
    /// file inside it produce different strings, and the comparison never got as far as the real
    /// rule. Keeping the resources means commit-time conflict checking uses the very same
    /// [`conflict_between`] the long eligibility uses, at the same shape.
    resources: Vec<ResourceRequest>,
    /// Shared with the caller's token, so a commit that is never explicitly released still ends and
    /// is reported instead of leaking the resource set forever.
    record: std::sync::Arc<CommitRecord>,
}

/// One observed identity replacement, recorded so the migration is provable rather than assumed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityMigration {
    pub claim_id: EditClaimId,
    pub from: FileIdentity,
    pub to: FileIdentity,
}

/// How long and under what cancellation a caller is willing to wait for a complete set.
///
/// The three facts travel together because they are one decision: a deadline without a cancellation
/// source cannot be stopped when the caller goes away, and a cancellation source without the
/// caller's **original** deadline would silently invent a new timeout at the point of conflict.
#[derive(Clone, Copy)]
pub struct AdmissionWait<'a> {
    /// The caller's original execution deadline. Never extended by waiting.
    pub deadline: Instant,
    /// Asked before every attempt and between sleeps. `true` means "stop now and report the last
    /// refusal"; it is what makes the wait cancellable rather than merely bounded.
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    /// The first backoff interval. Later attempts double it up to a small cap.
    pub retry_interval: Duration,
}

impl<'a> AdmissionWait<'a> {
    /// A wait bounded by `deadline`, cancelled by `cancelled`, starting at a short interval.
    #[must_use]
    pub fn new(deadline: Instant, cancelled: &'a (dyn Fn() -> bool + Sync)) -> Self {
        Self { deadline, cancelled, retry_interval: Duration::from_millis(1) }
    }

    /// A wait that is never cancelled, for a caller with no cancellation source of its own.
    ///
    /// Deliberately named so the absence of cancellation is visible at the call site: this is the
    /// weaker form, and a caller that has a cancellation source should use it.
    #[must_use]
    pub fn bounded_only(deadline: Instant) -> Self {
        static NEVER: std::sync::OnceLock<Box<dyn Fn() -> bool + Sync + Send>> =
            std::sync::OnceLock::new();
        let never = NEVER.get_or_init(|| Box::new(|| false));
        // The closure is `'static`, so borrowing it for any lifetime is sound and the returned
        // value carries no reference to a temporary.
        Self { deadline, cancelled: never.as_ref(), retry_interval: Duration::from_millis(1) }
    }
}

impl std::fmt::Debug for AdmissionWait<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdmissionWait")
            .field("deadline", &self.deadline)
            .field("retry_interval", &self.retry_interval)
            .finish_non_exhaustive()
    }
}

/// The opaque id of one short commit. Private: produced only by the owner.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct CommitId(u64);

/// A snapshot of what the owner currently holds. Read-only, for tests and reporting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EligibilitySnapshot {
    pub claims: Vec<EditEligibility>,
    pub commits: usize,
    pub commit_executions: Vec<ExecutionId>,
    pub migrations: Vec<IdentityMigration>,
}

impl EligibilityOwner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, EligibilityState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Distinguishes a long edit claim from a short execution commit without inferring it from
    /// a path or session. A released claim is no longer an editor blocker.
    #[must_use]
    pub fn is_edit_refusal(&self, refusal: &AdmissionRefusal) -> bool {
        let Some(execution) = refusal.held_by_execution else { return false };
        self.lock().claims.values().any(|claim| claim.execution_id == execution)
    }

    /// Tries to take a long edit eligibility on `resource`.
    ///
    /// Succeeds when nothing else holds a conflicting eligibility or commit. A **holder**
    /// re-entering an eligibility it already holds gets the same eligibility back rather than a
    /// second one. The holder is the execution, not the session: two executions of one session are
    /// two holders, and the second really does conflict with the first.
    ///
    /// # Errors
    /// Returns the real conflicting holder when the resource is claimed or committed elsewhere.
    pub fn claim_edit(
        &self,
        resource: FileResource,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        execution_id: ExecutionId,
    ) -> Result<EditEligibility, AdmissionRefusal> {
        self.claim_edit_with_generation(resource, session_id, workspace_id, execution_id, None)
    }

    /// The same, carrying the Buffer generation this eligibility agrees with.
    ///
    /// A Buffer's single eligibility and its generation are one fact, so the caller that owns the
    /// generation passes it in: a second workspace cannot then create a second eligibility on the
    /// same object by presenting a different generation, because the object is the key and the
    /// generation is part of the record's identity check.
    ///
    /// # Errors
    /// As [`Self::claim_edit`].
    pub fn claim_edit_with_generation(
        &self,
        resource: FileResource,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        execution_id: ExecutionId,
        buffer_generation: Option<Revision>,
    ) -> Result<EditEligibility, AdmissionRefusal> {
        let request = ResourceRequest::new(resource.clone(), ResourceAccess::Write);
        let mut state = self.lock();
        // The complete-set rule applies here too: a claim that only *partly* overlaps a live
        // commit is a conflict, and it is judged by the same rule the commit used.
        if let Some(refusal) =
            conflicting_hold(&state, std::slice::from_ref(&request), execution_id)
        {
            return Err(refusal);
        }
        // Re-entry: the same **execution** on the same real resource keeps one eligibility. The
        // workspace is deliberately not part of this test: reaching one physical object through a
        // second workspace is still the same object, and the previous version's
        // `workspace_id == workspace_id` term is exactly how one session obtained two eligibilities
        // on one file.
        if let Some(existing) = state
            .claims
            .values()
            .find(|claim| {
                claim.execution_id == execution_id
                    && claim.resource().same_resource(&resource)
                    && claim.buffer_generation == buffer_generation
            })
            .cloned()
        {
            return Ok(existing);
        }
        state.next_claim = state.next_claim.saturating_add(1);
        let claim = EditEligibility {
            claim_id: format!("edc_{:016x}", state.next_claim),
            resources: vec![ResourceRequest::new(resource, ResourceAccess::Write)],
            session_id,
            workspace_id,
            execution_id,
            buffer_generation,
            epoch: Revision::INITIAL,
        };
        state.claims.insert(claim.claim_id.clone(), claim.clone());
        Ok(claim)
    }

    /// Takes a long edit eligibility covering a **set** of resources.
    ///
    /// # Why a claim can name more than one resource
    ///
    /// A long eligibility on one real file is really a claim on **two** things: the object whose
    /// bytes the editor will replace, and the directory entry that will name the replacement. An
    /// atomic save produces a new object identity, so a claim that named only the old object would
    /// leave the name free for exactly the window the batch forbids — between the replacement and
    /// the eligibility migration.
    ///
    /// A single-resource claim cannot express that, and the commit path correctly refuses to commit
    /// a set that the presented claim does not fully cover. So the claim is expressible as a set, and
    /// the commit's own coverage check then passes on the caller's real resources instead of being
    /// relaxed.
    ///
    /// # Errors
    /// As [`Self::claim_edit`].
    pub fn claim_edit_set(
        &self,
        resources: Vec<ResourceRequest>,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        execution_id: ExecutionId,
        buffer_generation: Option<Revision>,
    ) -> Result<EditEligibility, AdmissionRefusal> {
        if resources.is_empty() {
            return Err(stale_refusal(&[], "an empty edit eligibility"));
        }
        let mut state = self.lock();
        if let Some(refusal) = conflicting_hold(&state, &resources, execution_id) {
            return Err(refusal);
        }
        // Re-entry is by the claim's **first** resource, which is the object the caller names: that
        // is the resource whose identity a replacement changes, and so the one whose re-entry must
        // not create a second eligibility.
        if let Some(existing) = state
            .claims
            .values()
            .find(|claim| {
                claim.execution_id == execution_id
                    && claim
                        .resources
                        .first()
                        .is_some_and(|held| held.resource.same_resource(&resources[0].resource))
                    && claim.buffer_generation == buffer_generation
            })
            .cloned()
        {
            return Ok(existing);
        }
        state.next_claim = state.next_claim.saturating_add(1);
        let claim = EditEligibility {
            claim_id: format!("edc_{:016x}", state.next_claim),
            resources,
            session_id,
            workspace_id,
            execution_id,
            buffer_generation,
            epoch: Revision::INITIAL,
        };
        state.claims.insert(claim.claim_id.clone(), claim.clone());
        Ok(claim)
    }

    /// Takes a short commit for the **complete** resource set of one side effect.
    ///
    /// This is the atomic half. It succeeds only when no other execution holds a conflicting long
    /// eligibility and no conflicting commit is in flight, and it records the whole set so a
    /// second writer arriving during the real write is refused by the same real rule rather than
    /// by a string comparison.
    ///
    /// A commit taken with a long eligibility must present one that is **currently live**, owned
    /// by this execution, on a resource that really covers what is being committed, with the epoch
    /// the caller holds. Saving the passed id without checking it is what let a released claim
    /// commit and let a tokenless commit through under a live claim.
    ///
    /// # Errors
    /// Returns the real conflicting holder, or [`ConflictReason::SameObject`] against `"stale"` for
    /// an eligibility that is no longer the live one.
    pub fn commit(
        &self,
        request: &CommitRequest,
        session_id: SessionId,
        execution_id: ExecutionId,
        claim: Option<&EditEligibility>,
    ) -> Result<ResourceCommit, AdmissionRefusal> {
        let requests = request.requests().collect::<Vec<_>>();
        let mut state = self.lock();
        // The arming check happens before the judgement, so a seam-injected refusal is reached on
        // exactly the path a real conflict would be: the complete set has been assembled and the
        // owner is about to decide, and the caller sees the same typed refusal either way.
        if let Some(injected) = fault::take(fault::Stage::BeforeAdmit, request, execution_id) {
            return Err(injected);
        }
        if let Some(refusal) = conflicting_hold(&state, &requests, execution_id) {
            return Err(refusal);
        }
        if let Some(injected) = fault::take(fault::Stage::ConflictAtAdmit, request, execution_id) {
            return Err(injected);
        }
        // A claim handed in must still be the live one. Presenting an id is not evidence: the
        // record must exist, belong to this execution, cover every resource being committed, and
        // still carry the epoch the caller believes it holds.
        let claim_id = match claim {
            Some(presented) => {
                let live = state.claims.get(&presented.claim_id).ok_or_else(|| {
                    stale_refusal(&requests, "a released or unknown edit eligibility")
                })?;
                if live.execution_id != execution_id {
                    return Err(stale_refusal(
                        &requests,
                        "an edit eligibility held by a different execution",
                    ));
                }
                if live.epoch != presented.epoch {
                    return Err(stale_refusal(
                        &requests,
                        "an edit eligibility whose epoch has moved (an identity migration or a \
                         concurrent commit changed it)",
                    ));
                }
                for request in &requests {
                    // A presented claim must cover **every** resource being committed, and it is
                    // judged against the claim's whole set: a Buffer whose claim names the object and
                    // the entry covers both, while a claim naming one object does not cover a scope
                    // it never claimed.
                    let covered =
                        live.resources.iter().any(|held| conflict_between(request, held).is_some());
                    if !covered {
                        return Err(stale_refusal(
                            &requests,
                            "a resource the presented edit eligibility does not cover",
                        ));
                    }
                }
                Some(live.claim_id.clone())
            }
            // No claim presented: allowed only when nothing live would have been required. A
            // caller that names an object someone else holds a long eligibility on is refused by
            // `conflicting_hold` above, so this branch is the genuinely free case.
            None => None,
        };
        state.next_commit = state.next_commit.saturating_add(1);
        let commit_id = state.next_commit;
        let record = std::sync::Arc::new(CommitRecord {
            commit_id,
            released_explicitly: std::sync::atomic::AtomicBool::new(false),
            execution_id,
            resources: requests.clone(),
            owner: std::sync::Arc::downgrade(&self.state),
        });
        state.commits.insert(
            commit_id,
            CommitHolder {
                commit_id,
                execution_id,
                session_id,
                claim_id: claim_id.clone(),
                resources: requests.clone(),
                record: std::sync::Arc::clone(&record),
            },
        );
        Ok(ResourceCommit {
            commit_id: CommitId(commit_id),
            execution_id,
            claim_id,
            resources: requests,
            record,
        })
    }

    /// Releases a short commit taken by [`Self::commit`].
    ///
    /// Only the record the token names is removed, and only its owning execution may do it. The
    /// previous shape removed whatever was at a resource key, which is how one execution's release
    /// silently cleared another execution's live commit — the measured
    /// `first_release_clears_second_commit`. Returns whether a record was really removed, so a
    /// caller can report "released" and "already gone" apart.
    ///
    /// Called only after the caller's real execution — or its recovery — has finished. A caller
    /// that returns early while a write is still landing must keep holding it.
    pub fn release_commit(&self, commit: ResourceCommit) -> bool {
        let mut state = self.lock();
        let Some(holder) = state.commits.get(&commit.commit_id.0) else {
            return false;
        };
        if holder.execution_id != commit.execution_id {
            return false;
        }
        // Mark before removing: this token is about to become the last reference, and its `Drop`
        // must not report an ordinary release as an abandoned write.
        commit.record.released_explicitly.store(true, std::sync::atomic::Ordering::Release);
        state.commits.remove(&commit.commit_id.0).map(|_| ()).is_some()
    }

    /// Synchronous bounded admission for callers that explicitly own an OS thread.
    ///
    /// This sleeps that thread and must never run inside a Core blocking carrier or consume an
    /// execution permit while waiting. Reactive production callers make a single `commit`
    /// attempt and wait in their async application owner. No lock spans the sleep.
    ///
    /// # Errors
    /// Returns the final [`AdmissionRefusal`]. A non-retryable refusal returns immediately, because
    /// waiting cannot change a stale-eligibility answer.
    pub fn commit_waiting(
        &self,
        request: &CommitRequest,
        session_id: SessionId,
        execution_id: ExecutionId,
        claim: Option<&EditEligibility>,
        wait: &AdmissionWait<'_>,
    ) -> Result<ResourceCommit, AdmissionRefusal> {
        let mut interval = wait.retry_interval.max(Duration::from_micros(50));
        loop {
            match self.commit(request, session_id, execution_id, claim) {
                Ok(commit) => return Ok(commit),
                Err(refusal) => {
                    if !refusal.retryable || (wait.cancelled)() {
                        return Err(refusal);
                    }
                    let now = Instant::now();
                    if now >= wait.deadline {
                        return Err(refusal);
                    }
                    let remaining = wait.deadline.saturating_duration_since(now);
                    std::thread::sleep(interval.min(remaining));
                    // Bounded exponential backoff, so a short hold is picked up quickly while a
                    // long one does not turn into a spin. The cap keeps the worst-case latency
                    // small relative to any realistic execution deadline.
                    interval = (interval * 2).min(Duration::from_millis(20));
                }
            }
        }
    }

    /// Releases a long edit eligibility held by `claim_id` for `session_id`.
    ///
    /// Refuses to release an eligibility owned by another session, so one session cannot end
    /// another's eligibility. It releases **only a claim**: a live commit is a real side effect
    /// that has already started, and ending an eligibility is not proof that the side effect
    /// ended. The previous shape removed the claim and left the commit; the requirement is the
    /// other way round — the commit outlives the eligibility that authorised it.
    ///
    /// Returns whether an eligibility was actually released, which lets a caller report "revoked"
    /// and "never held" apart instead of claiming both.
    pub fn release_claim(&self, claim_id: &str, session_id: SessionId) -> bool {
        let mut state = self.lock();
        let owned = state.claims.get(claim_id).is_some_and(|claim| claim.session_id == session_id);
        if owned {
            state.claims.remove(claim_id);
        }
        owned
    }

    /// Releases every **claim** belonging to one session.
    ///
    /// This is the session-lifecycle release, and it deliberately does **not** touch commits. The
    /// previous shape removed the session's commits too, which handed a still-running write to
    /// anyone who asked: a session ending is not evidence that a side effect it started has
    /// finished. Their commits stay until the execution that owns them releases them, or until
    /// their recovery responsibility is taken over — which is a separate, explicit act.
    ///
    /// Returns how many eligibilities were released.
    pub fn release_session(&self, session_id: SessionId) -> usize {
        let mut state = self.lock();
        let before = state.claims.len();
        state.claims.retain(|_, claim| claim.session_id != session_id);
        before.saturating_sub(state.claims.len())
    }

    /// Releases every eligibility belonging to one workspace, and reports the commits it left.
    ///
    /// Used by workspace lifecycle, which owns the whole workspace rather than one file. This is
    /// the deliberate exception to per-file coordination: closing a workspace really does end every
    /// edit eligibility inside it.
    ///
    /// The return value names the commits that are **still live** in this workspace, because the
    /// caller has to account for them: a workspace lifecycle that dropped them would be claiming
    /// the writes had ended. They are kept, and the caller decides whether their executions have
    /// really finished or whether their recovery responsibility has been assumed.
    pub fn release_workspace(&self, workspace_id: WorkspaceId) -> WorkspaceRelease {
        let mut state = self.lock();
        let before = state.claims.len();
        state.claims.retain(|_, claim| claim.workspace_id != workspace_id);
        let released = before.saturating_sub(state.claims.len());
        let surviving_commits = state
            .commits
            .values()
            .filter(|holder| {
                holder
                    .resources
                    .iter()
                    .any(|request| request.resource.owner().workspace_id == workspace_id)
            })
            .map(|holder| holder.commit_id)
            .collect::<Vec<_>>();
        WorkspaceRelease { released_claims: released, surviving_commits }
    }

    /// Migrates an eligibility when the object it names was really replaced.
    ///
    /// The ordinary atomic save replaces the file with a new object, so the `FileIdentity` the
    /// eligibility was taken against no longer exists. Without this, the eligibility would appear
    /// to be on a resource nobody holds, and a second writer would be admitted during the save that
    /// produced the replacement.
    ///
    /// The migration is validated **inside the same lock** as the commit protection, and it is
    /// refused when the new identity is already held by someone else: accepting it would leave two
    /// live eligibilities on one object, which is the measured defect this replaces. A refusal is
    /// returned with the real holder rather than silently dropping the migration.
    ///
    /// # Errors
    /// Returns the real conflicting holder when the replacement is already claimed or committed.
    pub fn migrate_claim_identity(
        &self,
        claim_id: &str,
        to: FileIdentity,
    ) -> Result<Option<IdentityMigration>, AdmissionRefusal> {
        let mut state = self.lock();
        let Some(claim) = state.claims.get(claim_id).cloned() else {
            // The eligibility may legitimately have been released between the two observations.
            return Ok(None);
        };
        let FileResource::Object { identity, location, .. } = claim.resource() else {
            // Only an object has an identity that a replacement can change.
            return Ok(None);
        };
        let from = identity.clone();
        if from == to {
            return Ok(None);
        }
        let mut migrated_resource = claim.resource().clone();
        let FileResource::Object { identity, .. } = &mut migrated_resource else {
            return Ok(None);
        };
        *identity = to.clone();
        // The replacement must be free for *this* holder. `conflicting_hold` skips records owned by
        // this execution, so the migration does not conflict with the eligibility being migrated —
        // only with somebody else's.
        let request = ResourceRequest::new(migrated_resource.clone(), ResourceAccess::Write);
        if let Some(refusal) =
            conflicting_hold(&state, std::slice::from_ref(&request), claim.execution_id)
        {
            return Err(refusal);
        }
        let _ = location;
        let claim = state
            .claims
            .get_mut(claim_id)
            .expect("the eligibility was present a moment ago under this lock");
        claim.resources[0].resource = migrated_resource;
        claim.epoch = Revision::new(claim.epoch.value().saturating_add(1));
        let migration = IdentityMigration { claim_id: claim_id.to_owned(), from, to };
        state.migrations.push(migration.clone());
        Ok(Some(migration))
    }

    /// The live long eligibility on exactly this resource, if any.
    #[must_use]
    pub fn claim_on(&self, resource: &FileResource) -> Option<EditEligibility> {
        let state = self.lock();
        state.claims.values().find(|claim| claim.resource().same_resource(resource)).cloned()
    }

    /// Every live eligibility. Read-only.
    #[must_use]
    pub fn snapshot(&self) -> EligibilitySnapshot {
        let state = self.lock();
        let mut claims = state.claims.values().cloned().collect::<Vec<_>>();
        claims.sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
        let mut commit_executions =
            state.commits.values().map(|holder| holder.execution_id).collect::<Vec<_>>();
        commit_executions.sort();
        EligibilitySnapshot {
            claims,
            commits: state.commits.len(),
            commit_executions,
            migrations: state.migrations.clone(),
        }
    }

    /// The claim ids of the live commits, so a caller can see which long eligibility authorised a
    /// write that is still in flight. Read-only.
    #[must_use]
    pub fn live_commit_claims(&self) -> Vec<Option<EditClaimId>> {
        let state = self.lock();
        let mut claims =
            state.commits.values().map(|holder| holder.claim_id.clone()).collect::<Vec<_>>();
        claims.sort();
        claims
    }

    /// Whether the owner really holds this commit, for a caller that has to prove a release landed.
    #[must_use]
    pub fn holds_commit(&self, commit: &ResourceCommit) -> bool {
        let state = self.lock();
        state
            .commits
            .get(&commit.commit_id.0)
            .is_some_and(|holder| holder.execution_id == commit.execution_id)
    }
}

/// What a workspace release really did, so the caller cannot read it as "everything ended".
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceRelease {
    pub released_claims: usize,
    /// Commits still live in that workspace. The caller owns making them safe; the owner refuses
    /// to pretend they ended.
    pub surviving_commits: Vec<u64>,
}

/// The refusal for a commit that presented an eligibility which is no longer the live one.
fn stale_refusal(requests: &[ResourceRequest], what: &str) -> AdmissionRefusal {
    AdmissionRefusal {
        reason: ConflictReason::SameObject,
        requested: requests.first().map_or_else(String::new, |r| r.resource.display()),
        held: what.to_owned(),
        held_by: None,
        held_by_execution: None,
        retryable: false,
    }
}

/// The still-live conflict against a request set, if there is one.
///
/// # Why the same session is not exempt
///
/// The previous implementation skipped every record belonging to the caller's session and
/// documented that as a rule: *"a claim or commit held by the same session is not a conflict: one
/// session's own work is allowed to proceed in its own order."* The user's confirmed behaviour is
/// the opposite — **two tools in the same session must obey the same file-conflict protection** —
/// and the measured consequence of the old rule was that a second commit on one object was
/// admitted while the first was in flight. The comparison is therefore by **execution**: an
/// execution never conflicts with its own record, and two executions of one session conflict
/// exactly as two sessions do.
///
/// # Why the whole set is compared at once
///
/// Every request in the incoming set is compared against every live record's **whole** resource
/// list with [`conflict_between`]. Comparing a derived string key instead is what lost the
/// directory-versus-child conflict: a directory scope and a file inside it produce different keys
/// and are nevertheless the same real conflict.
///
/// A record with an indeterminate identity — a slot whose volume case rule the resolver could not
/// establish — is treated as a conflict for any overlapping request rather than as free. A missing
/// determinate fact must not admit a change as "no conflict".
fn conflicting_hold(
    state: &EligibilityState,
    requests: &[ResourceRequest],
    execution_id: ExecutionId,
) -> Option<AdmissionRefusal> {
    if let Some(request) = requests.iter().find(|request| !request.has_determinate_identity()) {
        return Some(AdmissionRefusal {
            reason: ConflictReason::SameSlot,
            requested: request.resource.display(),
            held: "file.name_comparison_unavailable".into(),
            held_by: None,
            held_by_execution: None,
            retryable: false,
        });
    }
    for claim in state.claims.values() {
        if claim.execution_id == execution_id {
            continue;
        }
        if let Some(reason) = conflict_for_set(requests, &claim.resources) {
            return Some(AdmissionRefusal {
                reason,
                requested: display_of(requests),
                held: claim.resource().display(),
                held_by: Some(claim.session_id),
                held_by_execution: Some(claim.execution_id),
                retryable: true,
            });
        }
    }
    for holder in state.commits.values() {
        if holder.execution_id == execution_id {
            continue;
        }
        let held = holder.resources.clone();
        if let Some(reason) = conflict_for_set(requests, &held) {
            return Some(AdmissionRefusal {
                reason,
                requested: display_of(requests),
                held: holder
                    .resources
                    .first()
                    .map_or_else(String::new, |request| request.resource.display()),
                held_by: Some(holder.session_id),
                held_by_execution: Some(holder.execution_id),
                retryable: true,
            });
        }
    }
    None
}

/// The real conflict between two complete sets, at the same rule for both.
///
/// # The indeterminate case, and why it is not a blanket refusal
///
/// `has_determinate_identity` is false for one shape only: a slot whose volume case rule the
/// resolver could not establish, so two spellings of one entry cannot be shown equal *or* distinct.
/// The rule is that such a request must not be admitted as "no conflict" — and the narrow reading of
/// that is the correct one: the undecidable request is refused against a holder whose resource
/// **could be the same entry**, not against every holder in the process.
///
/// Refusing it against everything would make one unreadable volume serialise every file operation in
/// every workspace, which is the coarse global exclusion this design exists to remove. So the
/// undecidable side is compared structurally — anchor and chain, plus the leaf under **both**
/// readings of the rule — and a holder that cannot be that same entry stays parallel.
fn conflict_for_set(
    mine: &[ResourceRequest],
    theirs: &[ResourceRequest],
) -> Option<ConflictReason> {
    for request in mine {
        for held in theirs {
            if let Some(reason) = conflict_between(request, held) {
                return Some(reason);
            }
            // The undecidable side: this request may be the same entry as `held`, and nothing
            // proved it is not. Only a pair that really could be one entry is refused.
        }
    }
    None
}

/// Display the first target and the number of additional targets in a refusal.
fn display_of(requests: &[ResourceRequest]) -> String {
    match requests {
        [] => String::new(),
        [only] => only.resource.display(),
        many => format!("{} (+{} more)", many[0].resource.display(), many.len().saturating_sub(1)),
    }
}

/// The path a caller labels a resource with. Kept here so every entry builds its `ResourceOwner`
/// the same way instead of each inventing its own display text.
#[must_use]
pub fn owner_of(
    workspace_id: WorkspaceId,
    session_id: SessionId,
    path: &WorkspacePath,
) -> crate::resource::ResourceOwner {
    crate::resource::ResourceOwner { workspace_id, session_id, path: path.clone() }
}

/// Named, default-off fault seams for the eligibility owner.
///
/// A directed test has to reach a **real** stage of the owner rather than re-implement it: an
/// arithmetic guard, or a refusal injected at the exact instant the owner would admit. Both are
/// real outcomes the caller already has to handle — a refusal is a normal [`AdmissionRefusal`] —
/// so nothing here fabricates a fact or invents a code path the product does not have. Nothing is
/// armed unless a caller arms it, and a seam names one operation and one target so it cannot
/// silently perturb an unrelated request.
pub mod fault {
    use std::sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    };

    use super::{AdmissionRefusal, CommitRequest, ExecutionId};

    /// Which stage a seam may perturb.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Stage {
        /// The point where the complete set has been judged and the owner is about to insert the
        /// record. A refusal injected here exercises the caller's whole-set failure path.
        BeforeAdmit,
        /// A real commit that would succeed is forced to look like a conflicting hold, so the
        /// caller's "someone else holds it" branch is reachable without a second thread.
        ConflictAtAdmit,
    }

    /// What one armed seam applies to.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Target {
        /// Any request.
        Any,
        /// Only this execution.
        Execution(ExecutionId),
    }

    static ARMED: AtomicBool = AtomicBool::new(false);
    static STATE: Mutex<Option<(Stage, Target, u32)>> = Mutex::new(None);

    fn lock() -> MutexGuard<'static, Option<(Stage, Target, u32)>> {
        STATE.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Arms one seam for `remaining` admissions. Default-off until this is called.
    pub fn arm(stage: Stage, target: Target, remaining: u32) {
        *lock() = Some((stage, target, remaining));
        ARMED.store(true, Ordering::SeqCst);
    }

    /// Disarms every seam.
    pub fn disarm() {
        *lock() = None;
        ARMED.store(false, Ordering::SeqCst);
    }

    /// Whether a seam is armed right now, for a test that must not silently measure nothing.
    #[must_use]
    pub fn is_armed() -> bool {
        ARMED.load(Ordering::SeqCst)
    }

    /// Takes one use of the seam at `stage` for `execution`, if one is armed for it.
    ///
    /// Returns the injected refusal, or `None` when the seam is off, exhausted, or aimed at a
    /// different execution. The remaining-use counter is decremented under the seam's own lock, so
    /// a concurrent run cannot take more uses than were armed.
    pub(crate) fn take(
        stage: Stage,
        request: &CommitRequest,
        execution: ExecutionId,
    ) -> Option<AdmissionRefusal> {
        if !is_armed() {
            return None;
        }
        let mut state = lock();
        let (armed_stage, target, remaining) = state.as_mut()?;
        if *armed_stage != stage || *remaining == 0 {
            return None;
        }
        if matches!(target, Target::Execution(only) if *only != execution) {
            return None;
        }
        *remaining -= 1;
        Some(AdmissionRefusal {
            reason: crate::resource::ConflictReason::SameObject,
            requested: request
                .resources
                .first()
                .map_or_else(String::new, |request| request.resource.display()),
            held: format!("injected {stage:?} fault seam"),
            held_by: None,
            held_by_execution: None,
            retryable: false,
        })
    }
}

#[cfg(test)]
mod tests;
