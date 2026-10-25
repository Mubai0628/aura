//! The **one** resource identity every file entry is coordinated by.
//!
//! Batch A's confirmed decision is that ordinary file writes, moves, deletes and Buffer saves are
//! coordinated by their **real resource**, so two operations on the same physical file wait for
//! each other while operations on unrelated files run in parallel — inside one workspace and
//! across workspaces.
//!
//! # What a resource is
//!
//! A resource is the thing on disk, not the text a caller happened to spell and not the workspace
//! it happens to have been opened through:
//!
//! * an **object** is its real volume + file identity **plus the verified location it was reached
//!   at** — the identity of the object itself and the identity of every directory above it, from
//!   the verified anchor down. The identity alone says *which* object; the location says *where*
//!   it is, and only together can they answer the questions this batch has to answer (see below);
//! * a **slot** is the verified parent directory's identity plus the leaf name compared under the
//!   **real** case rule of the volume the parent lives on, so it names a directory entry that may
//!   not exist yet — which is what a create, a move destination and a delete-recreate have to
//!   coordinate on;
//! * a **directory scope** is the verified directory identity plus its ancestry chain, so
//!   renaming, deleting or replacing a directory conflicts with writes to files under it and
//!   still leaves unrelated siblings parallel.
//!
//! `WorkspaceId`, `SessionId` and the display path are **ownership only**. They are carried so a
//! caller can say who is asking and show a readable name, and they are deliberately excluded from
//! the identity comparison: including them is exactly how one physical file used to get two
//! writable claims in two workspaces, and how two unrelated workspaces with the same relative
//! file name used to be bound together.
//!
//! # Why the object carries a location
//!
//! "The same object" and "an object somewhere inside that directory" are different questions, and
//! an identity alone cannot answer the second. A file's parent directory is not reachable from the
//! file's `FileIdentity`, and Windows exposes no parent pointer for an identity that is already
//! known. So the caller that **did** open the object through a verified chain passes that chain
//! in, and containment is decided by identity equality along it.
//!
//! The chain is the identity of every directory strictly above the object, ordered from the
//! outermost verified directory inwards, plus [`ResourceLocation::anchor`] naming the outermost
//! directory it starts from. An anchor makes a chain *placeable*: it says which verified directory
//! the walk started at, so the chain can be read as "these directories, under this one".
//!
//! # Why an anchor is not a comparability gate
//!
//! The pre-fix rule was "two chains from different anchors are not comparable, so no containment is
//! ever claimed". That rule is **wrong about the world**, and it was measured wrong: a workspace
//! rooted at `/tree` and a workspace rooted at `/tree/nested` are two anchors, and a file written
//! through the second is genuinely inside a directory scope taken through the first. Under the
//! pre-fix rule the nested write was judged unrelated and really landed while the outer directory
//! scope was held.
//!
//! The distinction the rule was reaching for is real, but it belongs to one comparison and not to
//! the other:
//!
//! * **Equality of chains** needs the same anchor. Two ancestry lists with a shared suffix are not
//!   one chain, so `list_contains` refuses to read a suffix as containment.
//! * **Containment** does not. Both anchors are *themselves verified directory identities*, so an
//!   anchor can be located inside the other chain just like any other ancestor. That is what makes
//!   a nested workspace's chain placeable under a parent workspace's chain, and it is decided by
//!   identity — never by comparing the two anchors' display text.
//!
//! So [`ResourceLocation::is_at_or_under`] answers containment from identities alone and
//! [`ResourceLocation::same_chain`] answers "these two walks describe one place".
//!
//! # Why case is a rule and not a `to_lowercase`
//!
//! Windows compares directory entries through the object manager, whose rule is
//! case-insensitive-or-preserving for ordinary volumes. Lowercasing the leaf here would *assert*
//! that rule, and it would be wrong in two ways: it also folds characters the volume may genuinely
//! distinguish (a Turkish dotless `ı` against `I` under an invariant-locale comparison), and it
//! would leave the module claiming a rule it never checked. [`SlotCaseRule`] therefore names the
//! rule the resolver actually established, and the folding that follows from it is applied by
//! the held directory comparison table — never by an assumption inside a comparison. A volume whose rule is
//! [`SlotCaseRule::Unknown`] declares no folding, which can only make two slots *less* likely to
//! compare equal, and the owner treats that as a missing determinate fact rather than a conflict
//! that was decided.
//!
//! # What this module is not
//!
//! It owns *identity*, not the decision. [`conflict_between`] answers "do these two requests touch
//! the same thing", and the eligibility owner built on top of it decides who holds a long edit
//! claim and how a short commit is admitted. Nothing here opens a file, blocks, or performs I/O,
//! so it can be used both from a request lane that only has parameters and from a mutation that
//! already holds verified handles.
use aura_domain_common::{FileIdentity, SessionId, WorkspaceId, WorkspacePath};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The kind of access a caller needs on a resource.
///
/// This is the *access*, not the tool: two different routes that both intend to write one file
/// must produce the same requirement, and a read must never take a write claim.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAccess {
    /// Observing the resource. Never blocks another reader, and never creates a long claim.
    Read,
    /// Replacing or deleting exactly this object, or the directory entry that names it.
    Write,
    /// Replacing or deleting this directory and everything inside it.
    SubtreeWrite,
}

impl ResourceAccess {
    /// Whether this access creates a claim rather than a bare observation.
    ///
    /// Reads never do: the confirmed behaviour is that an ordinary read does not take a long write
    /// or observation lock and never establishes a write claim on behalf of someone else.
    #[must_use]
    pub const fn is_writing(self) -> bool {
        !matches!(self, Self::Read)
    }
}

/// How a path was reached, which is the only reason the workspace distinction is carried at all.
///
/// It is intentionally **not** part of the identity comparison. It exists so a caller can report
/// which workspace observed a resource and so cross-workspace association can be *shown*.
#[derive(Clone, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceOwner {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub path: WorkspacePath,
}

/// One resource a file operation touches.
///
/// The identity is what [`FileResource::identity_of`] returns; everything else is evidence and
/// ownership. Two resources are the same resource when their identities are equal — never because
/// their display paths look alike.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "resource_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileResource {
    /// A file or directory that exists and was verified: its real volume + file identity, and the
    /// verified location it was reached at.
    ///
    /// This is the case that must associate across workspaces. A hardlink, a second workspace
    /// rooted at the same directory, or a nested workspace all resolve to the same `identity`.
    ///
    /// `location` is what makes the object usable for a real containment question. It is
    /// deliberately **not** part of the identity: one object reached through two spellings is one
    /// object, and two different spellings that resolve to the same chain are the same place.
    Object { identity: FileIdentity, location: ResourceLocation, owner: ResourceOwner },
    /// One directory entry: verified parent identity, the verified location of that parent, and
    /// the leaf name under the volume's real case rule.
    ///
    /// This is a slot, not an object: it names a position that may not exist yet. The parent's
    /// location is carried for the same reason the object's is — so "this entry is inside the
    /// directory being replaced" is decided by verified ancestry, never by path text.
    Slot { parent: SlotParent, leaf_name: String, owner: ResourceOwner },
    /// One entry **below a directory that does not exist yet**.
    ///
    /// The pre-fix model had no way to say this, so `slot` collapsed every uncreated path onto its
    /// nearest existing ancestor: `missing-one/note.txt` and `missing-two/note.txt` both became
    /// `slot:<root>/note.txt` and were judged the *same* entry. Two unrelated targets then
    /// serialised against each other, and the directory each of them will really be created inside
    /// was not covered at all.
    ///
    /// So the pending components are carried, in order, from the nearest ancestor the resolver
    /// really opened down to the leaf. The identity is the parent plus this whole relative path, so
    /// two paths that diverge below the ancestor are two resources.
    ///
    /// It is deliberately **not** a promise that those directories exist, and it is not a slot that
    /// can be compared for equality with one — [`FileResource::identity_of`] keeps them apart — but
    /// it *is* placeable: the `containment_path` answers "which directory will this be created
    /// inside", which is what a directory scope has to cover.
    ProspectiveEntry {
        parent: SlotParent,
        /// The components from the verified parent down to the leaf, inclusive. Never empty: the
        /// last component is the leaf itself.
        pending: Vec<String>,
        owner: ResourceOwner,
    },
    /// A directory and everything inside it.
    ///
    /// `directory_identity` is the directory itself. `location` is the **parent** location of that
    /// directory and therefore must not repeat the directory, so
    /// `location.ancestry.last() == Some(&directory_identity)` is true by construction: the chain
    /// ends at the directory's immediate parent. That single fact is what makes the
    /// slot-is-this-directory-entry case decidable at all.
    Subtree { directory_identity: FileIdentity, location: ResourceLocation, owner: ResourceOwner },
}

/// Where a verified object sits, in identities rather than in text.
///
/// A chain always travels with the anchor it was walked from. The anchor is *not* a comparability
/// gate — it is a verified directory identity like any other ancestor, so it can be located inside
/// another chain (see [`ResourceLocation::is_at_or_under`]). What it does decide is chain
/// **equality**: two walks that started in different places are two places even when their
/// ancestry lists share a suffix.
#[derive(Clone, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLocation {
    /// The outermost verified directory the chain starts from.
    ///
    /// For a workspace-relative resolution that anchor is the verified workspace root. A second
    /// workspace rooted *inside* the first therefore has an anchor that appears in the first
    /// workspace's chain, and that is exactly how a nested workspace's write is recognised as being
    /// inside a directory scope the parent workspace holds. Two unrelated roots never appear in each
    /// other's chains, so their files stay parallel.
    pub anchor: FileIdentity,
    /// The identity of every directory strictly between the anchor and the object, from the
    /// anchor inwards, excluding the anchor itself and excluding the object itself.
    ///
    /// An object directly inside the anchor therefore has an empty chain, and the anchor is still
    /// its parent.
    pub ancestry: Vec<FileIdentity>,
}

impl ResourceLocation {
    /// A location at the anchor itself, which is also the location of everything directly inside.
    #[must_use]
    pub fn at_anchor(anchor: FileIdentity) -> Self {
        Self { anchor, ancestry: Vec::new() }
    }

    /// The full chain from the anchor inwards, anchor first.
    pub fn chain(&self) -> impl Iterator<Item = &FileIdentity> {
        std::iter::once(&self.anchor).chain(self.ancestry.iter())
    }

    /// The directory this location sits in: the last ancestor, or the anchor.
    #[must_use]
    pub fn parent(&self) -> &FileIdentity {
        self.ancestry.last().unwrap_or(&self.anchor)
    }

    /// Whether this location is `directory` itself or sits under it, decided by identity alone.
    ///
    /// Containment is *not* gated on two walks having the same anchor. Both anchors are verified
    /// directory identities, so either anchor may itself be an ancestor of the other walk — which is
    /// precisely the nested-workspace case this batch had to repair: a workspace rooted at
    /// `/tree/nested` walks from an anchor that appears inside the `/tree` workspace's chain, and a
    /// write through the nested workspace is therefore really inside a `/tree` directory scope.
    ///
    /// Two unrelated trees never contain one another's anchors or ancestors, so they stay parallel.
    /// This still guesses nothing from path text: every comparison is between identities the
    /// resolver opened.
    #[must_use]
    pub fn is_at_or_under(&self, directory: &FileIdentity) -> bool {
        self.chain().any(|node| node == directory)
    }

    /// Whether two walks describe the same place.
    ///
    /// Deliberately stricter than containment: two chains with a shared suffix are not one chain, so
    /// the anchors must be equal and the ancestries must match exactly.
    #[must_use]
    pub fn same_chain(&self, other: &Self) -> bool {
        self.anchor == other.anchor && self.ancestry == other.ancestry
    }

    /// A location one level deeper, for an object inside `directory`.
    ///
    /// The directory must be this location's own parent for the result to describe a real chain,
    /// which the caller established by resolving the child through the parent it verified.
    #[must_use]
    pub fn child_at(&self, directory: FileIdentity) -> Self {
        let mut ancestry = self.ancestry.clone();
        ancestry.push(directory);
        Self { anchor: self.anchor.clone(), ancestry }
    }
}

/// The verified parent of a directory entry.
#[derive(Clone, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlotParent {
    /// The parent directory's real identity.
    pub identity: FileIdentity,
    /// Where that parent directory sits, so the entry's containment is decidable without text.
    pub location: ResourceLocation,
    /// How the volume the parent lives on really compares leaf names.
    pub case_rule: SlotCaseRule,
    /// Held-directory comparison facts. Never serialized into an operation or permission input.
    #[serde(skip)]
    #[schemars(skip)]
    pub name_comparison: Option<aura_domain_common::FileNameComparison>,
}

/// How a volume compares two directory-entry names.
///
/// This is established by the resolver from the volume the parent was opened on — never asserted
/// here — and it is carried on the request so the comparison can apply the rule the resolver
/// proved rather than a rule this module invented.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotCaseRule {
    /// The volume folds case for ordinary directory lookups: `Notes.txt` and `notes.txt` are one
    /// entry. This is the rule of the NTFS/FAT volumes Aura publishes to.
    InsensitivePreserving,
    /// The volume distinguishes case, so the two spellings are two entries.
    Sensitive,
    /// The caller could not establish the rule. No folding is applied: the two spellings then
    /// compare as different, which is the conservative direction — it can only make the owner
    /// treat the situation as a missing fact rather than silently merge two entries.
    Unknown,
}

impl SlotCaseRule {
    /// Whether this rule lets the comparison decide.
    ///
    /// `Unknown` is not a rule that says "different"; it says the caller has not established the
    /// fact. A caller that needs a determinate answer must treat the two spellings as
    /// *unresolved* rather than as distinct entries.
    #[must_use]
    pub const fn is_determinate(self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

/// Case folding for the case-insensitive-preserving namespace rule.
///
/// Deliberately simple and total: the rule folds each character to its simple uppercase mapping,
/// which is what the object manager does for the ASCII range that ordinary source file names use.
/// It is a separate function so the folding is visible as *the rule being applied* rather than as
/// an incidental `to_lowercase()` inside a comparator, and so a volume with a different rule has
/// somewhere to say so.
impl SlotParent {
    /// Only an actual boundary's one-to-one table establishes an insensitive name key.
    #[must_use]
    pub fn comparison_key(&self, name: &str) -> Option<String> {
        match self.case_rule {
            SlotCaseRule::Sensitive => Some(name.to_owned()),
            SlotCaseRule::InsensitivePreserving => {
                self.name_comparison.as_ref().map(|map| map.key(name))
            }
            SlotCaseRule::Unknown => None,
        }
    }

    fn fold(&self, name: &str) -> String {
        self.comparison_key(name).unwrap_or_else(|| name.to_owned())
    }
}

impl FileResource {
    /// The canonical identity compared for conflict.
    ///
    /// Deliberately free of `WorkspaceId`, `SessionId` and display text: those are exactly the
    /// fields that must not be able to split one physical resource or bind two unrelated ones.
    #[must_use]
    pub fn identity_of(&self) -> ResourceIdentity {
        match self {
            Self::Object { identity, .. } => ResourceIdentity::Object(identity.clone()),
            Self::Slot { parent, leaf_name, .. } => ResourceIdentity::Slot {
                parent_identity: parent.identity.clone(),
                leaf_name: parent.fold(leaf_name),
            },
            Self::ProspectiveEntry { parent, pending, .. } => ResourceIdentity::ProspectiveEntry {
                parent_identity: parent.identity.clone(),
                // Every pending component but the leaf is a directory name this operation will
                // really create, and the leaf is the entry inside them. Folding the whole relative
                // path is what keeps two targets under two different uncreated parents apart.
                relative_path: pending
                    .iter()
                    .map(|component| parent.fold(component))
                    .collect::<Vec<_>>()
                    .join("/"),
            },
            Self::Subtree { directory_identity, .. } => {
                ResourceIdentity::Subtree(directory_identity.clone())
            }
        }
    }

    /// The verified location this resource was reached at, if the caller established one.
    ///
    /// A slot's location is its parent's location *extended by that parent*, because the entry
    /// sits inside the parent: that is what lets a slot be placed inside a directory scope without
    /// inventing a chain. A prospective entry sits inside the directories it will create, so its
    /// location is its parent's location extended by those components — including the intermediate
    /// ones, which is what makes a future intersecting directory covered.
    #[must_use]
    pub fn location_of(&self) -> Option<ResourceLocation> {
        match self {
            Self::Object { location, .. } => Some(location.clone()),
            Self::Slot { parent, .. } => Some(parent.location.child_at(parent.identity.clone())),
            Self::ProspectiveEntry { parent, pending, .. } => {
                // The components below the verified parent are names, not identities, so they
                // cannot be placed in an identity chain. What *can* be placed is the verified
                // parent itself, which is the nearest directory this entry will really be created
                // inside; `containment_path` carries the rest as names.
                let _ = pending;
                Some(parent.location.child_at(parent.identity.clone()))
            }
            Self::Subtree { location, .. } => Some(location.clone()),
        }
    }

    /// The directory a resource will really be created inside or replaced at, as a verified chain
    /// plus the names below it.
    ///
    /// `None` when the resource carries no verified parent at all.
    ///
    /// The chain is the **deepest verified directory this resource sits under**, and `below` are the
    /// verified names between that directory and the resource:
    ///
    /// * an object or a subtree reports the directory that really contains it, which is why its
    ///   `below` is empty;
    /// * an entry — a slot or a prospective entry — reports its verified parent, with its own name
    ///   (and any uncreated components after that parent) as `below`. The parent's identity is
    ///   deliberately *not* repeated in the chain, because it is already the anchor of the parent's
    ///   own location and repeating it would make an entry look like it sits *inside itself*.
    ///
    /// This is the fact a directory scope needs: a scope covering a directory must cover every
    /// resource whose verified chain reaches that directory. The uncreated components in `below`
    /// are names, not identities, so they cannot be placed in an identity chain — what they carry is
    /// the honest statement that the path continues below the deepest directory that exists.
    #[must_use]
    pub fn containment_path(&self) -> Option<ContainmentPath> {
        match self {
            Self::Object { location, .. } => {
                Some(ContainmentPath { chain: location.clone(), below: Vec::new() })
            }
            Self::Slot { parent, leaf_name, .. } => Some(ContainmentPath {
                chain: parent.location.clone(),
                below: vec![parent.fold(leaf_name)],
            }),
            Self::ProspectiveEntry { parent, pending, .. } => Some(ContainmentPath {
                chain: parent.location.clone(),
                below: pending.iter().map(|component| parent.fold(component)).collect(),
            }),
            Self::Subtree { location, directory_identity, .. } => Some(ContainmentPath {
                chain: location.child_at(directory_identity.clone()),
                below: Vec::new(),
            }),
        }
    }

    /// The directory identity this resource names as its own entry, when it names one.
    ///
    /// An object and a subtree both have a directory identity of their own: the object's own
    /// identity may be a directory, and the subtree's `directory_identity` is one by definition.
    /// A slot's or a prospective entry's parent is not "the entry", so it is not reported here.
    #[must_use]
    pub fn own_identity(&self) -> Option<&FileIdentity> {
        match self {
            Self::Object { identity, .. } => Some(identity),
            Self::Subtree { directory_identity, .. } => Some(directory_identity),
            Self::Slot { .. } | Self::ProspectiveEntry { .. } => None,
        }
    }

    /// The verified parent this entry will be created under, for the entry-shaped resources.
    #[must_use]
    pub fn entry_parent(&self) -> Option<&SlotParent> {
        match self {
            Self::Slot { parent, .. } | Self::ProspectiveEntry { parent, .. } => Some(parent),
            Self::Object { .. } | Self::Subtree { .. } => None,
        }
    }

    /// The ownership and display facts, which never take part in the comparison.
    #[must_use]
    pub fn owner(&self) -> &ResourceOwner {
        match self {
            Self::Object { owner, .. }
            | Self::Slot { owner, .. }
            | Self::ProspectiveEntry { owner, .. }
            | Self::Subtree { owner, .. } => owner,
        }
    }

    /// Whether two resources are the same real thing.
    #[must_use]
    pub fn same_resource(&self, other: &Self) -> bool {
        self.identity_of() == other.identity_of()
    }

    /// A stable, human-readable name for reporting. Never used for comparison.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Object { owner, .. } | Self::Subtree { owner, .. } => owner.path.to_string(),
            Self::Slot { leaf_name, owner, .. } => {
                let parent = owner.path.as_str().rsplit_once('/').map(|(parent, _)| parent);
                match parent {
                    Some(parent) if !parent.is_empty() => format!("{parent}/{leaf_name}"),
                    _ => leaf_name.clone(),
                }
            }
            // The prospective entry's display is the caller's own path, which is the only honest
            // name for a path whose components do not exist yet.
            Self::ProspectiveEntry { owner, .. } => owner.path.to_string(),
        }
    }

    /// A stable text key for ordering and for reporting a conflict.
    ///
    /// Derived from the identity, so it does not vary with the spelling of the display path and
    /// cannot be used to make two workspaces' same-named files collide. It is a *reporting* key:
    /// two resources that really conflict may still produce two different keys (a directory and a
    /// file inside it do), which is exactly why the decision is [`conflict_between`] and never
    /// string equality.
    #[must_use]
    pub fn lock_key(&self, access: ResourceAccess) -> String {
        let access = match access {
            ResourceAccess::Read => "read",
            ResourceAccess::Write => "write",
            ResourceAccess::SubtreeWrite => "subtree-write",
        };
        format!("{}:{access}", self.identity_text())
    }

    fn identity_text(&self) -> String {
        match self.identity_of() {
            ResourceIdentity::Object(identity) => format!("object:{}", identity_text(&identity)),
            ResourceIdentity::Slot { parent_identity, leaf_name } => {
                format!("slot:{}/{leaf_name}", identity_text(&parent_identity))
            }
            ResourceIdentity::ProspectiveEntry { parent_identity, relative_path } => {
                format!("pending:{}/{relative_path}", identity_text(&parent_identity))
            }
            ResourceIdentity::Subtree(directory_identity) => {
                format!("subtree:{}", identity_text(&directory_identity))
            }
        }
    }
}

/// Where an entry will really be created, expressed as a verified chain plus uncreated names.
///
/// The chain is the deepest directory the resolver verified above the resource; `below` are the
/// components under it that the resource names — its own leaf, and any components that do not exist
/// yet. The chain never repeats the entry's own parent, so a scope that is that parent is found by
/// walking the chain rather than by matching a duplicated node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainmentPath {
    pub chain: ResourceLocation,
    pub below: Vec<String>,
}

impl ContainmentPath {
    /// Whether this path will be created at or under `directory`.
    #[must_use]
    pub fn is_at_or_under(&self, directory: &FileIdentity) -> bool {
        self.chain.is_at_or_under(directory)
    }
}

fn identity_text(identity: &FileIdentity) -> String {
    format!("{}:{}", identity.volume_serial_number, identity.file_id)
}

/// The comparison form of a [`FileResource`].
///
/// Split out so the identity can be computed once and compared many times without cloning the
/// ownership facts, and so a reader can see plainly what does and does not take part.
///
/// The location is deliberately **absent** here. Identity answers "the same object"; location
/// answers "inside which directory", and the two questions are asked separately by
/// [`conflict_between`] so that folding a location into the equality test cannot make one object
/// reached through two spellings look like two objects.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ResourceIdentity {
    Object(FileIdentity),
    Slot {
        parent_identity: FileIdentity,
        leaf_name: String,
    },
    /// An entry below a directory that does not exist yet. The relative path is the whole pending
    /// tail, folded under the parent's case rule, so two targets under two different uncreated
    /// parents can never collapse into one identity.
    ProspectiveEntry {
        parent_identity: FileIdentity,
        relative_path: String,
    },
    Subtree(FileIdentity),
}

/// One resource and the access a single request needs on it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRequest {
    pub resource: FileResource,
    pub access: ResourceAccess,
}

impl ResourceRequest {
    #[must_use]
    pub fn new(resource: FileResource, access: ResourceAccess) -> Self {
        Self { resource, access }
    }

    #[must_use]
    pub fn lock_key(&self) -> String {
        self.resource.lock_key(self.access)
    }
}

/// A real, observed conflict between two requests.
///
/// It names both sides so the refusal is explainable, and carries no timing claim: nothing here
/// says how long anything waits.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceConflict {
    pub requested: String,
    pub held: String,
    pub reason: ConflictReason,
}

/// Why two requests conflict, in the terms the user-behaviour table is written in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    /// The same physical object, reached by whatever spelling or workspace.
    SameObject,
    /// The same directory entry: parent identity plus leaf name.
    SameSlot,
    /// One directory scope really contains the other, established by verified ancestry.
    SubtreeContains,
}

/// Whether two requests touch the same real resource in a conflicting way.
///
/// This is the whole coordination rule, stated once. It is a function of the two **complete**
/// resources — identity *and* verified location — because either alone answers only half the
/// question, and the previous implementation's failures were one half each: a commit that compared
/// only a string key missed a directory-versus-child conflict, and a slot rule that compared only a
/// parent identity invented a conflict between two unrelated siblings.
///
/// * two reads never conflict — an ordinary read does not take an observation lock;
/// * the same object conflicts when either side writes, under whatever name or workspace it was
///   reached;
/// * the same slot conflicts when either side writes, so a create and a delete-recreate of one
///   name are ordered;
/// * a directory scope conflicts with a write of the scope itself, with a write of any directory
///   whose verified location is inside it, and with a write of any **entry** whose verified parent
///   is inside it — that is the rename/delete-a-directory-versus-write-a-file-inside-it case,
///   decided by identity along a verified chain rather than by path text;
/// * a slot write and a directory scope conflict when the slot is the entry that names the
///   directory being replaced, and then *only*;
/// * nothing else conflicts, which is precisely how unrelated files stay parallel inside **and**
///   across workspaces, and how unrelated leaf names under one shared parent stay parallel.
#[must_use]
pub fn conflict_between(
    mine: &ResourceRequest,
    theirs: &ResourceRequest,
) -> Option<ConflictReason> {
    if !mine.access.is_writing() && !theirs.access.is_writing() {
        return None;
    }
    let my_identity = mine.resource.identity_of();
    let their_identity = theirs.resource.identity_of();
    if my_identity == their_identity {
        return Some(match my_identity {
            ResourceIdentity::Object(_) => ConflictReason::SameObject,
            ResourceIdentity::Slot { .. } | ResourceIdentity::ProspectiveEntry { .. } => {
                ConflictReason::SameSlot
            }
            ResourceIdentity::Subtree(_) => ConflictReason::SubtreeContains,
        });
    }
    if subtree_reaches(mine, theirs) || subtree_reaches(theirs, mine) {
        return Some(ConflictReason::SubtreeContains);
    }
    if prospective_scope_reaches(mine, theirs) || prospective_scope_reaches(theirs, mine) {
        return Some(ConflictReason::SubtreeContains);
    }
    // The delete-recreate case from the other side: one request names the **entry** that holds a
    // directory, the other names the directory itself. Their identities can never be equal — a
    // slot identity is a parent plus a leaf, a directory identity is an object — so equality
    // cannot decide it, and the location chain alone cannot either, because the slot's parent
    // location stops at the parent. What decides it is the two facts together, and only those:
    // the slot's parent location ends at the directory's parent, and the directory is that
    // parent's entry under the slot's own leaf. Any other reading makes every sibling under a
    // shared parent look like the directory, which is the false conflict this replaces.
    if let Some(reason) = same_declared_entry(mine, theirs) {
        return Some(reason);
    }
    if let Some(reason) = slot_names_scope_root(mine, theirs) {
        return Some(reason);
    }
    if let Some(reason) = slot_names_scope_root(theirs, mine) {
        return Some(reason);
    }
    None
}

/// Whether two requests name **one directory entry**, even though their shapes differ.
///
/// Identity equality cannot answer this: a [`FileResource::Slot`] is a verified parent plus one
/// leaf, and a [`FileResource::ProspectiveEntry`] is that same verified parent plus a whole pending
/// tail. When the tail is exactly that one leaf, the two describe one place — the entry under that
/// parent — and a caller that created the parent between the two derivations must not thereby get a
/// second, independent claim on the same name.
///
/// This is an entry-versus-entry rule only, and it is deliberately narrow: the two parents must be
/// the *same* verified directory (identity, and the same chain, so a shared parent identity under
/// two different walks is not read as one place), and the folded name must match under both
/// readings of the rule. Two unrelated uncreated parents never reach it, which is exactly how
/// `missing-one/note.txt` and `missing-two/note.txt` stay apart.
fn same_declared_entry(mine: &ResourceRequest, theirs: &ResourceRequest) -> Option<ConflictReason> {
    let (my_parent, my_name) = entry_parent_and_names(mine)?;
    let (their_parent, their_name) = entry_parent_and_names(theirs)?;
    if my_parent.identity != their_parent.identity {
        return None;
    }
    if !my_parent.location.same_chain(&their_parent.location) {
        return None;
    }
    if my_name != their_name {
        return None;
    }
    Some(ConflictReason::SameSlot)
}

/// The verified parent and the folded pending names of an entry-shaped resource.
fn entry_parent_and_names(request: &ResourceRequest) -> Option<(&SlotParent, Vec<String>)> {
    match &request.resource {
        FileResource::Slot { parent, leaf_name, .. } => {
            Some((parent, vec![parent.fold(leaf_name)]))
        }
        FileResource::ProspectiveEntry { parent, pending, .. } => {
            Some((parent, pending.iter().map(|component| parent.fold(component)).collect()))
        }
        FileResource::Object { .. } | FileResource::Subtree { .. } => None,
    }
}

/// Whether `entry` is the directory entry that names `scope`'s directory.
///
/// Provable only from a comparable chain plus the leaf: the scope's own directory must sit
/// directly inside the entry's parent, and the entry's leaf must be the *name* of that directory.
/// The name is not carried on a `FileIdentity`, so the caller that resolved both supplies it by
/// raising the directory under the entry's own leaf — which is why a scope whose display path is
/// not under the entry's display name is simply not this entry.
///
/// A sibling under the same parent fails the leaf test. A directory reached through a different
/// anchor is *not* automatically excluded any more: if the scope's directory identity sits inside
/// the entry's chain, the entry really does name a directory inside the entry's parent's scope, and
/// refusing it because the two walks started elsewhere is exactly the nested-workspace hole this
/// batch repairs. What still has to hold is the leaf: the entry's own verified name must be the
/// name the scope's directory was verified under.
fn slot_names_scope_root(
    entry: &ResourceRequest,
    scope: &ResourceRequest,
) -> Option<ConflictReason> {
    if !entry.access.is_writing() {
        return None;
    }
    let ResourceIdentity::Slot { parent_identity, leaf_name } = entry.resource.identity_of() else {
        return None;
    };
    let ResourceIdentity::Subtree(directory_identity) = scope.resource.identity_of() else {
        return None;
    };
    // A directory is not its own parent's entry.
    if directory_identity == parent_identity {
        return None;
    }
    let entry_location = entry.resource.location_of()?;
    // The scope's directory must sit directly in the entry's parent, or the entry's parent must sit
    // inside the scope's own verified location. Identity decides both, from either anchor.
    let scope_chain = scope.resource.containment_path()?;
    if scope_chain.chain.parent() != &parent_identity
        && !scope_chain.is_at_or_under(&parent_identity)
        && !entry_location.is_at_or_under(&directory_identity)
    {
        return None;
    }
    if !leaf_matches_scope_name(&leaf_name, scope, entry.resource.entry_parent()?) {
        return None;
    }
    Some(ConflictReason::SubtreeContains)
}

/// Whether a folded entry name is the name of the directory the scope replaces.
///
/// The directory's own name is the last component of the display path the caller recorded, folded
/// under the entry's rule. This is *not* path-text guessing about identity: it is the caller's own
/// statement of which name it verified the directory under, compared with the entry's verified
/// leaf under the same case rule. A caller that did not record a name simply gets `false`, which
/// is the conservative direction — the two requests then stay parallel and the object-level rule
/// still protects them.
fn leaf_matches_scope_name(
    folded_leaf: &str,
    scope: &ResourceRequest,
    parent: &SlotParent,
) -> bool {
    let path = scope.resource.owner().path.as_str();
    let name = path.rsplit('/').next().unwrap_or(path);
    !name.is_empty() && parent.comparison_key(name).is_some_and(|key| folded_leaf == key)
}

/// Whether `outer` is a directory scope that really contains `inner`, with `inner` writing.
///
/// The contained side must be writing: a read inside a directory that is being replaced is the
/// read's own problem to report as a sharing violation, and reads take no claim.
///
/// Containment is claimed from **verified identities**, and it is deliberately *not* gated on the
/// two walks having started at the same anchor. Both anchors are verified directory identities, so
/// an anchor can itself sit inside the other walk — which is how a workspace rooted at
/// `/tree/nested` is placed inside a directory scope taken by the workspace rooted at `/tree`. The
/// pre-fix rule returned "not proven" for that pair and a real nested write landed while the outer
/// scope was held.
///
/// What is *not* done: two chains that merely share a suffix are never read as one chain. Every
/// comparison below is identity equality against a directory that really appears in a verified
/// chain, so nothing is guessed from path text.
///
/// Two shapes of outer scope are handled, and both are real:
///
/// * a **verified** directory scope, whose directory identity is a fact and therefore appears in
///   the chains of everything under it;
/// * a scope over a directory that **does not exist yet** — the outer scope's own directory name is
///   one of the components the outer entry will create, so the two are compared as "the inner entry
///   is inside the directory the outer entry names". This is the case that would otherwise leave a
///   future directory's own file unprotected, and it is decided from the outer entry's verified
///   parent plus its folded pending components, never from path text crossing a workspace.
fn subtree_reaches(outer: &ResourceRequest, inner: &ResourceRequest) -> bool {
    if !inner.access.is_writing() {
        return false;
    }
    let ResourceIdentity::Subtree(outer_directory) = outer.resource.identity_of() else {
        return false;
    };
    let Some(inner_path) = inner.resource.containment_path() else {
        return false;
    };
    // The outer scope's own directory is inside itself.
    if inner.resource.identity_of() == ResourceIdentity::Subtree(outer_directory.clone()) {
        return true;
    }
    // Any resource whose verified chain passes through the outer directory is inside it. This
    // covers an object, a subtree, a slot and a prospective entry alike, because every one of them
    // reports the chain of the deepest directory that really exists above it.
    if inner_path.chain.is_at_or_under(&outer_directory) {
        return true;
    }
    // An entry may also be reached through a chain that does not *start* inside the outer
    // directory while still being created inside it: the entry's own verified parent is the
    // deepest directory it knows, and the outer scope may be an ancestor of that parent reached
    // from a different anchor. The parent's identity is the fact that decides it.
    if let Some(parent) = inner.resource.entry_parent() {
        return parent.location.is_at_or_under(&outer_directory)
            || parent.identity == outer_directory.clone();
    }
    false
}

/// Whether the *prospective* scope `outer` really contains `inner`.
///
/// A scope over a directory that does not exist yet has no identity to appear in anything's chain,
/// so containment has to be decided the other way round: the outer entry names a directory (plus
/// any components above it), and the inner entry is inside it exactly when the inner entry's
/// verified parent is that same verified directory and the outer entry's pending components are a
/// prefix of the inner entry's pending components — with at least one component left for the inner
/// entry, because a directory is not inside itself.
///
/// Both sides must share the same verified parent chain, so this never crosses two unrelated trees
/// and never guesses from workspace-relative text.
fn prospective_scope_reaches(outer: &ResourceRequest, inner: &ResourceRequest) -> bool {
    if !inner.access.is_writing() {
        return false;
    }
    let ResourceIdentity::ProspectiveEntry { parent_identity, relative_path } =
        outer.resource.identity_of()
    else {
        return false;
    };
    let Some((their_parent, their_names)) = entry_parent_and_names(inner) else {
        // Only an entry can be *inside* a directory that does not exist yet: an existing object or
        // directory proves its own chain, and the traversal above already decided it.
        return false;
    };
    if their_parent.identity != parent_identity {
        return false;
    }
    let Some(outer_parent) = outer.resource.entry_parent() else {
        return false;
    };
    if !outer_parent.location.same_chain(&their_parent.location) {
        return false;
    }
    let outer_components = relative_path.split('/').collect::<Vec<_>>();
    if outer_components.len() >= their_names.len() {
        // The inner entry is not strictly below the directory the outer entry names.
        return false;
    }
    outer_components
        .iter()
        .zip(their_names.iter())
        .all(|(outer_component, inner_component)| *outer_component == inner_component.as_str())
}

/// The scope form of a resource, for callers that only need "what directory does this replace".
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceScope {
    /// Exactly one object, at a verified location.
    Object { identity: FileIdentity, location: ResourceLocation },
    /// One directory entry under a verified parent.
    Slot { parent: FileIdentity, leaf_name: String, rule: SlotCaseRule },
    /// One entry below a directory that does not exist yet, with the whole pending tail.
    ProspectiveEntry { parent: FileIdentity, relative_path: String, rule: SlotCaseRule },
    /// One directory and everything under it.
    Subtree { directory: FileIdentity, location: ResourceLocation },
}

impl ResourceRequest {
    /// The scope this request really covers, so a caller can compare scopes without re-deriving
    /// the identity form.
    #[must_use]
    pub fn scope(&self) -> ResourceScope {
        match &self.resource {
            FileResource::Object { identity, location, .. } => {
                ResourceScope::Object { identity: identity.clone(), location: location.clone() }
            }
            FileResource::Slot { parent, leaf_name, .. } => ResourceScope::Slot {
                parent: parent.identity.clone(),
                leaf_name: parent.fold(leaf_name),
                rule: parent.case_rule,
            },
            FileResource::ProspectiveEntry { parent, pending, .. } => {
                ResourceScope::ProspectiveEntry {
                    parent: parent.identity.clone(),
                    relative_path: pending
                        .iter()
                        .map(|component| parent.fold(component))
                        .collect::<Vec<_>>()
                        .join("/"),
                    rule: parent.case_rule,
                }
            }
            FileResource::Subtree { directory_identity, location, .. } => ResourceScope::Subtree {
                directory: directory_identity.clone(),
                location: location.clone(),
            },
        }
    }

    /// Whether the request names a determinate resource.
    ///
    /// A slot or prospective entry whose volume rule is unknown is not determinate: two spellings of
    /// one entry cannot be shown equal or distinct, so a caller that must decide has to treat it as
    /// a missing fact rather than as a free resource. Everything else is determinate by
    /// construction, because the resolver that produced it opened the object or the parent through
    /// a verified handle.
    #[must_use]
    pub fn has_determinate_identity(&self) -> bool {
        match &self.resource {
            FileResource::Slot { parent, .. } | FileResource::ProspectiveEntry { parent, .. } => {
                parent.case_rule.is_determinate()
                    && match &self.resource {
                        FileResource::Slot { leaf_name, .. } => {
                            parent.comparison_key(leaf_name).is_some()
                        }
                        FileResource::ProspectiveEntry { pending, .. } => {
                            pending.iter().all(|name| parent.comparison_key(name).is_some())
                        }
                        _ => false,
                    }
            }
            FileResource::Object { .. } | FileResource::Subtree { .. } => true,
        }
    }
}

#[cfg(test)]
pub(crate) fn ascii_comparison_fixture() -> aura_domain_common::FileNameComparison {
    static TABLE: std::sync::OnceLock<aura_domain_common::FileNameComparison> =
        std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mut units = (0..=u16::MAX).collect::<Vec<_>>();
            for unit in b'a'..=b'z' {
                units[usize::from(unit)] = u16::from(unit.to_ascii_uppercase());
            }
            aura_domain_common::FileNameComparison::from_uppercase_table(units)
                .expect("ASCII fixture")
        })
        .clone()
}

#[cfg(test)]
mod tests;
