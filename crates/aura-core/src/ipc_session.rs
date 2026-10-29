//! Per-connection execution lanes for request handlers.
//!
//! The Core's read loop must never wait for a handler: one slow handler used to
//! stall every later request on the connection (a `runtime.readiness` probe that
//! takes tens of seconds starved the settings page and the rebuilt window until
//! Main timed out and retired the channel).
//!
//! This module owns the replacement: four bounded lanes per connection.
//!
//! * the concurrent lane runs the routes the registry marks `ReadOnly`, several
//!   at a time, because two reads cannot observe each other's ordering;
//! * the session lane runs the declared `SessionId`-bound write routes - one lane
//!   per session - so a session's own requests keep their arrival order while two
//!   *different* sessions make progress independently. A session waiting on its own
//!   authority, launch lease or storage section must not stop another session's
//!   ordinary request from running;
//! * the serial lane runs everything else one at a time in arrival order, so
//!   connection-level state (global configuration, this connection's own
//!   subscription set) keeps its ordering;
//! * the control lane runs the declared control routes, so a cancellation is
//!   *received* while a long ordered handler is still running instead of queuing
//!   behind it.
//!
//! All four lanes are bounded (running *plus* waiting) and all four answer a
//! burst with an overload error instead of blocking the reader, so cancellation
//! and connection control stay responsive no matter how slow a handler is. No
//! handler body ever runs inside the read loop.
//!
//! A lane decides *when* a request runs, never *whether* it may: every admitted
//! request still takes the same full path — ledger entry, invocation claim where
//! its route declares one, handler, reply, settle — so no lane skips
//! authentication, permission or the invocation transaction.
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

mod lane_key;
#[cfg(test)]
#[path = "ipc_session/resource_wait_tests.rs"]
mod resource_wait_tests;
mod terminal_input;
#[cfg(test)]
#[path = "ipc_session/terminal_input_tests.rs"]
mod terminal_input_tests;
use lane_key::{LaneKey, SessionLaneKey};

use aura_domain_common::{CorrelationId, ErrorCategory, ErrorCode, WorkspaceId, WorkspacePath};
use aura_ipc::{JsonRpcRequest, routes::RouteIdempotency, trace};
use aura_workspace_domain::{
    BufferSaveCommand, FsApplyChangesetRequest, FsApplyPatchRequest, FsChangesetOperation,
    FsCopyRequest, FsCreateDirectoryRequest, FsDeleteRequest, FsMoveRequest, FsWriteFileRequest,
};
use serde::de::DeserializeOwned;

/// Deserializes a request's own parameters as the registered route's request DTO.
///
/// This is the single place the classification is allowed to read a request's meaning. It uses
/// `serde_json::from_value` on the real contract type, so the classification and the handler agree
/// by construction: whatever the handler would reject as an invalid request, this also rejects,
/// and the request then keeps the ordered lane instead of gaining a resource lane from a string
/// that happens to be present.
fn typed_params<T: DeserializeOwned>(request: &JsonRpcRequest) -> Option<T> {
    serde_json::from_value(request.params.clone()).ok()
}

/// How many read-only handlers may run at once.
pub(crate) const MAX_CONCURRENT_READ_ONLY: usize = 8;
/// How many read-only handlers may wait for a lane.
pub(crate) const MAX_QUEUED_READ_ONLY: usize = 32;
/// How many ordered handlers may wait behind the one that is running.
pub(crate) const MAX_QUEUED_SERIAL: usize = 32;
/// How many session-bound write handlers may run at once, across all sessions.
///
/// This is the connection-level admission bound for the session lane: it counts
/// *sessions with work in flight*, so it is the total a single connection can put into
/// the Core at once through session-bound writes. It does not change what a session may
/// have queued behind its own request.
pub(crate) const MAX_CONCURRENT_SESSION_WRITES: usize = 8;
/// How many session-bound write handlers may wait for their session's turn, in total.
pub(crate) const MAX_QUEUED_SESSION_WRITES: usize = 32;
/// How many **resource waits** one connection may carry at once, across every file lane.
///
/// # Why a wait needs its own bound and is not an execution slot
///
/// A request whose declared targets conflict with a live one is not executing: it is parked inside
/// the real common admission, waiting for another execution to release the object. That waiting
/// retains its handler task, but releases its worker and execution slot. Counting it
/// against [`MAX_CONCURRENT_SESSION_WRITES`] was therefore the measured defect: several workspaces,
/// or several names for one object, produce **different lane keys** that then wait on the **same**
/// real resource, and each of those waiters took one of the connection's eight execution slots.
/// Eight related waiters could therefore exhaust the connection and make an unrelated file request
/// and an ordinary `agent.submit` / `agent.inbox.mutate` / permission write queue behind them.
///
/// So a wait has its own identity, its own bound and its own queue discipline, and it is **not**
/// counted as a running resource operation. It is deliberately a *small* bound: waiting is bounded
/// work, and a caller that exceeds it is better told to retry than left parked indefinitely.
pub(crate) const MAX_RESOURCE_WAITERS: usize = 64;
/// How many control handlers may run at once. Handling one is a ledger transition
/// plus a reply, so a few can overlap without ordering anything.
pub(crate) const MAX_CONCURRENT_CONTROL: usize = 4;
/// How many control handlers may wait for a lane.
pub(crate) const MAX_QUEUED_CONTROL: usize = 16;

/// Which lane a request belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Lane {
    /// Safe to run beside other requests: the registry marks it read-only.
    Concurrent,
    /// Runs alone, in arrival order: connection-level effects and global configuration.
    Serial,
    /// One lane **per session**: same session ordered, different sessions independent.
    Session,
    /// Declared connection control: must not wait behind ordered business work.
    Control,
}

impl Lane {
    const COUNT: usize = 4;

    const fn index(self) -> usize {
        match self {
            Self::Concurrent => 0,
            Self::Serial => 1,
            Self::Session => 2,
            Self::Control => 3,
        }
    }
}

/// One **verified file target's** lane identity within the connection.
///
/// This is the connection-level half of the confirmed file-coordination decision: ordinary file
/// writes, moves, deletes and Buffer saves are coordinated by their **actual target**, so two
/// operations on the same file wait for each other while operations on different files proceed in
/// parallel. It coordinates *ordering on this connection*, and deliberately nothing more.
///
/// WHAT IT IS NOT. It does not do resource admission, does not hold a write claim, and does not
/// replace the shared coordination boundary: the real mutual exclusion lives in the file
/// protection layer, which knows the actual `FileIdentity`, the directory-entry rules and the
/// workspace relationships. This key only decides *when* a request may start so that two requests
/// naming the same target on one connection do not both run at once.
///
/// WHY THE KEY COMES FROM THE DECLARED REQUEST AND NOT FROM DISK. A lane is chosen before the
/// handler runs, so no handle exists yet to identify the object. The key is therefore built from
/// the request's **deserialized DTO** — the real `WorkspaceFileTarget`, the real recursive flag,
/// the real changeset operations — normalised the way Windows compares path names. It cannot see
/// through a hardlink or a reparse point; that is the protection layer's job, and it does see them
/// because it resolves real identities. A request whose parameters do not deserialize as its own
/// registered request type does **not** get a resource lane: it keeps the ordered lane, which can
/// only be stricter than what it asked for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileTargetLaneKey {
    inner: ResourceLaneKey,
}

impl FileTargetLaneKey {
    fn of(workspace: RawWorkspace, targets: Vec<RawTarget>) -> Self {
        Self { inner: ResourceLaneKey::of(workspace, targets) }
    }

    /// The declared targets, for a test that has to prove which resources a request took.
    #[cfg(test)]
    fn declared(&self) -> &[RawTarget] {
        &self.inner.targets
    }

    /// The tree this lane is anchored to, for a test that has to prove two requests were separated
    /// by their real root rather than by a path string.
    #[cfg(test)]
    fn workspace(&self) -> &RawWorkspace {
        &self.inner.workspace
    }
}

/// Whether a route may run on the concurrent read-only lane.
///
/// The classification comes from the authoritative route registry rather than a
/// name prefix, so a new route is serialised unless it explicitly declares
/// itself read-only.
#[must_use]
pub(crate) fn is_concurrent_read_only(request: &JsonRpcRequest) -> bool {
    aura_ipc::routes::core_route(&request.method)
        .is_some_and(|route| route.idempotency == RouteIdempotency::ReadOnly)
}

/// Session-scoped writes that carry the session they belong to.
///
/// WHY A TABLE AND NOT A PARAMETER SNIFF. Which requests may run beside another
/// session's is an ordering claim about Core state, so it is declared here once, next to
/// the lane it selects, and reviewed like any other classification. Nothing about a route
/// changes by being listed: it is still authenticated, still takes its invocation claim,
/// still runs its handler and still settles. Only *when* it may run changes - and only
/// against a different session.
///
/// WHAT IS DELIBERATELY NOT HERE
/// * `session.create`: it has no session yet, so there is nothing to order it against;
///   it stays on the ordered lane.
/// * `permission.set_defaults`, host-preparation mutations, external-grant mutations,
///   quarantine and artifact routes: global configuration or repository-wide state, not
///   one session's, so they keep the original order.
/// * `projection.subscribe` / `projection.unsubscribe`: connection state, not a session's.
const SESSION_BOUND_WRITE_ROUTES: [&str; 14] = [
    // Sending a message and editing the pending queue of one session.
    "agent.submit",
    "agent.inbox.mutate",
    // One session's own run/approval/query control.
    "agent.start",
    "agent.continue",
    "agent.related.start",
    "agent.cancel",
    "agent.mode.control",
    "agent.query.answer",
    "agent.query.touch",
    "agent.query.select_file",
    "agent.context.recheck",
    "agent.approval.resolve",
    // One session's permission configuration and its own shell stop.
    "permission.set_session_mode",
    "agent.shell.stop",
];

/// Whether a write route is bound to one session, and to which one.
///
/// Read-only requests are not session writes: a read cannot observe another read's
/// ordering, and the concurrent lane already serves them. A route that names a session
/// but carries no usable `session_id` parameter is **not** put on a session lane - it
/// falls through to the ordered lane, which can only be stricter than what it asked for.
#[must_use]
pub(crate) fn session_write_scope(request: &JsonRpcRequest) -> Option<SessionLaneKey> {
    if is_concurrent_read_only(request)
        || CONTROL_ROUTES.contains(&request.method.as_str())
        || CONNECTION_ORDERED_ROUTES.contains(&request.method.as_str())
    {
        return None;
    }
    if !SESSION_BOUND_WRITE_ROUTES.contains(&request.method.as_str()) {
        return None;
    }
    let session_id = request.params.get("session_id")?.as_str()?;
    if session_id.is_empty() {
        return None;
    }
    Some(SessionLaneKey::of(session_id))
}

/// File routes whose real resource set makes them safe to coordinate by resource rather than by
/// connection, each with the **real DTO** its registered route declares.
///
/// WHY A TYPED TABLE AND NOT A PARAMETER NAME. The first version of this table paired a route with
/// the *name* of a parameter and read it with `params.get(parameter)?.as_str()?`. Every entry was
/// dead on arrival: the real DTOs for `fs.write_file`/`fs.apply_patch`/`fs.delete` spell the target
/// as `WorkspaceFileTarget { path: WorkspacePath }` and `fs.move` spells both ends as
/// `WorkspaceFileTarget`, so `.as_str()` returned `None` for all of them and the request silently
/// kept the ordered lane. A second entry named `buffer.save`, which is not a registered route at
/// all — the real one is `workspace.buffer.save` — and it was the *only* entry that ever matched,
/// because it was the only one given a plain string in the tests. So the classification is now
/// built by **deserializing the registered route's own request DTO** and reading the typed fields.
/// A request that does not deserialize is not a valid request of that route: it keeps the ordered
/// lane and the handler rejects it exactly as before.
///
/// WHAT IS DELIBERATELY NOT HERE:
/// * global configuration (`permission.set_defaults`, host preparation, external grants,
///   quarantine, artifact export) — repository-wide or connection-wide, so it keeps its order;
/// * workspace lifecycle (open/close/migrate) and `projection.subscribe`/`unsubscribe` — these
///   order connection or workspace state, not one file;
/// * `agent.tools.configure` and the other Tool settings writes — process-wide configuration.
///
/// A route listed here is not made more permissive: it still takes its ledger entry, its
/// authentication, its permission check, its command claim and its reply exactly as before. Only
/// *when* it may start, relative to a different resource, changes.
///
/// `fs.copy` and `fs.apply_changeset` ARE here now. The earlier version left them on the ordered
/// lane on the argument that "a single-target lane key would be a lie" for a multi-target route.
/// That argument was about the old string key, not about the requirement: a multi-target operation
/// must take **one full admission** over its whole resource set, which is exactly what
/// [`ResourceLaneKey`] carries. Leaving them serialised the whole connection for an operation whose
/// real impact is a known, finite set of files — the coarse serialisation the brief says does not
/// count as done.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileRoute {
    /// `fs.write_file`: one target.
    WriteFile,
    /// `fs.apply_patch`: one target.
    ApplyPatch,
    /// `fs.create_directory`: one target.
    CreateDirectory,
    /// `fs.delete`: one target. `recursive` widens it to a directory subtree.
    Delete,
    /// `fs.move`: **both** ends, exactly as the confirmed decision requires.
    Move,
    /// `fs.copy`: source and destination.
    Copy,
    /// `fs.apply_changeset`: the full set of operations, in their declared order.
    ApplyChangeset,
    /// `workspace.buffer.save`: one target, the Buffer's own path.
    BufferSave,
}

impl FileRoute {
    /// The registered route descriptor this entry claims to classify.
    ///
    /// The method literal is looked up in the registry, and the DTO is deserialized from the
    /// request's own parameters. There is exactly one source of truth for both, so a route rename
    /// or a DTO change cannot leave a stale string classification behind.
    fn method(self) -> &'static str {
        match self {
            Self::WriteFile => aura_ipc::method::FS_WRITE_FILE,
            Self::ApplyPatch => aura_ipc::method::FS_APPLY_PATCH,
            Self::CreateDirectory => aura_ipc::method::FS_CREATE_DIRECTORY,
            Self::Delete => aura_ipc::method::FS_DELETE,
            Self::Move => aura_ipc::method::FS_MOVE,
            Self::Copy => aura_ipc::method::FS_COPY,
            Self::ApplyChangeset => aura_ipc::method::FS_APPLY_CHANGESET,
            Self::BufferSave => aura_ipc::method::BUFFER_SAVE,
        }
    }

    fn of(method: &str) -> Option<Self> {
        [
            Self::WriteFile,
            Self::ApplyPatch,
            Self::CreateDirectory,
            Self::Delete,
            Self::Move,
            Self::Copy,
            Self::ApplyChangeset,
            Self::BufferSave,
        ]
        .into_iter()
        .find(|route| route.method() == method)
    }

    /// The workspace this route's request names, from its own deserialized DTO.
    ///
    /// Returns `None` when the request does not deserialize as this route's request type — the
    /// invalid case, which must not gain a resource lane at all.
    fn declared_workspace(self, request: &JsonRpcRequest) -> Option<WorkspaceId> {
        match self {
            Self::WriteFile => {
                typed_params::<FsWriteFileRequest>(request).map(|it| it.workspace_id)
            }
            Self::ApplyPatch => {
                typed_params::<FsApplyPatchRequest>(request).map(|it| it.workspace_id)
            }
            Self::CreateDirectory => {
                typed_params::<FsCreateDirectoryRequest>(request).map(|it| it.workspace_id)
            }
            Self::Delete => typed_params::<FsDeleteRequest>(request).map(|it| it.workspace_id),
            Self::Move => typed_params::<FsMoveRequest>(request).map(|it| it.workspace_id),
            Self::Copy => typed_params::<FsCopyRequest>(request).map(|it| it.workspace_id),
            Self::ApplyChangeset => {
                typed_params::<FsApplyChangesetRequest>(request).map(|it| it.workspace_id)
            }
            Self::BufferSave => {
                typed_params::<BufferSaveCommand>(request).map(|it| it.workspace_id)
            }
        }
    }

    /// The raw target texts this request really declares, from its deserialized DTO.
    ///
    /// Returns `None` when the request does not deserialize as this route's request type, which is
    /// the invalid/incomplete case: those keep their original rejection and must not gain resource
    /// eligibility from a mere string.
    fn declared_targets(
        self,
        request: &JsonRpcRequest,
        workspace: &RawWorkspace,
    ) -> Option<Vec<RawTarget>> {
        match self {
            Self::WriteFile => {
                let typed: FsWriteFileRequest = typed_params(request)?;
                Some(vec![RawTarget::file(typed.target.path, workspace)])
            }
            Self::ApplyPatch => {
                let typed: FsApplyPatchRequest = typed_params(request)?;
                Some(vec![RawTarget::file(typed.target.path, workspace)])
            }
            Self::CreateDirectory => {
                let typed: FsCreateDirectoryRequest = typed_params(request)?;
                Some(vec![RawTarget::slot(typed.target.path, workspace)])
            }
            Self::Delete => {
                let typed: FsDeleteRequest = typed_params(request)?;
                // A recursive delete really removes a directory and everything in it, so it takes
                // the subtree scope. A non-recursive delete names one entry.
                Some(vec![if typed.recursive {
                    RawTarget::subtree(typed.target.path, workspace)
                } else {
                    RawTarget::slot(typed.target.path, workspace)
                }])
            }
            Self::Move => {
                // BOTH ends. A move is a write to the source's entry, a write to the
                // destination's entry, and a change to the destination's object.
                let typed: FsMoveRequest = typed_params(request)?;
                Some(vec![
                    RawTarget::slot(typed.source.path.clone(), workspace),
                    RawTarget::object_and_slot(typed.source.path, workspace),
                    RawTarget::slot(typed.destination.path.clone(), workspace),
                    RawTarget::object_and_slot(typed.destination.path, workspace),
                ])
            }
            Self::Copy => {
                // The full set: the destination write, and the source read that the copy's
                // content is taken from. The source is read-only, so it never blocks another
                // reader, but a writer of the source must still be ordered against it.
                let typed: FsCopyRequest = typed_params(request)?;
                Some(vec![
                    RawTarget::slot(typed.destination.path.clone(), workspace),
                    RawTarget::object_and_slot(typed.destination.path, workspace),
                    RawTarget::read(typed.source.path, workspace),
                ])
            }
            Self::ApplyChangeset => {
                // The whole set, in the declared order. An empty changeset declares no resource,
                // so it takes none: an empty set cannot conflict with anything.
                let typed: FsApplyChangesetRequest =
                    typed_params::<FsApplyChangesetRequest>(request)?;
                let mut targets = Vec::new();
                for operation in typed.operations {
                    match operation {
                        FsChangesetOperation::WriteFile { target, .. }
                        | FsChangesetOperation::ApplyPatch { target, .. } => {
                            targets.push(RawTarget::file(target.path, workspace));
                        }
                        FsChangesetOperation::CreateDirectory { target } => {
                            targets.push(RawTarget::slot(target.path, workspace));
                        }
                        FsChangesetOperation::Delete { target, recursive, .. } => {
                            targets.push(if recursive {
                                RawTarget::subtree(target.path, workspace)
                            } else {
                                RawTarget::slot(target.path, workspace)
                            });
                        }
                        FsChangesetOperation::Copy { source, destination, .. } => {
                            targets.push(RawTarget::slot(destination.path.clone(), workspace));
                            targets.push(RawTarget::object_and_slot(destination.path, workspace));
                            targets.push(RawTarget::read(source.path, workspace));
                        }
                        FsChangesetOperation::Move { source, destination, .. } => {
                            targets.push(RawTarget::slot(source.path.clone(), workspace));
                            targets.push(RawTarget::object_and_slot(source.path, workspace));
                            targets.push(RawTarget::slot(destination.path.clone(), workspace));
                            targets.push(RawTarget::object_and_slot(destination.path, workspace));
                        }
                    }
                }
                Some(targets)
            }
            Self::BufferSave => {
                let typed: BufferSaveCommand = typed_params(request)?;
                Some(vec![RawTarget::file(typed.path, workspace)])
            }
        }
    }
}

/// One raw target a request declares, before it is turned into a verified resource.
///
/// The lane runs before any handler, so no handle exists yet and no identity can be read. This
/// carries the declared allocation shape only; the real identity is resolved by the protection
/// layer, which is the only place that may open a file.
///
/// # Why the workspace is carried and how it is used
///
/// The previous version dropped the workspace entirely and keyed the lane on the relative path
/// text alone. That produced the measured false conflict: two **unrelated** workspaces both holding
/// a `note.txt` got the same lane, so the second waited for the first even though nothing about them
/// was related.
///
/// The fix is deliberately not "add `WorkspaceId` and treat it as a global resource key". A
/// `WorkspaceId` is ownership, and using it as the identity would split one physical file reached
/// through two workspaces into two lanes — the mirror-image error, and the one the resource model
/// exists to prevent. What the lane needs is a fact it can actually compare at this stage: the
/// **canonical root** the workspace resolves to. Two workspaces over one root therefore share a
/// lane (they really are one tree), while two workspaces over different roots never do.
///
/// When the canonical root is not known at the point the lane is chosen, the `WorkspaceId` is used
/// as the anchor instead. That is the conservative direction: it can only make two requests that
/// really are one tree wait for each other, never let two unrelated ones run together.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RawTarget {
    path: WorkspacePath,
    scope: RawScope,
    access: RawAccess,
    /// The canonical root this target was reached through, when the caller knows it.
    root: Option<String>,
}

/// The workspace a raw target belongs to, as a comparable anchor.
///
/// Kept on the **lane key** rather than on every target, because one request names one workspace:
/// a move's two ends are in one tree, and a request that somehow named two would be invalid before
/// it reached a lane.
#[derive(Clone, Debug, Eq, PartialEq)]
struct RawWorkspace {
    /// The canonical root when known, otherwise the workspace id.
    ///
    /// The two are kept in one field on purpose: a value that could be either must not compare equal
    /// to a different kind of value, and folding them into one string makes that impossible.
    anchor: String,
    /// Whether `anchor` is a canonical root (compare it as a real path) or a workspace id.
    ///
    /// A real root is compared under the Windows case rule, because two spellings of one directory
    /// are one directory. A workspace id is compared exactly.
    is_root: bool,
}

impl RawWorkspace {
    /// A workspace the caller could only name by id.
    ///
    /// Strictly weaker than a canonical root, and deliberately so: two workspaces over one root that
    /// both fall back here compare as different and wait for each other, which is the safe
    /// direction. The stronger fact is used wherever the caller has it.
    fn of_id(workspace_id: WorkspaceId) -> Self {
        Self { anchor: workspace_id.to_string(), is_root: false }
    }

    /// Whether two targets are in the same tree.
    fn same_tree(&self, other: &Self) -> bool {
        if self.is_root != other.is_root {
            return false;
        }
        self.anchor == other.anchor
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RawScope {
    /// One file: the object it names and the entry that names it.
    File,
    /// One directory entry, which may be a create or a delete-recreate.
    Slot,
    /// A directory and everything inside it.
    Subtree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RawAccess {
    Read,
    Write,
}

impl RawTarget {
    fn file(path: WorkspacePath, workspace: &RawWorkspace) -> Self {
        Self {
            path,
            scope: RawScope::File,
            access: RawAccess::Write,
            root: Some(workspace.anchor.clone()),
        }
    }

    fn slot(path: WorkspacePath, workspace: &RawWorkspace) -> Self {
        Self {
            path,
            scope: RawScope::Slot,
            access: RawAccess::Write,
            root: Some(workspace.anchor.clone()),
        }
    }

    fn subtree(path: WorkspacePath, workspace: &RawWorkspace) -> Self {
        Self {
            path,
            scope: RawScope::Subtree,
            access: RawAccess::Write,
            root: Some(workspace.anchor.clone()),
        }
    }

    /// Both the entry and the object at one path, which is what an overwrite or a move really
    /// touches: the entry decides whether the target exists, the object is what gets replaced.
    fn object_and_slot(path: WorkspacePath, workspace: &RawWorkspace) -> Self {
        Self {
            path,
            scope: RawScope::File,
            access: RawAccess::Write,
            root: Some(workspace.anchor.clone()),
        }
    }

    fn read(path: WorkspacePath, workspace: &RawWorkspace) -> Self {
        Self {
            path,
            scope: RawScope::File,
            access: RawAccess::Read,
            root: Some(workspace.anchor.clone()),
        }
    }
}

fn split_parent(path: &WorkspacePath) -> (&str, &str) {
    match path.as_str().rsplit_once('/') {
        Some((parent, leaf)) => (parent, leaf),
        None => ("", path.as_str()),
    }
}

/// The resource lane key: the **whole** declared resource set of one request, in one tree.
///
/// Two requests share a lane only when they are in the same tree **and** their sets overlap in a way
/// that really conflicts. The old key was one lowercased path string with no tree at all, so two
/// unrelated workspaces' same-named files shared a lane *and* a move's two ends did not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResourceLaneKey {
    workspace: RawWorkspace,
    targets: Vec<RawTarget>,
}

impl ResourceLaneKey {
    fn of(workspace: RawWorkspace, targets: Vec<RawTarget>) -> Self {
        Self { workspace, targets }
    }

    /// Whether two requests must not run at the same time on this connection.
    ///
    /// Two requests in different trees are unrelated and never share a lane, which is the measured
    /// false conflict this removes: two workspaces over different roots each holding a `note.txt`
    /// must not wait for one another.
    fn overlaps(&self, other: &Self) -> bool {
        if !self.workspace.same_tree(&other.workspace) {
            return false;
        }
        self.targets.iter().any(|mine| {
            other.targets.iter().any(|theirs| {
                // A move or a copy names both ends of one path; compare every pairing.
                same_declared_allocation(mine, theirs)
            })
        })
    }
}

/// Whether two declared allocations really touch the same thing.
///
/// Before any handle exists this can only be decided from what the request declared, and the rule
/// must never be *narrower* than the truth: a lane that serialises two requests which turn out to
/// be unrelated is only stricter, while a lane that lets two requests which turn out to be the same
/// file run together is unsafe. So a same-parent same-leaf pair always overlaps, a subtree overlaps
/// anything under it, and a different parent with a different leaf does not — which is what keeps
/// unrelated files parallel inside **and** across workspaces.
///
/// Both sides must already be in the same tree: this function compares declarations *within* one
/// workspace, and the tree comparison is what [`ResourceLaneKey::overlaps`] does first.
fn same_declared_allocation(mine: &RawTarget, theirs: &RawTarget) -> bool {
    if mine.access == RawAccess::Read && theirs.access == RawAccess::Read {
        return false;
    }
    // A target that could not be anchored to a root is compared on its id; two of those are the
    // same tree only when the ids are equal, which the lane key has already established.
    match (&mine.root, &theirs.root) {
        (Some(mine), Some(theirs)) if mine != theirs => return false,
        _ => {}
    }
    let (my_parent, my_leaf) = split_parent(&mine.path);
    let (their_parent, their_leaf) = split_parent(&theirs.path);
    if my_parent == their_parent && my_leaf == their_leaf {
        return true;
    }
    // A subtree request reaches everything under its directory, by declared parent chain.
    let subtree_reaches = |subtree: &RawTarget, other: &RawTarget| {
        if subtree.scope != RawScope::Subtree || other.access == RawAccess::Read {
            return false;
        }
        let (subtree_parent, subtree_leaf) = split_parent(&subtree.path);
        let (other_parent, other_leaf) = split_parent(&other.path);
        let subtree_declared = join_declared(subtree_parent, subtree_leaf);
        let other_declared = join_declared(other_parent, other_leaf);
        other_declared == subtree_declared
            || other_declared.starts_with(&format!("{subtree_declared}/"))
            // The other side may be the entry that names the subtree's own directory.
            || subtree_declared
                .rsplit_once('/')
                .is_some_and(|(parent, _)| parent == other_declared)
    };
    subtree_reaches(mine, theirs) || subtree_reaches(theirs, mine)
}

fn join_declared(parent: &str, leaf: &str) -> String {
    if parent.is_empty() { leaf.to_owned() } else { format!("{parent}/{leaf}") }
}

/// The resource a request declares, if it declares one through its own registered DTO.
///
/// A missing, incomplete or invalid parameter set yields `None`, and the request keeps the ordered
/// lane - being stricter than asked is always safe, while guessing a target is not. Nothing here
/// grants eligibility from a mere string: the parameters must deserialize as the route's real
/// request type, which is the same validation the handler applies. An invalid request therefore
/// keeps both its original rejection *and* its original ordered lane.
///
/// # The workspace anchor, and what it is used for
///
/// The DTO names its workspace by id, which is ownership rather than identity. Using the id as the
/// lane's tree anchor is therefore the conservative fallback, not the intent: two workspaces over
/// one canonical root that both fall back here compare as different and wait for each other, which
/// is safe but stricter than the confirmed behaviour. Where the canonical root is known the lane
/// uses **that**, so those two really do share a lane while two workspaces over different roots
/// never do. The registry of open workspaces is asked here rather than on the handler path because
/// the lane is chosen before any handler runs.
#[must_use]
pub(crate) fn file_target_scope(request: &JsonRpcRequest) -> Option<FileTargetLaneKey> {
    if is_concurrent_read_only(request)
        || CONTROL_ROUTES.contains(&request.method.as_str())
        || CONNECTION_ORDERED_ROUTES.contains(&request.method.as_str())
    {
        return None;
    }
    let route = FileRoute::of(&request.method)?;
    let workspace_id = route.declared_workspace(request)?;
    let workspace = RawWorkspace::of_id(workspace_id);
    let targets = route.declared_targets(request, &workspace)?;
    if targets.is_empty() {
        return None;
    }
    Some(FileTargetLaneKey::of(workspace, targets))
}

/// The lane key a request must use.
#[must_use]
pub(crate) fn lane_key_for(request: &JsonRpcRequest) -> LaneKey {
    if CONTROL_ROUTES.contains(&request.method.as_str()) {
        return LaneKey::Connection(Lane::Control);
    }
    if CONNECTION_ORDERED_ROUTES.contains(&request.method.as_str()) {
        return LaneKey::Connection(Lane::Serial);
    }
    if let Some(terminal) = terminal_input::scope(request) {
        return LaneKey::Terminal(terminal);
    }
    if let Some(session) = session_write_scope(request) {
        return LaneKey::Session(session);
    }
    if let Some(target) = file_target_scope(request) {
        return LaneKey::FileTarget(target);
    }
    if let Some(publication) = publication_scope::scope(request) {
        return LaneKey::Publication(publication);
    }
    if is_concurrent_read_only(request) {
        LaneKey::Connection(Lane::Concurrent)
    } else {
        LaneKey::Connection(Lane::Serial)
    }
}

#[path = "ipc_session/publication_scope.rs"]
mod publication_scope;

/// The lane a request must use. The session lane is reported as one lane even though it
/// holds one key per session; use [`lane_key_for`] where the key matters.
#[cfg(test)]
#[must_use]
pub(crate) fn lane_for(request: &JsonRpcRequest) -> Lane {
    lane_key_for(request).lane()
}

/// Routes the registry marks read-only that still mutate *this connection's own*
/// registry, so their arrival order must stay their execution order: a subscribe
/// and an unsubscribe for the same projection end in whichever state runs last.
///
/// The route table records idempotency ("is this safe to retry"), not ordering,
/// so ordering is decided here instead of being guessed from a name prefix.
/// These are exactly the routes the exit review names as same-resource ordered;
/// the durable fix is an explicit ordering field in the table itself.
const CONNECTION_ORDERED_ROUTES: [&str; 2] = ["projection.subscribe", "projection.unsubscribe"];

/// Declared connection control: routes whose whole purpose is to change what
/// happens to *other* requests, so they must not wait behind a long ordered
/// handler. `aura.cancel` is the case that matters — a write that takes tens of
/// seconds used to hold the serial lane, so a cancellation for a *different*
/// request was executed only after that write finished and was then answered
/// `TooLateToCancel`.
///
/// Ordering semantics of this lane: its routes are not ordered against business
/// work and not ordered among themselves either, because each one only reads and
/// marks one other request's ledger entry under the ledger lock. They are kept off
/// the concurrent read-only lane so a read-only burst can never fill the lane a
/// cancellation needs, and off the serial lane so ordered work cannot block them.
/// Being on this lane skips nothing else: the request still takes its ledger
/// entry, its invocation claim, its handler and one reply.
///
/// A route joins this set only when it provably has no ordering relationship with
/// any resource write; everything unrecognised stays on the serial lane.
const CONTROL_ROUTES: [&str; 1] = ["aura.cancel"];

/// Terminal state of one dispatched handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HandlerTerminal {
    /// The handler answered; the ledger should record this terminal.
    Answered(aura_runtime_contract::IpcRequestTerminal),
    /// The handler failed before queueing any reply. The reader owes this
    /// request exactly one typed error — never a Failed terminal with nothing
    /// sent, which would leave the peer waiting for its own timeout.
    Unreplied,
    /// The reply path itself failed, so this channel can no longer deliver
    /// anything and must be retired instead of answered twice.
    TransportFailed,
    /// The handler panicked. The reply can no longer be trusted, so the session
    /// is retired rather than pretending the request failed cleanly.
    Panicked,
}

/// A request waiting for a lane. The ticket is session-local, so two requests
/// that reuse a correlation id can never share one lane.
struct QueuedRequest {
    ticket: u64,
    /// The slot this request needs. Several queued requests may share one key, and then
    /// they run in arrival order; requests with different keys do not wait for each other.
    key: LaneKey,
    request: JsonRpcRequest,
}

/// A request handed out by [`IpcSession::take_ready`]. The caller owns the
/// request while it runs and returns the ticket to [`IpcSession::attach`].
#[derive(Debug)]
pub(crate) struct AdmittedRequest {
    ticket: u64,
    lane: Lane,
    request: JsonRpcRequest,
}

impl AdmittedRequest {
    /// The lane this request was admitted to.
    #[must_use]
    pub(crate) fn lane(&self) -> Lane {
        self.lane
    }

    /// Split into the lane ticket and the request.
    #[must_use]
    pub(crate) fn into_parts(self) -> (u64, JsonRpcRequest) {
        (self.ticket, self.request)
    }

    /// The request's correlation id, without consuming the admission.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn request_id(&self) -> CorrelationId {
        self.request.id
    }
}

/// A handler currently running on a lane. The handle is absent only between
/// taking a lane and starting the task.
struct RunningRequest {
    request_id: CorrelationId,
    handle: Option<tokio::task::JoinHandle<HandlerTerminal>>,
}

/// Why a request could not be accepted for concurrent execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Admission {
    /// Accepted; the caller should keep reading.
    Accepted,
    /// The lane and its queue are both full. The caller must answer this
    /// request with an overload error instead of blocking the read loop.
    Overloaded { lane: Lane },
}

impl Admission {
    /// The typed error for [`Admission::Overloaded`]. There is no existing
    /// overload contract in the runtime envelope set, so this is the minimal
    /// addition: an `Unavailable`, retryable failure that names the reason.
    pub(crate) fn overloaded_error(
        request_id: CorrelationId,
        lane: Lane,
    ) -> aura_runtime_contract::IpcErrorEnvelope {
        let message = match lane {
            Lane::Concurrent => {
                "Aura Core is already running the maximum number of concurrent read-only \
                 requests; retry this request after the current ones finish."
            }
            Lane::Serial => {
                "Aura Core is already running the maximum number of ordered requests; retry \
                 this request after the current one finishes."
            }
            Lane::Session => {
                "Aura Core is already running the maximum number of session requests; retry \
                 this request after the current one finishes."
            }
            Lane::Control => {
                "Aura Core is already handling the maximum number of control requests; retry \
                 this request after the current ones finish."
            }
        };
        aura_runtime_contract::IpcErrorEnvelope {
            code: ErrorCode::new("ipc.handler_overloaded")
                .unwrap_or_else(|_| ErrorCode::new("ipc.error").expect("static fallback code")),
            category: ErrorCategory::Unavailable,
            message: message.to_owned(),
            retryable: true,
            correlation_id: request_id,
            terminal: aura_runtime_contract::IpcRequestTerminal::Failed,
            details: std::collections::BTreeMap::new(),
        }
    }
}

/// A parked handler remains owned by this connection until completion or teardown.
struct WaitingRequest {
    key: LaneKey,
    running: RunningRequest,
    resume: Option<tokio::sync::oneshot::Sender<()>>,
}

impl RunningRequest {
    fn is_finished(&self) -> bool {
        self.handle.as_ref().is_some_and(tokio::task::JoinHandle::is_finished)
    }

    fn reap(self, finished: &mut Vec<(CorrelationId, HandlerTerminal)>) {
        let Some(handle) = self.handle else {
            return;
        };
        let terminal = handle.now_or_never().unwrap_or(HandlerTerminal::Panicked);
        finished.push((self.request_id, terminal));
    }
}

/// Keys preserve related request order; the execution bound applies to running tasks.
struct LaneState {
    running: BTreeMap<u64, (LaneKey, RunningRequest)>,
    queued: VecDeque<QueuedRequest>,
    max_concurrent: usize,
    /// Upper bound on work in flight *plus* work waiting.
    max_admitted: usize,
    lane: Lane,
    /// Requests that are parked in a **resource wait** rather than executing.
    ///
    /// # What the separation buys
    ///
    /// A request whose declared resource conflicts with a live one is not running: it is waiting for
    /// another execution to release the real object. It therefore must not occupy a
    /// [`LaneState::max_concurrent`] execution slot, because a slot is the connection's permission to
    /// *do work*, and a wait does none. Holding the slot is what let eight related waiters — arriving
    /// through several workspaces or several names for one object, so with different lane keys —
    /// exhaust the connection and starve an unrelated file request and the ordinary session writes.
    ///
    /// A wait ticket keeps everything that makes the wait fair and bounded: its own identity (the
    /// lane ticket), its position in the original arrival order, its original deadline and its
    /// cancellation/disconnect handover. What it does not keep is a claim on execution capacity. When
    /// the object is released the request is re-admitted to the ordinary path and **then** competes
    /// for a real slot, so "has a slot" again means exactly "is executing".
    waiting: BTreeMap<u64, WaitingRequest>,
    /// The bound on simultaneous waits, so waiting is bounded work rather than an unbounded park.
    max_waiting: usize,
}

impl LaneState {
    fn new(lane: Lane, max_concurrent: usize, max_admitted: usize) -> Self {
        let max_concurrent = max_concurrent.max(1);
        Self {
            running: BTreeMap::new(),
            queued: VecDeque::new(),
            max_concurrent,
            max_admitted: max_admitted.max(max_concurrent),
            lane,
            waiting: BTreeMap::new(),
            // The wait bound only applies to the lane that can carry a resource wait at all; every
            // other lane gets zero, so a wait cannot be recorded where it has no meaning.
            max_waiting: if lane == Lane::Session { MAX_RESOURCE_WAITERS } else { 0 },
        }
    }

    fn admitted(&self) -> usize {
        // A waiting request is admitted work for *this connection*, so it is still counted against
        // the lane's own queue bound -- the bound that stops an unbounded burst. It is deliberately
        // not counted against `max_concurrent`, which is the execution capacity.
        self.running.len().saturating_add(self.queued.len()).saturating_add(self.waiting.len())
    }

    fn key_is_running(&self, key: &LaneKey) -> bool {
        self.running.values().any(|(running, _)| running.holds(key))
            || self.waiting.values().any(|waiting| waiting.key.holds(key))
    }

    /// Whether this lane gives a key exactly one slot.
    ///
    /// True wherever a key names *who must not run twice at once*: the ordered lane, the
    /// control lane and each session's lane. False for the read-only lane, whose requests
    /// deliberately share one key and are meant to run beside each other.
    fn serialised_by_key(&self) -> bool {
        self.lane != Lane::Concurrent
    }

    fn admit(&mut self, ticket: u64, key: LaneKey, request: JsonRpcRequest) -> Admission {
        if self.admitted() >= self.max_admitted {
            return Admission::Overloaded { lane: self.lane };
        }
        self.queued.push_back(QueuedRequest { ticket, key, request });
        Admission::Accepted
    }

    /// Take as many queued requests as free keys allow. The key is reserved before the
    /// caller starts the task, so a burst cannot be admitted twice.
    ///
    /// On a lane that gives a key one slot, the queue is walked once in arrival order and a
    /// key is considered only at its **first** queued request: if that one cannot run
    /// because its key is busy, nothing later of the same key may either - so a session's
    /// requests never overtake each other. Requests behind it are still served, which is
    /// what lets B run while A waits. The read-only lane has no such rule: its requests are
    /// meant to run beside each other, and the only bound is its own capacity.
    fn take_ready(&mut self) -> Vec<AdmittedRequest> {
        let mut ready = Vec::new();
        let serialised = self.serialised_by_key();
        let mut blocked_keys: Vec<LaneKey> = Vec::new();
        let mut index = 0;
        let mut taken = 0;
        // What the lane can hand out, decided once. `self.running` grows as this round
        // hands handlers out, so counting it inside the loop would count this round's own
        // work twice: the lane would start half of what it is allowed to.
        let free_slots = self.max_concurrent.saturating_sub(self.running.len());
        loop {
            if index >= self.queued.len() || taken >= free_slots {
                break;
            }
            let key = self.queued[index].key.clone();
            if serialised {
                if blocked_keys.iter().any(|blocked| blocked.holds(&key)) {
                    // A later request of a key already passed over this round.
                    index += 1;
                    continue;
                }
                if self.key_is_running(&key) {
                    // The key's first queued request cannot run; nothing of this key may.
                    blocked_keys.push(key);
                    index += 1;
                    continue;
                }
            }
            let queued = self.queued.remove(index).expect("index is inside the queue");
            let lane = key.lane();
            self.running.insert(
                queued.ticket,
                (key, RunningRequest { request_id: queued.request.id, handle: None }),
            );
            taken += 1;
            ready.push(AdmittedRequest { ticket: queued.ticket, lane, request: queued.request });
        }
        self.resume_ready();
        ready
    }

    fn attach(&mut self, ticket: u64, handle: tokio::task::JoinHandle<HandlerTerminal>) {
        match self.running.get_mut(&ticket) {
            Some((_, running)) if running.handle.is_none() => running.handle = Some(handle),
            Some(_) => {
                // Two tasks cannot own one lane; the second reply would be lost.
                trace("core.handler.duplicate_attach", &format!("ticket={ticket}"));
                handle.abort();
            }
            None => {
                // The request was already reaped; nothing to attach.
                handle.abort();
            }
        }
    }

    /// Records that a running request is now **waiting on a real resource**, and frees its
    /// execution slot.
    ///
    /// The caller has established that the request is inside the real common admission and has not
    /// produced any effect yet. The request's task keeps running — it is parked inside the
    /// admission, which is exactly where its cancellation and deadline live — but it no longer
    /// occupies a slot, so unrelated work on this same connection can proceed.
    ///
    /// Returns whether the ticket was really running here. A ticket that is not running is not an
    /// error: a wait can be reported after the request already finished, and that must be a no-op
    /// rather than a new entry.
    fn begin_waiting(&mut self, ticket: u64) -> bool {
        let Some((_, running)) = self.running.get(&ticket) else {
            return false;
        };
        if self.waiting.len() >= self.max_waiting {
            // Waiting is bounded work. Over the bound the request keeps its slot and its own real
            // deadline, which is the honest outcome: this connection will not park more waits.
            return false;
        }
        let request_id = running.request_id;
        let (key, running) = self.running.remove(&ticket).expect("running ticket exists");
        self.waiting.insert(ticket, WaitingRequest { key, running, resume: None });
        trace(
            "core.lane.resource_wait",
            &format!(
                "lane={:?} id={request_id} ticket={ticket} waiting={}",
                self.lane,
                self.waiting.len()
            ),
        );
        true
    }

    fn request_resume(&mut self, ticket: u64, permit: tokio::sync::oneshot::Sender<()>) {
        if let Some(waiting) = self.waiting.get_mut(&ticket) {
            // At most one outstanding retry per task. An expired receiver cannot take capacity.
            waiting.resume = Some(permit);
        } else if self.running.contains_key(&ticket) {
            // The wait cap kept the original slot; the request still owns that capacity.
            let _ = permit.send(());
        }
    }

    /// Unrelated new work is served before polling retries. Same-key work stays behind its
    /// waiter, and ready waiters compete in their original ticket order. Otherwise eight
    /// contended polling retries could take every freed slot again on every reader turn.
    fn resume_ready(&mut self) {
        let tickets = self.waiting.keys().copied().collect::<Vec<_>>();
        for ticket in tickets {
            if self.running.len() >= self.max_concurrent {
                break;
            }
            let waiting = &self.waiting[&ticket];
            if !waiting.resume.as_ref().is_some_and(|permit| !permit.is_closed()) {
                continue;
            }
            if self.running.values().any(|(key, _)| key.holds(&waiting.key))
                || self.waiting.range(..ticket).any(|(_, earlier)| earlier.key.holds(&waiting.key))
            {
                continue;
            }
            let mut waiting = self.waiting.remove(&ticket).expect("waiting ticket exists");
            let permit = waiting.resume.take().expect("ready waiter has a receiver");
            self.running.insert(ticket, (waiting.key, waiting.running));
            let _ = permit.send(());
        }
    }

    #[cfg(test)]
    fn waiting_tickets(&self) -> Vec<u64> {
        self.waiting.keys().copied().collect()
    }

    /// Collect every handler that finished since the last call.
    fn poll_completions(&mut self, finished: &mut Vec<(CorrelationId, HandlerTerminal)>) {
        let ready = self
            .running
            .iter()
            .filter(|(_, (_, running))| running.is_finished())
            .map(|(ticket, _)| *ticket)
            .collect::<Vec<_>>();
        for ticket in ready {
            if let Some((_, running)) = self.running.remove(&ticket) {
                running.reap(finished);
            }
        }
        let ready = self
            .waiting
            .iter()
            .filter(|(_, waiting)| waiting.running.is_finished())
            .map(|(ticket, _)| *ticket)
            .collect::<Vec<_>>();
        for ticket in ready {
            if let Some(waiting) = self.waiting.remove(&ticket) {
                waiting.running.reap(finished);
            }
        }
    }

    /// Teardown owns both executing and waiting tasks. Dropping a JoinHandle would detach
    /// a waiter and could let it write after this connection was already retired.
    fn drain_handles(&mut self, handles: &mut Vec<tokio::task::JoinHandle<HandlerTerminal>>) {
        for (_, (_, running)) in std::mem::take(&mut self.running) {
            if let Some(handle) = running.handle {
                handles.push(handle);
            }
        }
        for (_, waiting) in std::mem::take(&mut self.waiting) {
            if let Some(handle) = waiting.running.handle {
                handles.push(handle);
            }
        }
        self.queued.clear();
    }
}

#[path = "ipc_session/resource_wait.rs"]
mod resource_wait;
pub(crate) use resource_wait::{
    RESOURCE_WAIT_HANDLER, ResourceWaitHandler, ResourceWaitSignal, ResourceWaitSignals,
    current_resource_wait_handler, with_resource_wait_handler,
};

pub(crate) struct IpcSession {
    lanes: [LaneState; Lane::COUNT],
    /// Next lane ticket to hand out. Never reused, so a ticket always names one
    /// request even when correlation ids repeat.
    next_ticket: u64,
    secrets: BTreeMap<CorrelationId, aura_windows_platform::SecretBytes>,
}

impl IpcSession {
    pub(crate) fn new() -> Self {
        Self::with_limits(MAX_CONCURRENT_READ_ONLY, MAX_CONCURRENT_READ_ONLY + MAX_QUEUED_READ_ONLY)
    }

    /// `max_concurrent` read-only lanes, and at most `max_admitted` read-only
    /// requests in flight or waiting together. The ordered, session and control lanes
    /// keep their own fixed bounds: the ordered lane runs one at a time with
    /// [`MAX_QUEUED_SERIAL`] waiting, the session lane runs one handler per session with
    /// [`MAX_CONCURRENT_SESSION_WRITES`] sessions in flight and
    /// [`MAX_QUEUED_SESSION_WRITES`] waiting in total, and the control lane
    /// [`MAX_CONCURRENT_CONTROL`] with [`MAX_QUEUED_CONTROL`] waiting, so a
    /// read-only burst cannot consume the lane a cancellation needs.
    pub(crate) fn with_limits(max_concurrent: usize, max_admitted: usize) -> Self {
        Self {
            lanes: [
                LaneState::new(Lane::Concurrent, max_concurrent, max_admitted),
                LaneState::new(Lane::Serial, 1, 1 + MAX_QUEUED_SERIAL),
                // The connection-level bound counts a session's work in flight *and*
                // waiting, so session-bound writes are refused on overload rather than
                // queued without limit.
                LaneState::new(
                    Lane::Session,
                    MAX_CONCURRENT_SESSION_WRITES,
                    MAX_CONCURRENT_SESSION_WRITES + MAX_QUEUED_SESSION_WRITES,
                ),
                LaneState::new(
                    Lane::Control,
                    MAX_CONCURRENT_CONTROL,
                    MAX_CONCURRENT_CONTROL + MAX_QUEUED_CONTROL,
                ),
            ],
            next_ticket: 0,
            secrets: BTreeMap::new(),
        }
    }

    /// Accept a request for its lane.
    ///
    /// Never blocks: the request either takes a lane, waits in that lane's
    /// bounded queue, or is reported as overloaded so the caller can answer it
    /// and keep reading. The bound counts work in flight *and* work waiting, so
    /// a burst cannot grow without limit.
    pub(crate) fn admit(&mut self, request: JsonRpcRequest) -> Admission {
        let key = lane_key_for(&request);
        let ticket = self.next_ticket;
        self.next_ticket = self.next_ticket.wrapping_add(1);
        self.lanes[key.lane().index()].admit(ticket, key, request)
    }

    /// The same ordering and capacity as ordinary requests. Secret payloads never enter JSON.
    pub(crate) fn admit_secret(
        &mut self,
        request: JsonRpcRequest,
        secret: aura_windows_platform::SecretBytes,
    ) -> Admission {
        let id = request.id;
        let admission = self.admit(request);
        if admission == Admission::Accepted {
            self.secrets.insert(id, secret);
        }
        admission
    }
    pub(crate) fn take_secret(
        &mut self,
        id: CorrelationId,
    ) -> Option<aura_windows_platform::SecretBytes> {
        self.secrets.remove(&id)
    }

    /// Take every queued request that has a free lane. The caller starts the
    /// task, so it keeps ownership of the handler body and its reply, and the
    /// runtime handle never has to be turned back into an inline future.
    pub(crate) fn take_ready(&mut self) -> Vec<AdmittedRequest> {
        let mut ready = Vec::new();
        for lane in &mut self.lanes {
            ready.extend(lane.take_ready());
        }
        ready
    }

    /// Record the task that runs an already-taken request.
    pub(crate) fn attach(&mut self, ticket: u64, handle: tokio::task::JoinHandle<HandlerTerminal>) {
        for lane in &mut self.lanes {
            if lane.running.contains_key(&ticket) {
                lane.attach(ticket, handle);
                return;
            }
        }
        handle.abort();
    }

    /// Collect every handler that finished since the last call. Panics are
    /// reported as [`HandlerTerminal::Panicked`] instead of being dropped.
    pub(crate) fn poll_completions(&mut self) -> Vec<(CorrelationId, HandlerTerminal)> {
        let mut finished = Vec::new();
        for lane in &mut self.lanes {
            lane.poll_completions(&mut finished);
        }
        finished
    }

    /// Requests that are running right now, on either lane.
    #[must_use]
    pub(crate) fn running_count(&self) -> usize {
        self.lanes.iter().map(|lane| lane.running.len()).sum()
    }

    /// Ids of requests that were handed to a handler task, i.e. that really
    /// started. Used by disconnect cleanup to tell work that may have had an
    /// effect from requests that never left the queue.
    #[must_use]
    pub(crate) fn running_request_ids(&self) -> Vec<CorrelationId> {
        self.lanes
            .iter()
            .flat_map(|lane| {
                lane.running
                    .values()
                    .map(|(_, running)| running.request_id)
                    .chain(lane.waiting.values().map(|waiting| waiting.running.request_id))
            })
            .collect()
    }

    /// Requests waiting for a lane, on either lane.
    #[must_use]
    pub(crate) fn queued_count(&self) -> usize {
        self.lanes.iter().map(|lane| lane.queued.len()).sum()
    }

    /// Requests parked in a **resource wait**, on either lane.
    ///
    /// These are requests this connection still owes a reply for, and they are deliberately not
    /// counted as running: a wait is not executing, so it must not appear in
    /// [`Self::running_count`] — which is exactly the distinction that stops related waiters from
    /// exhausting the connection's execution capacity.
    #[must_use]
    pub(crate) fn waiting_count(&self) -> usize {
        self.lanes.iter().map(|lane| lane.waiting.len()).sum()
    }

    /// Records that one running request is now parked in a resource wait, freeing its slot.
    pub(crate) fn begin_resource_wait(&mut self, ticket: u64) -> bool {
        for lane in &mut self.lanes {
            if lane.running.contains_key(&ticket) {
                return lane.begin_waiting(ticket);
            }
        }
        false
    }

    /// The tickets currently parked in a resource wait, on either lane.
    ///
    /// Used by the scheduling regressions to state that a cancelled or budget-exhausted request is
    /// really gone from the wait set.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn waiting_tickets(&self) -> Vec<u64> {
        self.lanes.iter().flat_map(LaneState::waiting_tickets).collect()
    }

    /// Resource reports change scheduling state without giving up ownership of the handler.
    pub(crate) fn apply_wait_signals(&mut self, signals: Vec<ResourceWaitSignal>) {
        for signal in signals {
            match signal {
                ResourceWaitSignal::Entered { ticket } => {
                    self.begin_resource_wait(ticket);
                }
                ResourceWaitSignal::Resume { ticket, permit } => {
                    for lane in &mut self.lanes {
                        if lane.waiting.contains_key(&ticket) || lane.running.contains_key(&ticket)
                        {
                            lane.request_resume(ticket, permit);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Requests this session still owes a reply for.
    ///
    /// A **waiting** request is one of them: the connection still owes it a reply, and a teardown
    /// has to settle it. It is just not executing.
    #[must_use]
    pub(crate) fn is_busy(&self) -> bool {
        self.running_count() > 0 || self.queued_count() > 0 || self.waiting_count() > 0
    }

    /// One lane's counters, for a test that has to state which lane carried which work.
    ///
    /// It reports counts only — no request content, no key — so a test can assert the *shape* of the
    /// admission (which lane is busy, how much is queued, how much is waiting) without the production
    /// type having to expose its internals or a test reaching into private fields. The third number
    /// is the wait count, kept separate from the running count precisely so a test can state that
    /// waiters really are **not** occupying execution slots.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn lane_counts(&self, lane: Lane) -> (usize, usize) {
        let state = &self.lanes[lane.index()];
        (state.running.len(), state.queued.len())
    }

    /// One lane's counters including the wait count, for the scheduling regressions.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn lane_counts_with_waiting(&self, lane: Lane) -> (usize, usize, usize) {
        let state = &self.lanes[lane.index()];
        (state.running.len(), state.queued.len(), state.waiting.len())
    }

    /// Stop every running handler and drop both queues. The session is ending,
    /// so the replies can no longer be delivered; the caller closes the ledger
    /// separately. Queued work is *dropped*, never promoted into a new task.
    ///
    /// Prefer [`Self::shutdown_within`] on a connection teardown: a handler parked in
    /// synchronous work does not stop just because it was aborted, and an unbounded
    /// await on it is how a teardown outlives its budget.
    #[cfg(test)]
    pub(crate) async fn shutdown(&mut self) {
        self.secrets.clear();
        let mut handles = Vec::new();
        for lane in &mut self.lanes {
            lane.drain_handles(&mut handles);
        }
        for handle in &handles {
            handle.abort();
        }
        for handle in handles {
            let _ = handle.await;
        }
    }

    /// Ends the session inside `budget` and reports the handlers that did not stop.
    ///
    /// `abort()` only asks a task to stop at its next await point. A handler parked in
    /// synchronous work — a host API, a persistence write — never reaches one, so
    /// awaiting it is an unbounded wait that no budget can interrupt. That is how a
    /// connection teardown outlives the reclaim budget while the read loop is supposed
    /// to be closing out.
    ///
    /// So the wait is bounded here, and it is bounded without losing anything: this
    /// handle set can be polled without consuming a handle whose task is still running,
    /// so the handlers that did not stop keep their identity and are handed back to the
    /// caller. They are *not* cancelled, *not* released, and above all not reported as
    /// no-effect: the work is still real, and somebody has to keep owning it.
    pub(crate) async fn shutdown_within(
        &mut self,
        budget: std::time::Duration,
    ) -> Vec<tokio::task::JoinHandle<HandlerTerminal>> {
        self.secrets.clear();
        let mut handles = Vec::new();
        for lane in &mut self.lanes {
            lane.drain_handles(&mut handles);
        }
        for handle in &handles {
            handle.abort();
        }
        let deadline = tokio::time::Instant::now() + budget;
        let mut outstanding = Vec::new();
        let mut pending = handles.into_iter();
        while let Some(mut handle) = pending.next() {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            // Polling with a timeout does not consume the handle when it expires, so a
            // task that is still running keeps its identity and can be handed over.
            if tokio::time::timeout(remaining, &mut handle).await.is_err() {
                outstanding.push(handle);
                outstanding.extend(pending);
                break;
            }
        }
        outstanding
    }
}

/// Small extension so a finished handle can be consumed without awaiting twice.
trait NowOrNever<T> {
    fn now_or_never(self) -> Option<T>;
}
impl<T> NowOrNever<T> for tokio::task::JoinHandle<T> {
    fn now_or_never(mut self) -> Option<T> {
        let mut context = Context::from_waker(std::task::Waker::noop());
        match Pin::new(&mut self).poll(&mut context) {
            Poll::Ready(result) => Some(match result {
                Ok(value) => value,
                Err(_) => return None,
            }),
            Poll::Pending => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{ContentHash, EditorReservationId, Revision, SessionId, WorkspaceId};
    use aura_workspace_domain::{FsExpectedState, WorkspaceFileTarget};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn read_only_request_named(tail: char) -> JsonRpcRequest {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": format!("cor_01ARZ3NDEKTSV4RRFFQ69G5FA{tail}"),
            "method": "runtime.readiness",
            "params": {},
        }))
        .expect("request")
    }

    fn read_only_request() -> JsonRpcRequest {
        read_only_request_named('V')
    }

    fn command_request_named(tail: char) -> JsonRpcRequest {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": format!("cor_01ARZ3NDEKTSV4RRFFQ69G5FA{tail}"),
            "method": "fs.write_file",
            "params": {},
        }))
        .expect("request")
    }

    fn control_request_named(tail: char) -> JsonRpcRequest {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": format!("cor_01ARZ3NDEKTSV4RRFFQ69G5FA{tail}"),
            "method": "aura.cancel",
            "params": {},
        }))
        .expect("request")
    }

    /// Start every request `take_ready` handed out, running `body` per request.
    fn spawn_ready<B, F>(session: &mut IpcSession, body: B)
    where
        B: Fn() -> F,
        F: std::future::Future<Output = HandlerTerminal> + Send + 'static,
    {
        for admitted in session.take_ready() {
            let (ticket, _request) = admitted.into_parts();
            let handle = tokio::spawn(body());
            session.attach(ticket, handle);
        }
    }

    fn succeeded() -> HandlerTerminal {
        HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Succeeded)
    }

    async fn drain(
        session: &mut IpcSession,
        expected: usize,
    ) -> Vec<(CorrelationId, HandlerTerminal)> {
        let mut completed = Vec::new();
        for _ in 0..500 {
            completed.extend(session.poll_completions());
            if completed.len() >= expected {
                break;
            }
            tokio::task::yield_now().await;
        }
        completed
    }

    #[test]
    fn read_only_routes_come_from_the_registry_not_a_name_prefix() {
        let readiness = read_only_request();
        assert!(is_concurrent_read_only(&readiness));
        assert_eq!(lane_for(&readiness), Lane::Concurrent);
        let write = command_request_named('W');
        assert!(!is_concurrent_read_only(&write), "side-effecting routes stay serialised");
        assert_eq!(lane_for(&write), Lane::Serial);
    }

    #[test]
    fn connection_ordered_read_only_routes_are_serialised() {
        // Read-only per the registry, but both mutate this connection's
        // subscription set: for the same projection the last one to run decides
        // the state, so order must be preserved.
        for method in ["projection.subscribe", "projection.unsubscribe"] {
            let request: JsonRpcRequest = serde_json::from_value(serde_json::json!({
                "jsonrpc": "2.0",
                "id": "cor_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "method": method,
                "params": {},
            }))
            .expect("request");
            assert!(is_concurrent_read_only(&request), "{method} is read-only in the table");
            assert_eq!(lane_for(&request), Lane::Serial, "{method} must keep arrival order");
        }
        // A pure read on the same projection stays concurrent.
        let snapshot: JsonRpcRequest = serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "cor_01ARZ3NDEKTSV4RRFFQ69G5FAW",
            "method": "projection.snapshot",
            "params": {},
        }))
        .expect("request");
        assert_eq!(lane_for(&snapshot), Lane::Concurrent);
    }

    #[test]
    fn a_cancellation_gets_the_control_lane_instead_of_the_ordered_one() {
        let cancel = control_request_named('C');
        assert!(!is_concurrent_read_only(&cancel), "cancelling mutates state, it is not a read");
        assert_eq!(
            lane_for(&cancel),
            Lane::Control,
            "a cancellation must not wait behind ordered business work"
        );
        // Nothing else moved: an ordinary command stays ordered one at a time, and
        // a pure read stays concurrent.
        assert_eq!(lane_for(&command_request_named('W')), Lane::Serial);
        assert_eq!(lane_for(&read_only_request()), Lane::Concurrent);
    }

    #[tokio::test]
    async fn a_cancellation_is_admitted_while_the_ordered_lane_is_full() {
        let mut session = IpcSession::with_limits(1, 2);
        // One ordered handler running and the queue behind it full.
        for _ in 0..(1 + MAX_QUEUED_SERIAL) {
            assert_eq!(session.admit(command_request_named('V')), Admission::Accepted);
        }
        assert_eq!(
            session.admit(command_request_named('W')),
            Admission::Overloaded { lane: Lane::Serial },
            "the ordered lane is full"
        );
        // The control lane keeps its own budget, so a cancellation is admitted
        // instead of being refused — or worse, queued behind that ordered work.
        assert_eq!(session.admit(control_request_named('C')), Admission::Accepted);
        for _ in 1..(MAX_CONCURRENT_CONTROL + MAX_QUEUED_CONTROL) {
            assert_eq!(session.admit(control_request_named('D')), Admission::Accepted);
        }
        assert_eq!(
            session.admit(control_request_named('E')),
            Admission::Overloaded { lane: Lane::Control },
            "the control lane is bounded too, and says so locally"
        );
        let error = Admission::overloaded_error(CorrelationId::new(), Lane::Control);
        assert!(error.message.contains("control"));
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn slow_read_only_handlers_run_concurrently() {
        let mut session = IpcSession::with_limits(2, 4);
        let started = Arc::new(AtomicUsize::new(0));
        // A semaphore starts empty, so a permit added before a handler waits is
        // still observed: no wake-up race between the two lanes.
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        for tail in ['V', 'W'] {
            let admitted = session.admit(read_only_request_named(tail));
            assert_eq!(admitted, Admission::Accepted, "burst {tail} must be admitted");
        }
        let (started_for_body, gate_for_body) = (Arc::clone(&started), Arc::clone(&gate));
        spawn_ready(&mut session, move || {
            let (started, gate) = (Arc::clone(&started_for_body), Arc::clone(&gate_for_body));
            async move {
                started.fetch_add(1, Ordering::SeqCst);
                let _permit = gate.acquire().await.expect("gate stays open");
                succeeded()
            }
        });
        assert_eq!(session.running_count(), 2, "both lanes are taken");
        assert_eq!(session.queued_count(), 0);
        // Both handlers are parked inside their bodies; neither has finished,
        // which is exactly what the old serial read loop could not do.
        for _ in 0..500 {
            if started.load(Ordering::SeqCst) == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(started.load(Ordering::SeqCst), 2, "both lanes must be used");
        assert!(
            session.poll_completions().is_empty(),
            "a parked handler must not be reported as finished"
        );

        // Release both and drain.
        gate.add_permits(2);
        let completed = drain(&mut session, 2).await;
        assert_eq!(completed.len(), 2, "both handlers must report a terminal");
        assert_eq!(session.running_count(), 0, "every lane must be released");
    }

    #[tokio::test]
    async fn ordered_requests_run_one_at_a_time_in_arrival_order() {
        let mut session = IpcSession::with_limits(4, 8);
        for tail in ['V', 'W', 'X'] {
            assert_eq!(session.admit(command_request_named(tail)), Admission::Accepted);
        }
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = {
            let order = Arc::clone(&order);
            move || {
                let order = Arc::clone(&order);
                async move {
                    order.lock().expect("order").push("run");
                    succeeded()
                }
            }
        };
        spawn_ready(&mut session, record.clone());
        assert_eq!(session.running_count(), 1, "only one ordered request may run");
        assert_eq!(session.queued_count(), 2, "the rest wait in arrival order");
        for _ in 0..3 {
            let completed = drain(&mut session, 1).await;
            assert_eq!(completed.len(), 1);
            spawn_ready(&mut session, record.clone());
        }
        assert_eq!(order.lock().expect("order").len(), 3, "every ordered request ran");
        assert_eq!(session.running_count(), 0);
        assert_eq!(session.queued_count(), 0);
    }

    #[tokio::test]
    async fn a_read_only_request_is_not_blocked_by_a_running_command() {
        let mut session = IpcSession::with_limits(2, 4);
        assert_eq!(session.admit(command_request_named('V')), Admission::Accepted);
        assert_eq!(session.admit(read_only_request_named('W')), Admission::Accepted);
        spawn_ready(&mut session, || async {
            std::future::pending::<()>().await;
            succeeded()
        });
        assert_eq!(
            session.running_count(),
            2,
            "the ordered lane must not hold back an independent read"
        );
        session.shutdown().await;
        assert_eq!(session.running_count(), 0);
    }

    #[tokio::test]
    async fn a_queued_request_waits_for_a_lane_instead_of_being_dropped() {
        let mut session = IpcSession::with_limits(1, 2);
        for _ in 0..2 {
            assert_eq!(session.admit(read_only_request()), Admission::Accepted);
        }
        let started = Arc::new(AtomicUsize::new(0));
        let started_for_body = Arc::clone(&started);
        spawn_ready(&mut session, move || {
            let started = Arc::clone(&started_for_body);
            async move {
                started.fetch_add(1, Ordering::SeqCst);
                succeeded()
            }
        });
        assert_eq!(session.queued_count(), 1, "the second request must still be waiting");
        let completed = drain(&mut session, 1).await;
        assert_eq!(completed.len(), 1);
        assert_eq!(session.running_count(), 0);
        let started_for_body = Arc::clone(&started);
        spawn_ready(&mut session, move || {
            let started = Arc::clone(&started_for_body);
            async move {
                started.fetch_add(1, Ordering::SeqCst);
                succeeded()
            }
        });
        let completed = drain(&mut session, 1).await;
        assert_eq!(completed.len(), 1, "the waiting request runs once a lane frees up");
        assert_eq!(started.load(Ordering::SeqCst), 2, "both requests ran, in order");
        assert_eq!(session.running_count(), 0);
        assert_eq!(session.queued_count(), 0);
    }

    #[tokio::test]
    async fn requests_that_reuse_a_correlation_id_get_separate_lanes() {
        let mut session = IpcSession::with_limits(2, 2);
        // The same correlation id twice: the ledger assigns ids per connection,
        // so a repeat must not let one request take over another's lane and
        // silently swallow its reply.
        for _ in 0..2 {
            assert_eq!(session.admit(read_only_request()), Admission::Accepted);
        }
        spawn_ready(&mut session, || async { succeeded() });
        assert_eq!(session.running_count(), 2, "a repeated id must not collapse a lane");
        let completed = drain(&mut session, 2).await;
        assert_eq!(completed.len(), 2, "both replies must survive");
        assert_eq!(completed[0].0, completed[1].0, "both report the id they were sent with");
        assert_eq!(session.running_count(), 0);
    }

    #[tokio::test]
    async fn the_queue_is_bounded_and_reports_overload() {
        let mut session = IpcSession::with_limits(1, 2);
        assert_eq!(session.admit(read_only_request()), Admission::Accepted);
        assert_eq!(session.admit(read_only_request()), Admission::Accepted);
        assert_eq!(
            session.admit(read_only_request()),
            Admission::Overloaded { lane: Lane::Concurrent },
            "a third request must be refused instead of blocking the reader"
        );
        let error = Admission::overloaded_error(CorrelationId::new(), Lane::Concurrent);
        assert_eq!(error.category, ErrorCategory::Unavailable);
        assert!(error.retryable);
        assert_eq!(error.code.as_str(), "ipc.handler_overloaded");
    }

    #[tokio::test]
    async fn the_ordered_lane_is_bounded_too() {
        let mut session = IpcSession::with_limits(1, 2);
        for _ in 0..(1 + MAX_QUEUED_SERIAL) {
            assert_eq!(session.admit(command_request_named('V')), Admission::Accepted);
        }
        assert_eq!(
            session.admit(command_request_named('W')),
            Admission::Overloaded { lane: Lane::Serial },
            "an ordered burst is bounded as well"
        );
        let error = Admission::overloaded_error(CorrelationId::new(), Lane::Serial);
        assert!(error.message.contains("ordered"));
    }

    #[tokio::test]
    async fn a_panicking_handler_is_reported_not_swallowed() {
        let mut session = IpcSession::with_limits(1, 2);
        assert_eq!(session.admit(read_only_request()), Admission::Accepted);
        spawn_ready(&mut session, || async move { panic!("handler panic probe") });
        let completions = drain(&mut session, 1).await;
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].1, HandlerTerminal::Panicked);
        assert_eq!(session.running_count(), 0, "a panicked lane must be released");
    }

    #[tokio::test]
    async fn shutdown_drops_queued_work_and_aborts_running_handlers() {
        let mut session = IpcSession::with_limits(1, 2);
        let finished = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            assert_eq!(session.admit(read_only_request()), Admission::Accepted);
        }
        let finished_for_body = Arc::clone(&finished);
        spawn_ready(&mut session, move || {
            let finished = Arc::clone(&finished_for_body);
            async move {
                // Long enough that only an explicit abort can stop it.
                std::future::pending::<()>().await;
                finished.fetch_add(1, Ordering::SeqCst);
                succeeded()
            }
        });
        assert_eq!(session.running_count(), 1);
        assert_eq!(session.queued_count(), 1, "the second request is still waiting");

        session.shutdown().await;

        assert_eq!(session.running_count(), 0, "the running handler is gone");
        assert_eq!(session.queued_count(), 0, "queued work must be dropped");
        assert!(
            session.take_ready().is_empty(),
            "shutdown must never promote dropped work into a task"
        );
        assert_eq!(finished.load(Ordering::SeqCst), 0, "no aborted handler ran to completion");
        assert!(!session.is_busy());
    }

    fn session_write_named(method: &str, session_id: &str, tail: char) -> JsonRpcRequest {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": format!("cor_01ARZ3NDEKTSV4RRFFQ69G5FA{tail}"),
            "method": method,
            "params": { "session_id": session_id },
        }))
        .expect("request")
    }

    /// Only the authorised `SessionId`-bound write entries move off the
    /// connection's ordered lane, and each lands on its own session's key.
    #[test]
    fn only_session_bound_writes_leave_the_ordered_lane() {
        for method in SESSION_BOUND_WRITE_ROUTES {
            let request = session_write_named(method, "ses_A", 'V');
            assert_eq!(
                lane_key_for(&request),
                LaneKey::Session(SessionLaneKey::of("ses_A")),
                "{method} is SessionId-bound and must run on its own session's lane"
            );
        }
        // Connection-level and global configuration keep the original order.
        for method in [
            "session.create",
            "session.rename",
            "permission.set_defaults",
            "security.host_preparation.prepare",
            "artifact.export",
            "agent.tools.configure",
        ] {
            let request = session_write_named(method, "ses_A", 'W');
            assert_eq!(
                lane_key_for(&request),
                LaneKey::Connection(Lane::Serial),
                "{method} is not a per-session operation and must keep the ordered lane"
            );
        }
        for method in ["projection.subscribe", "projection.unsubscribe"] {
            let request = session_write_named(method, "ses_A", 'X');
            assert_eq!(
                lane_key_for(&request),
                LaneKey::Connection(Lane::Serial),
                "{method} orders this connection's own state"
            );
        }
        // A session-bound route with no usable session id is not guessed onto a session
        // lane: it falls through to the ordered lane, which can only be stricter.
        for params in [
            serde_json::json!({}),
            serde_json::json!({ "session_id": "" }),
            serde_json::json!({ "session_id": 7 }),
        ] {
            let request: JsonRpcRequest = serde_json::from_value(serde_json::json!({
                "jsonrpc": "2.0",
                "id": "cor_01ARZ3NDEKTSV4RRFFQ69G5FAA",
                "method": "agent.submit",
                "params": params,
            }))
            .expect("request");
            assert_eq!(
                lane_key_for(&request),
                LaneKey::Connection(Lane::Serial),
                "without a session id the request keeps the ordered lane"
            );
        }
        // Read-only session-scoped routes stay on the concurrent lane: a read cannot
        // observe another read's ordering.
        let read = session_write_named("agent.inbox", "ses_A", 'Y');
        assert_eq!(lane_key_for(&read), LaneKey::Connection(Lane::Concurrent));
    }

    /// The exact counterexample at the lane: A's session-bound request is
    /// running and unfinished, and B's ordinary writes go through the ordinary entries
    /// (never `agent.runs.start`). B must not queue behind A.
    ///
    /// A's handler really stays in flight - it is parked on a future only this test can
    /// complete - so "A is waiting" is a lane state rather than a timing assumption, and
    /// A's later requests are genuinely excluded while B's proceed. Nothing here is timed:
    /// every value is a count the lane itself reports.
    #[tokio::test]
    async fn a_waiting_session_does_not_hold_up_another_sessions_requests() {
        let mut session = IpcSession::with_limits(2, 4);
        let a_submit = session_write_named("agent.submit", "ses_A", 'A');
        let a_inbox = session_write_named("agent.inbox.mutate", "ses_A", 'B');
        let a_mode = session_write_named("permission.set_session_mode", "ses_A", 'C');
        let b_submit = session_write_named("agent.submit", "ses_B", 'D');
        let b_inbox = session_write_named("agent.inbox.mutate", "ses_B", 'E');
        let b_mode = session_write_named("permission.set_session_mode", "ses_B", 'F');

        for request in [&a_submit, &b_submit, &a_inbox, &b_inbox, &a_mode, &b_mode] {
            assert_eq!(session.admit(request.clone()), Admission::Accepted);
        }

        // A's first request takes A's key, B's takes B's: one lane per session.
        let first = session.take_ready();
        assert_eq!(first.len(), 2, "A and B each get their session's slot");
        let lanes: Vec<Lane> = first.iter().map(AdmittedRequest::lane).collect();
        assert_eq!(lanes, vec![Lane::Session, Lane::Session]);
        let first_tickets: Vec<u64> = first.iter().map(|admitted| admitted.ticket).collect();
        let mut aborts = Vec::new();
        for admitted in first {
            let (ticket, request) = admitted.into_parts();
            // A's handler stays in flight; B's finishes when it is polled.
            let handle = if request.id == a_submit.id {
                tokio::spawn(std::future::pending::<HandlerTerminal>())
            } else {
                tokio::spawn(async { succeeded() })
            };
            aborts.push((request.id, handle.abort_handle()));
            session.attach(ticket, handle);
        }
        assert_eq!(session.running_count(), 2);
        assert_eq!(session.queued_count(), 4, "the later requests wait, they are not refused");
        assert!(session.running_request_ids().contains(&a_submit.id));
        assert!(session.running_request_ids().contains(&b_submit.id));

        // B's first request completes. A's has not, and cannot.
        let finished = drain(&mut session, 1).await;
        assert!(
            finished.iter().any(|(id, _)| *id == b_submit.id),
            "B's own request completed while A is unfinished: {finished:?}"
        );
        assert!(
            !finished.iter().any(|(id, _)| *id == a_submit.id),
            "A's request is still in flight: {finished:?}"
        );
        // Only B's key is free, and only B's next request - in B's own order - may start.
        let next = session.take_ready();
        assert_eq!(next.len(), 1, "one free key, one request: {next:?}");
        let (ticket, request) = next.into_iter().next().expect("one request").into_parts();
        assert_eq!(request.id, b_inbox.id, "B's requests keep their own order");
        assert!(
            !first_tickets.contains(&ticket),
            "a request that was still queued may not reuse a ticket that is already running"
        );
        session.attach(ticket, tokio::spawn(async { succeeded() }));
        assert!(
            session.queued_count() == 3 && session.running_count() == 2,
            "A's three requests are still queued behind the one in flight"
        );

        // B finishes its requests in its own order while A is held; A's queue does not
        // move. Every expectation is a count the lane itself reports, taken after the
        // previous handler has really ended.
        let finished = drain(&mut session, 1).await;
        assert!(finished.iter().any(|(id, _)| *id == b_inbox.id), "{finished:?}");
        let next = session.take_ready();
        assert_eq!(next.len(), 1, "B's last request: {next:?}");
        let (ticket, request) = next.into_iter().next().expect("one request").into_parts();
        assert_eq!(request.id, b_mode.id, "B's requests keep their own order");
        session.attach(ticket, tokio::spawn(async { succeeded() }));
        let finished = drain(&mut session, 1).await;
        assert!(finished.iter().any(|(id, _)| *id == b_mode.id), "{finished:?}");
        assert!(
            session.take_ready().is_empty(),
            "the held session's queued requests stay excluded"
        );
        assert_eq!(session.queued_count(), 2, "A's remaining two requests are still excluded");

        // A is released: its requests then run in its own arrival order, one at a time.
        // Abort is enough - the point here is that the *key* is released when the handler
        // ends, however it ends.
        let a_abort = aborts
            .iter()
            .find(|(id, _)| *id == a_submit.id)
            .map(|(_, abort)| abort.clone())
            .expect("A's abort handle");
        a_abort.abort();
        let finished = drain(&mut session, 1).await;
        assert!(
            finished.iter().any(|(id, _)| *id == a_submit.id),
            "A's first request ended: {finished:?}"
        );
        assert_eq!(finished[0].1, HandlerTerminal::Panicked, "an aborted handler is reported");
        let order = [a_inbox.id, a_mode.id];
        for expected in order {
            let next = session.take_ready();
            assert_eq!(next.len(), 1, "A's requests run one at a time");
            let (ticket, request) = next.into_iter().next().expect("one request").into_parts();
            assert_eq!(request.id, expected, "A's own order is preserved");
            session.attach(ticket, tokio::spawn(async { succeeded() }));
            drain(&mut session, 1).await;
        }
        assert!(!session.is_busy(), "every admitted request is finished");
    }

    /// Same session, same order, even when a *later* request of that session could be
    /// taken while an earlier one is still waiting: the lane never reorders its own.
    #[tokio::test]
    async fn a_sessions_own_requests_never_overtake_each_other() {
        let mut session = IpcSession::with_limits(2, 4);
        let first = session_write_named("agent.submit", "ses_A", 'A');
        let second = session_write_named("agent.inbox.mutate", "ses_A", 'B');
        assert_eq!(session.admit(first.clone()), Admission::Accepted);
        assert_eq!(session.admit(second.clone()), Admission::Accepted);
        let ready = session.take_ready();
        assert_eq!(ready.len(), 1, "one key, one handler");
        let (ticket, request) = ready.into_iter().next().expect("one request").into_parts();
        assert_eq!(request.id, first.id, "the first request runs first");
        session.attach(ticket, tokio::spawn(async { succeeded() }));
        assert!(session.take_ready().is_empty(), "the second cannot overtake the first");
        drain(&mut session, 1).await;
        let ready = session.take_ready();
        assert_eq!(ready.len(), 1);
        let (ticket, request) = ready.into_iter().next().expect("one request").into_parts();
        assert_eq!(request.id, second.id, "then the second, in arrival order");
        session.attach(ticket, tokio::spawn(async { succeeded() }));
        drain(&mut session, 1).await;
        assert!(!session.is_busy());
    }

    /// The connection-level bound still applies to the session lane: a burst of
    /// session-bound writes is refused rather than queued without limit.
    #[tokio::test]
    async fn the_session_lane_keeps_its_connection_level_bound() {
        let mut session = IpcSession::with_limits(1, 2);
        for index in 0..(MAX_CONCURRENT_SESSION_WRITES + MAX_QUEUED_SESSION_WRITES) {
            let request = session_write_named("agent.submit", &format!("ses_{index}"), 'V');
            assert_eq!(
                session.admit(request),
                Admission::Accepted,
                "session {index} is inside the connection's bound"
            );
        }
        assert_eq!(
            session.admit(session_write_named("agent.submit", "ses_overflow", 'W')),
            Admission::Overloaded { lane: Lane::Session },
            "one request past the bound, without starting a new session's lane"
        );
        let error = Admission::overloaded_error(CorrelationId::new(), Lane::Session);
        assert!(error.message.contains("session"), "the refusal names its lane: {}", error.message);
    }

    /// A teardown drops the session lane's queue too, and a refused request never took
    /// a key: the lane is released, not leaked.
    #[tokio::test]
    async fn a_session_lane_is_released_after_shutdown_and_after_an_overload() {
        let mut session = IpcSession::with_limits(1, 2);
        assert_eq!(
            session.admit(session_write_named("agent.submit", "ses_A", 'A')),
            Admission::Accepted
        );
        let running = session.take_ready();
        assert_eq!(running.len(), 1);
        for admitted in running {
            let (ticket, _request) = admitted.into_parts();
            session.attach(ticket, tokio::spawn(std::future::pending::<HandlerTerminal>()));
        }
        assert_eq!(
            session.admit(session_write_named("agent.submit", "ses_B", 'B')),
            Admission::Accepted
        );
        let queued_before = session.queued_count();
        session.shutdown_within(std::time::Duration::from_millis(200)).await;
        assert_eq!(session.queued_count(), 0, "the queued B request was dropped, not promoted");
        assert_eq!(session.running_count(), 0);
        assert_eq!(queued_before, 1);
    }

    /// Request-id suffixes that are valid Crockford base32 and avoid the letters the existing
    /// helpers already use, so no two requests in one test share an id.
    const CROCKFORD_TAILS: [char; 24] = [
        'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'M', 'N', 'P', 'Q', 'R', 'S', 'T', 'V', 'W',
        'X', 'Y', 'Z', '1', '2', '3',
    ];

    fn request_id_named(tail: char) -> String {
        format!("cor_01ARZ3NDEKTSV4RRFFQ69G5FA{tail}")
    }

    /// The workspace/session an ordinary file request names.
    ///
    /// These are the *ownership* facts the real DTOs carry. They take no part in the resource
    /// identity, which is why two requests naming different workspaces but the same declared
    /// allocation still share a lane and two naming the same workspace but different files do not.
    const WORKSPACE: &str = "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAA";
    const SESSION: &str = "ses_01ARZ3NDEKTSV4RRFFQ69G5FAB";

    /// A real `fs.write_file` request, built from the **real DTO** and then serialized.
    ///
    /// The previous tests used `{ "target": "notes/one.txt" }`, which is not what the route
    /// declares, so every one of them measured a request that the handler would reject. This
    /// builds the typed value the route really accepts and serializes it, so the classification is
    /// exercised against the same bytes a real caller sends.
    fn write_file_named(path: &str, tail: char) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::FS_WRITE_FILE,
            &FsWriteFileRequest {
                workspace_id: workspace_id(),
                session_id: session_id(),
                target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                content: "written".to_owned(),
                expected: FsExpectedState::Absent,
                encoding: None,
                line_ending: None,
            },
            tail,
        )
    }

    fn apply_patch_named(path: &str, tail: char) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::FS_APPLY_PATCH,
            &FsApplyPatchRequest {
                workspace_id: workspace_id(),
                session_id: session_id(),
                target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                unified_diff: String::new(),
                expected_hash: ContentHash::blake3(b"old"),
            },
            tail,
        )
    }

    fn delete_named(path: &str, recursive: bool, tail: char) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::FS_DELETE,
            &FsDeleteRequest {
                workspace_id: workspace_id(),
                session_id: session_id(),
                target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                expected: FsExpectedState::Absent,
                recursive,
            },
            tail,
        )
    }

    fn create_directory_named(path: &str, tail: char) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::FS_CREATE_DIRECTORY,
            &FsCreateDirectoryRequest {
                workspace_id: workspace_id(),
                session_id: session_id(),
                target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
            },
            tail,
        )
    }

    fn move_named(source: &str, destination: &str, tail: char) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::FS_MOVE,
            &FsMoveRequest {
                workspace_id: workspace_id(),
                session_id: session_id(),
                source: WorkspaceFileTarget { path: WorkspacePath::parse(source).expect("path") },
                destination: WorkspaceFileTarget {
                    path: WorkspacePath::parse(destination).expect("path"),
                },
                source_expected: FsExpectedState::Absent,
                destination_expected: FsExpectedState::Absent,
            },
            tail,
        )
    }

    fn copy_named(source: &str, destination: &str, tail: char) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::FS_COPY,
            &FsCopyRequest {
                workspace_id: workspace_id(),
                session_id: session_id(),
                source: WorkspaceFileTarget { path: WorkspacePath::parse(source).expect("path") },
                destination: WorkspaceFileTarget {
                    path: WorkspacePath::parse(destination).expect("path"),
                },
                source_expected: FsExpectedState::Absent,
                destination_expected: FsExpectedState::Absent,
            },
            tail,
        )
    }

    fn changeset_named(operations: Vec<FsChangesetOperation>, tail: char) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::FS_APPLY_CHANGESET,
            &FsApplyChangesetRequest {
                workspace_id: workspace_id(),
                session_id: session_id(),
                operations,
            },
            tail,
        )
    }

    /// A real `workspace.buffer.save`, on the **registered** route constant.
    ///
    /// The previous helper used the literal `"buffer.save"`, which is not in the registry at all,
    /// so the one entry that matched was the one route that does not exist. This uses
    /// `aura_ipc::method::BUFFER_SAVE`, which is `workspace.buffer.save`.
    fn buffer_save_named(path: &str, tail: char) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::BUFFER_SAVE,
            &BufferSaveCommand {
                workspace_id: workspace_id(),
                session_id: session_id(),
                path: WorkspacePath::parse(path).expect("path"),
                reservation_id: EditorReservationId::new(),
                reservation_epoch: Revision::INITIAL,
                expected_buffer_revision: Revision::INITIAL,
            },
            tail,
        )
    }

    fn workspace_id() -> WorkspaceId {
        WORKSPACE.parse().expect("workspace id")
    }

    fn session_id() -> SessionId {
        SESSION.parse().expect("session id")
    }

    fn typed_request_named<T: serde::Serialize>(
        method: &str,
        params: &T,
        tail: char,
    ) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: request_id_named(tail).parse().expect("correlation id"),
            command_invocation_id: None,
            method: method.to_owned(),
            params: serde_json::to_value(params).expect("typed params"),
        }
    }

    /// Every registered route constant this classification claims to cover, with a real request.
    fn real_file_requests() -> Vec<(&'static str, JsonRpcRequest)> {
        vec![
            (aura_ipc::method::FS_WRITE_FILE, write_file_named("notes/one.txt", 'A')),
            (aura_ipc::method::FS_APPLY_PATCH, apply_patch_named("notes/one.txt", 'B')),
            (aura_ipc::method::FS_DELETE, delete_named("notes/one.txt", false, 'C')),
            (aura_ipc::method::FS_CREATE_DIRECTORY, create_directory_named("notes/dir", 'D')),
            (aura_ipc::method::FS_MOVE, move_named("notes/one.txt", "notes/two.txt", 'E')),
            (aura_ipc::method::FS_COPY, copy_named("notes/one.txt", "notes/two.txt", 'F')),
            (aura_ipc::method::BUFFER_SAVE, buffer_save_named("notes/one.txt", 'G')),
        ]
    }

    /// **The A-R2 classification regression.**
    ///
    /// Every entry on this table is a route the registry really declares, and every request is
    /// built from that route's own request DTO. The measured lane must be the resource lane, and it
    /// must no longer be the connection's ordered lane.
    ///
    /// This is the criterion that rejects the previous implementation. That version read
    /// `params.get("target")?.as_str()?`, which is `None` for `WorkspaceFileTarget`, and named
    /// `buffer.save` instead of `workspace.buffer.save`; every one of these seven requests
    /// therefore landed on `Connection(Serial)` and the classification did nothing. A test built
    /// from strings and an unregistered route passed anyway, which is exactly why this one is
    /// built from the registry and the real DTOs.
    #[test]
    fn every_real_file_entry_leaves_the_ordered_lane_for_its_real_resource() {
        for (method, request) in real_file_requests() {
            assert!(
                aura_ipc::routes::core_route(method).is_some(),
                "{method} must be a registered route, or the classification is about nothing"
            );
            let key = lane_key_for(&request);
            assert_ne!(
                key,
                LaneKey::Connection(Lane::Serial),
                "{method} carries a real DTO with a determinate target and must not keep the \
                 ordered lane"
            );
            assert_eq!(key.lane(), Lane::Session, "{method} lands on the resource lane");
            let LaneKey::FileTarget(target) = &key else {
                panic!("{method} must land on a file-resource lane, got {key:?}");
            };
            assert!(
                !target.declared().is_empty(),
                "{method} must declare at least one real target"
            );
        }

        // The same DTOs on the connection's ordinary entries keep the original order.
        for method in [
            "permission.set_defaults",
            "security.host_preparation.prepare",
            "artifact.export",
            "security.external_grant.revoke",
        ] {
            let request = typed_request_named(method, &serde_json::json!({ "note": "x" }), 'Y');
            assert_eq!(
                lane_key_for(&request),
                LaneKey::Connection(Lane::Serial),
                "{method} is not a per-file operation and must keep the ordered lane"
            );
        }
        for method in ["projection.subscribe", "projection.unsubscribe"] {
            let request = typed_request_named(method, &serde_json::json!({}), 'Z');
            assert_eq!(lane_key_for(&request), LaneKey::Connection(Lane::Serial));
        }
    }

    /// An unregistered route never gets a resource lane, however it spells its parameters.
    ///
    /// This is the counterexample for the one entry that used to match: `buffer.save` is not in the
    /// registry, so a request to it must stay on the ordered lane and be rejected by dispatch. The
    /// previous implementation gave it a `FileTarget` lane, which is how a nonexistent route came
    /// to be the only evidence the classification worked.
    #[test]
    fn an_unregistered_route_never_gains_a_resource_lane() {
        let request =
            typed_request_named("buffer.save", &serde_json::json!({ "path": "note.txt" }), 'A');
        assert!(aura_ipc::routes::core_route("buffer.save").is_none());
        assert_eq!(
            lane_key_for(&request),
            LaneKey::Connection(Lane::Serial),
            "a nonexistent route must not be classified as a real file entry"
        );
    }

    /// An invalid or incomplete request keeps both its rejection and its ordered lane.
    ///
    /// Resource eligibility must never come from a string that merely happens to be present: the
    /// parameters have to deserialize as the route's real request type. Every shape below is
    /// rejected by the handler, so every one of them must stay on the ordered lane — including the
    /// string target the old implementation *did* accept.
    #[test]
    fn an_invalid_request_keeps_its_ordered_lane_and_gains_no_eligibility() {
        for (label, params) in [
            ("no parameters", serde_json::json!({})),
            ("a bare string target", serde_json::json!({ "target": "notes/one.txt" })),
            ("an empty object target", serde_json::json!({ "target": {} })),
            ("a numeric target", serde_json::json!({ "target": 7 })),
            (
                "a target with no workspace or session",
                serde_json::json!({ "target": { "path": "notes/one.txt" } }),
            ),
            (
                "a traversal path",
                serde_json::json!({
                    "workspace_id": WORKSPACE,
                    "session_id": SESSION,
                    "target": { "path": "../escape.txt" },
                    "content": "x",
                    "expected": { "expected_kind": "absent" },
                }),
            ),
        ] {
            for method in [
                aura_ipc::method::FS_WRITE_FILE,
                aura_ipc::method::FS_APPLY_PATCH,
                aura_ipc::method::FS_DELETE,
                aura_ipc::method::FS_CREATE_DIRECTORY,
                aura_ipc::method::FS_MOVE,
                aura_ipc::method::FS_COPY,
                aura_ipc::method::BUFFER_SAVE,
            ] {
                let request = typed_request_named(method, &params, 'B');
                assert_eq!(
                    lane_key_for(&request),
                    LaneKey::Connection(Lane::Serial),
                    "{method} with {label} must keep the ordered lane instead of gaining \
                     resource eligibility from a malformed parameter set"
                );
            }
        }
    }

    /// A move takes **both** ends. The destination-only version this replaces let another
    /// operation touch the source at the same time.
    #[test]
    fn a_move_covers_its_source_and_its_destination() {
        let moved = move_named("notes/source.txt", "notes/elsewhere.txt", 'A');
        let LaneKey::FileTarget(key) = lane_key_for(&moved) else {
            panic!("a move must take a resource lane");
        };
        let declared = key
            .declared()
            .iter()
            .map(|target| (target.path.to_string(), target.scope, target.access))
            .collect::<Vec<_>>();
        assert!(
            declared.iter().any(|(path, scope, access)| path == "notes/source.txt"
                && *scope == RawScope::Slot
                && *access == RawAccess::Write),
            "the move must hold its source's entry: {declared:?}"
        );
        assert!(
            declared.iter().any(|(path, scope, access)| path == "notes/source.txt"
                && *scope == RawScope::File
                && *access == RawAccess::Write),
            "the move must hold its source's object: {declared:?}"
        );
        assert!(
            declared.iter().filter(|(path, _, _)| path == "notes/elsewhere.txt").count() >= 2,
            "the move must hold its destination's entry and object: {declared:?}"
        );

        // A request that touches either end of the move conflicts with it; one that touches a
        // third file does not.
        for (side, request) in [
            ("source", write_file_named("notes/source.txt", 'B')),
            ("destination", write_file_named("notes/elsewhere.txt", 'C')),
        ] {
            let LaneKey::FileTarget(other) = lane_key_for(&request) else {
                panic!("{side} must take a resource lane");
            };
            assert!(
                key.inner.overlaps(&other.inner),
                "a write of the move's {side} must be ordered against the move"
            );
        }
        let unrelated = write_file_named("notes/untouched.txt", 'D');
        let LaneKey::FileTarget(other) = lane_key_for(&unrelated) else {
            panic!("an unrelated write must take a resource lane");
        };
        assert!(
            !key.inner.overlaps(&other.inner),
            "a third file must not be ordered against the move"
        );
    }

    /// A copy and a changeset take their **full** declared set, and an empty changeset takes none.
    #[test]
    fn a_copy_and_a_changeset_take_their_full_declared_set() {
        let copied = copy_named("notes/source.txt", "notes/destination.txt", 'A');
        let LaneKey::FileTarget(key) = lane_key_for(&copied) else {
            panic!("a copy must take a resource lane");
        };
        let paths = key.declared().iter().map(|target| target.path.to_string()).collect::<Vec<_>>();
        assert!(paths.contains(&"notes/source.txt".to_owned()), "{paths:?}");
        assert!(paths.contains(&"notes/destination.txt".to_owned()), "{paths:?}");
        assert!(
            key.declared().iter().any(|target| target.path.to_string() == "notes/source.txt"
                && target.access == RawAccess::Read),
            "the copy's source is read, so it must not block another reader"
        );

        let changeset = changeset_named(
            vec![
                FsChangesetOperation::CreateDirectory {
                    target: WorkspaceFileTarget {
                        path: WorkspacePath::parse("notes/new").expect("path"),
                    },
                },
                FsChangesetOperation::Delete {
                    target: WorkspaceFileTarget {
                        path: WorkspacePath::parse("notes/tree").expect("path"),
                    },
                    expected: FsExpectedState::Absent,
                    recursive: true,
                },
            ],
            'B',
        );
        let LaneKey::FileTarget(key) = lane_key_for(&changeset) else {
            panic!("a changeset with real targets must take a resource lane");
        };
        let declared = key.declared().to_vec();
        assert!(
            declared
                .iter()
                .any(|target| target.path.to_string() == "notes/new"
                    && target.scope == RawScope::Slot),
            "a created directory is a slot: {declared:?}"
        );
        assert!(
            declared.iter().any(|target| target.path.to_string() == "notes/tree"
                && target.scope == RawScope::Subtree),
            "a recursive delete really reaches a subtree: {declared:?}"
        );

        // An empty changeset declares no resource at all, and takes no lane.
        let empty = changeset_named(vec![], 'C');
        assert_eq!(
            lane_key_for(&empty),
            LaneKey::Connection(Lane::Serial),
            "an empty changeset has no declared resource to coordinate"
        );

        // A recursive delete really conflicts with a write inside the directory it removes,
        // while a non-recursive one only names the entry.
        let recursive = delete_named("notes/tree", true, 'D');
        let inside = write_file_named("notes/tree/file.txt", 'E');
        let LaneKey::FileTarget(recursive_key) = lane_key_for(&recursive) else {
            panic!("a recursive delete must take a resource lane");
        };
        let LaneKey::FileTarget(inside_key) = lane_key_for(&inside) else {
            panic!("a write must take a resource lane");
        };
        assert!(
            recursive_key.inner.overlaps(&inside_key.inner),
            "deleting a directory must be ordered against a write inside it"
        );
        let sibling = write_file_named("notes/tree-sibling/file.txt", 'F');
        let LaneKey::FileTarget(sibling_key) = lane_key_for(&sibling) else {
            panic!("a write must take a resource lane");
        };
        assert!(
            !recursive_key.inner.overlaps(&sibling_key.inner),
            "an unrelated sibling directory must stay parallel"
        );
    }

    /// The declared resource is compared the way the Windows namespace it protects compares names.
    #[test]
    fn declared_lanes_do_not_guess_directory_name_rules_before_authorization() {
        let upper = write_file_named("Notes/One.txt", 'A');
        let lower = write_file_named("notes/one.txt", 'B');
        let LaneKey::FileTarget(upper_key) = lane_key_for(&upper) else {
            panic!("a write must take a resource lane");
        };
        let LaneKey::FileTarget(lower_key) = lane_key_for(&lower) else {
            panic!("a write must take a resource lane");
        };
        assert!(
            !upper_key.inner.overlaps(&lower_key.inner),
            "raw spelling does not prove an alias: the actual owner coordinates verified identities"
        );
        // A different leaf under the same parent is a different resource.
        let sibling = write_file_named("notes/two.txt", 'C');
        let LaneKey::FileTarget(sibling_key) = lane_key_for(&sibling) else {
            panic!("a write must take a resource lane");
        };
        assert!(!upper_key.inner.overlaps(&sibling_key.inner));
        // The same leaf under a different parent is a different resource.
        let elsewhere = write_file_named("elsewhere/one.txt", 'D');
        let LaneKey::FileTarget(elsewhere_key) = lane_key_for(&elsewhere) else {
            panic!("a write must take a resource lane");
        };
        assert!(
            !upper_key.inner.overlaps(&elsewhere_key.inner),
            "a same-named file in another directory is unrelated"
        );
    }

    /// Batch A's capacity proof: same-target waiters cannot consume the connection's capacity,
    /// and unrelated targets really run in parallel while the waiters are queued.
    ///
    /// The old behaviour put every file mutation on the connection's single ordered lane, so one
    /// slow write held the whole lane and a second file could not start. The measured facts here
    /// are the lane's own counts, not a sleep: one target's handler is parked unfinished, several
    /// more same-target requests are waiting behind it, and a *different* target is still taken by
    /// `take_ready` in the same round.
    #[tokio::test]
    async fn same_target_waiters_do_not_block_a_different_target() {
        let mut session = IpcSession::with_limits(1, 2);
        // One target holds a real write point, and several more wait on that same target.
        let held = write_file_named("contended.txt", 'A');
        assert_eq!(session.admit(held.clone()), Admission::Accepted);
        let waiting = (0..6)
            .map(|index| {
                // Crockford base32 excludes `I` and `U`, so the suffixes skip them.
                let tail = CROCKFORD_TAILS[index];
                write_file_named("contended.txt", tail)
            })
            .collect::<Vec<_>>();
        for request in &waiting {
            assert_eq!(
                session.admit(request.clone()),
                Admission::Accepted,
                "same-target waiters are admitted into the lane's bounded queue"
            );
        }
        // A different file, and the ordinary connection entries, submitted while all of that is
        // still outstanding.
        let different = write_file_named("unrelated.txt", 'H');
        assert_eq!(session.admit(different.clone()), Admission::Accepted);
        let heartbeat: JsonRpcRequest = serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "cor_01ARZ3NDEKTSV4RRFFQ69G5FAN",
            "method": "runtime.readiness",
            "params": {},
        }))
        .expect("request");
        assert_eq!(session.admit(heartbeat.clone()), Admission::Accepted);
        let stop = control_request_named('P');
        assert_eq!(
            session.admit(stop.clone()),
            Admission::Accepted,
            "a cancellation keeps its own lane while file targets are contended"
        );

        // The contended target's handler is started and parked: it genuinely holds its target.
        let ready = session.take_ready();
        let started = ready.iter().map(AdmittedRequest::request_id).collect::<Vec<_>>();
        assert!(started.contains(&held.id), "the contended target's first request starts");
        assert!(
            started.contains(&different.id),
            "the unrelated target must start in the same round as the contended one, not queue \
             behind it: started={started:?}"
        );
        assert!(
            started.contains(&heartbeat.id),
            "an ordinary read stays usable while a file target is held"
        );
        assert!(started.contains(&stop.id), "a stop stays usable while a file target is held");
        assert_eq!(
            started.iter().filter(|id| **id == held.id).count(),
            1,
            "only one request per target may run"
        );
        let mut parked = 0;
        for admitted in ready {
            let (ticket, request) = admitted.into_parts();
            let handle = if request.id == held.id {
                parked += 1;
                tokio::spawn(std::future::pending::<HandlerTerminal>())
            } else {
                tokio::spawn(async { succeeded() })
            };
            session.attach(ticket, handle);
        }
        assert_eq!(parked, 1, "the contended target is held by exactly one running handler");

        // With the contended target held, the same-target waiters stay excluded while the
        // connection remains responsive: the remaining rounds hand out no same-target work.
        let mut served_other = Vec::new();
        for _ in 0..8 {
            let round = session.take_ready();
            if round.is_empty() {
                break;
            }
            for admitted in round {
                let (ticket, request) = admitted.into_parts();
                assert_ne!(request.id, held.id, "the contended target must not start twice");
                served_other.push(request.id);
                session.attach(ticket, tokio::spawn(async { succeeded() }));
            }
        }
        assert!(
            waiting.iter().all(|request| !served_other.contains(&request.id)),
            "no same-target waiter may run while its target is held: served={served_other:?}"
        );
        // The waiters are still queued, not running and not consuming a slot each: the lane holds
        // one running handler per target, so the contended target occupies exactly one of the
        // lane's slots however many requests wait behind it.
        let held_key = lane_key_for(&held);
        let contended_running = session
            .lanes
            .iter()
            .flat_map(|lane| lane.running.values())
            .filter(|(key, _)| key.holds(&held_key))
            .count();
        assert_eq!(
            contended_running, 1,
            "the contended target must occupy exactly one running slot"
        );
        assert_eq!(
            session
                .lanes
                .iter()
                .flat_map(|lane| lane.queued.iter())
                .filter(|queued| queued.key.holds(&held_key))
                .count(),
            waiting.len(),
            "every same-target waiter is still queued behind the one running request"
        );
    }

    /// The shared default capacity is not exhausted by waiters on one target.
    ///
    /// The session lane's connection-level bound counts work in flight *and* waiting. Waiters on
    /// one target occupy that bound, so this pins the measured behaviour: a realistic number of
    /// same-target waiters leaves the lane usable for other targets, and only an unbounded burst
    /// is refused - locally, on this lane, without touching the other three.
    #[tokio::test]
    async fn same_target_waiters_leave_the_shared_capacity_usable() {
        let mut session = IpcSession::with_limits(1, 2);
        // Far fewer waiters than the connection's own bound for this lane.
        let held = buffer_save_named("shared.txt", 'A');
        assert_eq!(session.admit(held.clone()), Admission::Accepted);
        for (index, tail) in CROCKFORD_TAILS.iter().enumerate().take(8) {
            assert_eq!(
                session.admit(buffer_save_named("shared.txt", *tail)),
                Admission::Accepted,
                "waiter {index} is inside the lane's bound"
            );
        }
        // Other targets are still admitted, which is what "the four permits are not exhausted by
        // waiters" means at the lane: the waiters do not own the connection.
        for (index, tail) in CROCKFORD_TAILS.iter().enumerate().skip(8).take(4) {
            assert_eq!(
                session.admit(buffer_save_named(&format!("other-{index}.txt"), *tail)),
                Admission::Accepted,
                "an unrelated target is still admitted while one target has waiters"
            );
        }
        // Ordinary control and reads never share this lane's bound at all.
        assert_eq!(session.admit(control_request_named('Z')), Admission::Accepted);
        let heartbeat: JsonRpcRequest = serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "cor_01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "method": "runtime.readiness",
            "params": {},
        }))
        .expect("request");
        assert_eq!(session.admit(heartbeat), Admission::Accepted);
        assert_eq!(session.admit(command_request_named('W')), Admission::Accepted);
    }
    /// **The measured cross-workspace false conflict, at the lane.**
    ///
    /// A probe recorded `lane_keys_equal: true`, `first_ready: 1`,
    /// `second_ready_while_first_held: 0`, `queued: 1` for two **different** workspaces' `note.txt`:
    /// the raw target kept no workspace at all, so one lowercased relative path was the whole key and
    /// two unrelated files became one lane.
    ///
    /// Two workspaces that are really different trees must therefore never share a lane, and the
    /// first must not hold the second up.
    #[tokio::test]
    async fn two_unrelated_workspaces_same_relative_path_do_not_share_a_lane() {
        let mut session = IpcSession::new();
        let first = write_file_in("note.txt", 'A', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAA");
        let second = write_file_in("note.txt", 'B', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FC1");
        let first_key = lane_key_for(&first);
        let second_key = lane_key_for(&second);
        assert_ne!(
            first_key, second_key,
            "two unrelated workspaces holding a note.txt must not be one lane"
        );
        let (LaneKey::FileTarget(mine), LaneKey::FileTarget(theirs)) = (&first_key, &second_key)
        else {
            panic!("both must take a resource lane");
        };
        assert!(
            !mine.inner.overlaps(&theirs.inner),
            "the two lanes must really be independent, not merely unequal values"
        );

        // The held first target does not hold up the unrelated second one.
        assert_eq!(session.admit(first.clone()), Admission::Accepted);
        let ready = session.take_ready();
        assert_eq!(ready.len(), 1);
        for admitted in ready {
            let (ticket, _request) = admitted.into_parts();
            session.attach(ticket, tokio::spawn(std::future::pending::<HandlerTerminal>()));
        }
        assert_eq!(session.admit(second.clone()), Admission::Accepted);
        let ready = session.take_ready();
        assert_eq!(
            ready.len(),
            1,
            "the unrelated workspace's request must start while the first is held"
        );
        let (ticket, request) = ready.into_iter().next().expect("one").into_parts();
        assert_eq!(request.id, second.id);
        session.attach(ticket, tokio::spawn(async { succeeded() }));
    }

    /// The same relative path in **one** workspace still shares a lane, and the second waits.
    ///
    /// This is the other half: removing the false conflict must not have removed the real one.
    #[tokio::test]
    async fn one_workspace_same_target_still_waits_for_itself() {
        let mut session = IpcSession::new();
        let held = write_file_in("note.txt", 'A', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAA");
        let waiting = write_file_in("note.txt", 'B', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAA");
        assert_eq!(session.admit(held.clone()), Admission::Accepted);
        let ready = session.take_ready();
        for admitted in ready {
            let (ticket, _request) = admitted.into_parts();
            session.attach(ticket, tokio::spawn(std::future::pending::<HandlerTerminal>()));
        }
        assert_eq!(session.admit(waiting.clone()), Admission::Accepted);
        assert!(
            session.take_ready().is_empty(),
            "one workspace, one target: the second request must wait"
        );
        assert_eq!(session.queued_count(), 1);
    }

    /// An unrelated **different file** in one workspace stays parallel — the whole point of the
    /// change, and the case the old whole-workspace exclusion got wrong.
    #[test]
    fn unrelated_files_in_one_workspace_take_independent_lanes() {
        let first = write_file_in("one.txt", 'A', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAA");
        let second = write_file_in("two.txt", 'B', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAA");
        let LaneKey::FileTarget(mine) = lane_key_for(&first) else {
            panic!("a resource lane");
        };
        let LaneKey::FileTarget(theirs) = lane_key_for(&second) else {
            panic!("a resource lane");
        };
        assert!(!mine.inner.overlaps(&theirs.inner), "two files, two lanes");
        assert_eq!(
            mine.workspace().anchor,
            theirs.workspace().anchor,
            "both are anchored to the one workspace they really named"
        );
    }

    /// A request in a workspace that names no root is anchored by its id, and two different ids are
    /// two trees even when every path in them is spelled identically.
    #[test]
    fn a_workspace_without_a_known_root_is_anchored_by_its_id() {
        let first = write_file_in("deep/same.txt", 'A', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAA");
        let same = write_file_in("deep/same.txt", 'B', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FAA");
        let other = write_file_in("deep/same.txt", 'C', "wsp_01ARZ3NDEKTSV4RRFFQ69G5FC1");
        let LaneKey::FileTarget(mine) = lane_key_for(&first) else {
            panic!("a resource lane");
        };
        let LaneKey::FileTarget(same_key) = lane_key_for(&same) else {
            panic!("a resource lane");
        };
        let LaneKey::FileTarget(other_key) = lane_key_for(&other) else {
            panic!("a resource lane");
        };
        assert!(mine.inner.overlaps(&same_key.inner), "one workspace, one path: related");
        assert!(
            !mine.inner.overlaps(&other_key.inner),
            "a different workspace id is a different tree"
        );
        assert!(!mine.workspace().is_root, "no canonical root was available here");
    }

    /// A real `fs.write_file` in a named workspace, so a test can vary the workspace.
    fn write_file_in(path: &str, tail: char, workspace: &str) -> JsonRpcRequest {
        typed_request_named(
            aura_ipc::method::FS_WRITE_FILE,
            &FsWriteFileRequest {
                workspace_id: workspace.parse().expect("workspace id"),
                session_id: session_id(),
                target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                content: "written".to_owned(),
                expected: FsExpectedState::Absent,
                encoding: None,
                line_ending: None,
            },
            tail,
        )
    }
}
