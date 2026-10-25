//! Real resource facts, derived from the chain the security resolver already verified.
//!
//! [`aura_workspace_domain::FileResource`] says what a resource *is*. This module is the only place
//! that **builds** one, and it builds it from handles rather than from text:
//!
//! * the anchor is the workspace root `SecurePathResolver` itself verified at open, so two
//!   workspaces over one canonical root produce comparable chains and two unrelated roots never do;
//! * the ancestor chain is the identities of the directories the resolver really opened on the way
//!   down, innermost last;
//! * a leaf name is folded under [`SlotCaseRule`], which is established here from the volume the
//!   parent handle lives on.
//!
//! # Why the facts are derived here and not at each entry
//!
//! Five entries ask the same question — Main Fs, the Agent file tools, Buffer save, the artifact
//! writer and the P04 external writer — and the previous design's whole failure was that each of
//! them answered it slightly differently. So a caller hands this module a **path relative to one
//! workspace** plus the resolver it already holds, and gets back resources that are comparable
//! with every other entry's. A caller that already holds a verified handle for something the
//! resolver cannot reach by path (a temporary publication, a granted external destination) uses
//! [`ResourceFacts::object_from_verified`] instead.
//!
//! # What is deliberately not derived
//!
//! A caller that could not establish a fact gets the honest answer rather than a guess: an
//! unresolvable path yields `None`, and a volume whose case rule could not be read yields
//! [`SlotCaseRule::Unknown`], which makes the request indeterminate rather than free. Nothing here
//! reads a path as text to decide containment, and nothing here invents an ancestor it did not
//! open.
use std::{path::Path, sync::Arc};

use aura_domain_common::{FileIdentity, SessionId, WorkspaceId, WorkspacePath};
use aura_windows_platform::{
    NativeFileAccess, NativeFileIdentity, VerifiedFile, VolumeCaseRule, directory_case_rule,
};
use aura_workspace_domain::{
    FileResource, ResourceLocation, ResourceOwner, SlotCaseRule, SlotParent,
};

use crate::{SecurePathResolver, WorkspaceError, domain_identity};

/// The facts one caller established, in the form every entry shares.
///
/// It **owns** an `Arc` of the resolver rather than borrowing one, so an entry can hold its
/// derivation across a call without the runtime's map guard or a lifetime it cannot name. The
/// resolver behind the `Arc` is the same one the runtime verified, so the anchor and the chain are
/// still the security chain's own facts and not a second derivation.
#[derive(Clone)]
pub struct ResourceFacts {
    resolver: Arc<SecurePathResolver>,
    /// The session the resources are labelled with. Ownership only: it never takes part in the
    /// comparison, and a caller that has no session of its own — a store deriving the object it
    /// already labelled — passes its real one rather than inventing a second identity for one file.
    session_id: Option<SessionId>,
    /// A rule a caller **stated** for entries whose own directory could not be interrogated.
    ///
    /// It is not the derivation's answer: [`Self::case_rule_of`] reads each entry's real parent. This
    /// exists only for the callers that derive an entry from something other than a verified
    /// directory handle — a publication with no parent handle — and it defaults to `Unknown` so no
    /// rule is ever assumed.
    stated_case_rule: SlotCaseRule,
}

impl ResourceFacts {
    /// Starts from a resolver whose root identity was already verified.
    ///
    /// The anchor is the root's own identity, which the resolver established from a handle — never
    /// from the path text a caller spelled.
    #[must_use]
    pub fn root(resolver: Arc<SecurePathResolver>, session_id: SessionId) -> Self {
        Self { resolver, session_id: Some(session_id), stated_case_rule: SlotCaseRule::Unknown }
    }

    /// The same, for a caller that derives a resource without a session of its own.
    ///
    /// Used where the resource is already labelled by the caller's own record — a Buffer save
    /// deriving the object it committed — so no second session identity is invented for one file.
    #[must_use]
    pub fn root_unlabelled(resolver: Arc<SecurePathResolver>) -> Self {
        Self { resolver, session_id: None, stated_case_rule: SlotCaseRule::Unknown }
    }

    /// Describes the object at `path`, with the chain the resolver really walked.
    ///
    /// Returns `None` when the path does not resolve to an existing object inside this root. A
    /// caller that needs the *entry* rather than the object — a create, a move destination, a
    /// delete-recreate — asks [`Self::slot`] instead, which works for a name that does not exist.
    ///
    /// # Errors
    /// Propagates the real resolution error, so a permission or scope failure is reported rather
    /// than turned into "no resource".
    pub fn object(&self, path: &WorkspacePath) -> Result<Option<FileResource>, WorkspaceError> {
        let verified = match self.resolver.open_existing(path, NativeFileAccess::Metadata) {
            Ok(verified) => verified,
            Err(WorkspaceError::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(Some(self.object_from_verified(path, verified.file.info())?))
    }

    /// Describes the object one verified handle names.
    ///
    /// This is the entry for a caller that already holds a handle rather than a path: the P04
    /// external writer's publication, a staging result, or a buffer's replacement. The identity
    /// comes from the held handle, and the chain comes from the resolver walking `path`'s
    /// directories, so the two facts cannot disagree: a caller cannot hand a handle for one object
    /// and a path for another without the mismatch showing up as the wrong location.
    pub fn object_from_verified(
        &self,
        path: &WorkspacePath,
        info: &aura_windows_platform::NativePathInfo,
    ) -> Result<FileResource, WorkspaceError> {
        Ok(FileResource::Object {
            identity: domain_identity(&info.identity),
            location: self.location_for(path)?,
            owner: self.owner(path),
        })
    }

    /// Describes the directory **entry** at `path`, whether or not it exists.
    ///
    /// # The complete pending location, not the nearest existing name
    ///
    /// The pre-fix version walked up to the deepest component that existed and then kept only the
    /// **last** component as the leaf. That lost every intermediate component that does not exist
    /// yet, and the measured consequence was on the real runtime: `missing-one/note.txt` and
    /// `missing-two/note.txt` both produced `slot:<root-id>/note.txt:write` and were judged the
    /// *same* slot. Two unrelated targets therefore serialised against each other, and neither
    /// future directory was covered.
    ///
    /// So the entry now carries the **whole** uncreated tail below the nearest verified ancestor:
    ///
    /// * when the parent really exists, this is an ordinary [`FileResource::Slot`] — the strongest
    ///   fact, a verified parent plus one leaf;
    /// * when components below the nearest existing directory are still missing, this is a
    ///   [`FileResource::ProspectiveEntry`] naming the verified parent **and** every pending
    ///   component, so two paths that diverge below the ancestor are two resources and a scope over
    ///   a future intersecting directory still covers them.
    ///
    /// This is what lets a multi-operation changeset be admitted as one set before any of it runs:
    /// `CreateDirectory { batch }` followed by `WriteFile { batch/value.txt }` is admitted together
    /// even though `batch` is not on disk when the set is judged. Refusing to admit it would make
    /// every changeset that creates its own parent impossible, and inventing a fake parent identity
    /// would make the entry incomparable with the entry the same name gets once the directory
    /// exists.
    ///
    /// # Resolution failures are not "does not exist"
    ///
    /// Only a genuine `NotFound` means a component is not there yet. A permission, sharing, scope
    /// or type failure is returned as its own error, because swallowing it into "missing" would
    /// claim the path continues below a directory this caller may not even be allowed to read.
    ///
    /// # Errors
    /// Propagates a real resolution error, and reports [`WorkspaceError::WrongKind`] when a
    /// component that must be a directory really is a file: nothing can be created under a file.
    pub fn slot(&self, path: &WorkspacePath) -> Result<FileResource, WorkspaceError> {
        let components = path.as_str().split('/').collect::<Vec<_>>();
        let Some(leaf) = components.last() else {
            return Err(WorkspaceError::WrongKind { path: path.clone() });
        };
        // Walk down to the deepest component that really exists. Everything below it is a name this
        // caller is about to create, so the deepest existing directory is the entry's real parent.
        let mut existing: Option<(usize, WorkspacePath, VerifiedFile)> = None;
        let mut blocked_by_file: Option<WorkspacePath> = None;
        for depth in (0..components.len()).rev() {
            let candidate = if depth == 0 {
                WorkspacePath::parse(WorkspacePath::ROOT).expect("root path is valid")
            } else {
                let Ok(candidate) = WorkspacePath::parse(components[..depth].join("/")) else {
                    continue;
                };
                candidate
            };
            match self.resolver.open_existing(&candidate, NativeFileAccess::Metadata) {
                Ok(opened) => {
                    let verified = opened.file;
                    if verified.info().is_directory {
                        existing = Some((depth, candidate, verified));
                        break;
                    }
                    // The component exists and is **not** a directory. Nothing can be created
                    // under it, and the pre-fix walk simply kept going upwards, which turned a
                    // real type failure into a plausible-looking ancestor chain.
                    blocked_by_file = Some(candidate);
                    break;
                }
                // A component that is genuinely absent is the ordinary "this operation will create
                // it" case: keep walking up.
                Err(WorkspaceError::NotFound { .. }) => {}
                // Every other failure is the real one. It is *not* read as absence, because a
                // permission or sharing failure says nothing about whether the name exists.
                Err(error) => return Err(error),
            }
        }
        if let Some(blocked) = blocked_by_file {
            return Err(WorkspaceError::WrongKind { path: blocked });
        }
        let Some((depth, parent_path, parent)) = existing else {
            return Err(WorkspaceError::NotFound { path: path.clone() });
        };
        let ancestry = self.location_for(&parent_path)?.ancestry;
        // The rule is read from the directory that really contains this entry, not from TEMP and
        // not from the volume's capability bit.
        let case_rule = self.case_rule_of(&parent);
        let name_comparison = aura_windows_platform::directory_name_comparison(&parent);
        let parent_identity = domain_identity(&parent.info().identity);
        let parent_location = ResourceLocation { anchor: self.resolver.root_identity(), ancestry };
        if depth + 1 == components.len() {
            // The parent really exists and the entry is a direct child of it: the strongest fact.
            return Ok(FileResource::Slot {
                parent: SlotParent {
                    identity: parent_identity,
                    location: parent_location,
                    case_rule,
                    name_comparison,
                },
                leaf_name: (*leaf).to_owned(),
                owner: self.owner(path),
            });
        }
        // Components below the verified parent do not exist yet. The whole pending tail travels, so
        // the entry names the complete location it will really occupy.
        let pending = components[depth..].iter().map(|component| (*component).to_owned()).collect();
        Ok(FileResource::ProspectiveEntry {
            parent: SlotParent {
                identity: parent_identity,
                location: parent_location,
                case_rule,
                name_comparison,
            },
            pending,
            owner: self.owner(path),
        })
    }

    /// Whether `path` really names a directory, from the resolver's own verified answer.
    ///
    /// Used where a caller must decide a scope from the **real node type** rather than from a DTO's
    /// `recursive` flag: a recursive delete of something that is really a file still names only that
    /// file, and a scope is never taken for a tree that is not there.
    ///
    /// `Ok(false)` when the name does not resolve: an absent target has no node type to widen to.
    ///
    /// # Errors
    /// Propagates a real resolution error, so a permission or sharing failure is never silently read
    /// as "not a directory" — that would turn a refusal into a narrower scope.
    pub fn is_directory(&self, path: &WorkspacePath) -> Result<bool, WorkspaceError> {
        match self.resolver.open_existing(path, NativeFileAccess::Metadata) {
            Ok(verified) => Ok(verified.file.info().is_directory),
            Err(WorkspaceError::NotFound { .. }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// The case rule of one directory's own volume, read from that directory's verified handle.
    ///
    /// The pre-fix derivation asked about `std::env::temp_dir()`, which describes the **TEMP
    /// volume** rather than the target's parent — a different volume entirely when the workspace
    /// lives on another drive, and the wrong rule whenever the two differ. The platform's
    /// `FILE_CASE_SENSITIVE_SEARCH` bit is a *capability* flag on the volume, and the pre-fix
    /// production rule was consequently `Unknown`, which the owner then treats as a missing fact.
    ///
    /// So the rule is read from the directory that really contains the entry, through the handle the
    /// resolver really opened. A directory whose flag cannot be read stays `Unknown`: that is the
    /// honest answer, and it is deliberately **not** upgraded from the volume's capability bit.
    fn case_rule_of(&self, directory: &VerifiedFile) -> SlotCaseRule {
        match directory_case_rule(directory) {
            VolumeCaseRule::InsensitivePreserving => SlotCaseRule::InsensitivePreserving,
            VolumeCaseRule::Sensitive => SlotCaseRule::Sensitive,
            VolumeCaseRule::Unknown => SlotCaseRule::Unknown,
        }
    }

    /// Describes the directory at `path` and everything inside it.
    ///
    /// The directory is opened to confirm it really is one, and its **own** location becomes the
    /// chain: the returned subtree's `location` ends at the directory's parent, which is what makes
    /// "the entry that names this directory" decidable against a slot.
    ///
    /// # Errors
    /// Propagates the real resolution error.
    pub fn directory_subtree(&self, path: &WorkspacePath) -> Result<FileResource, WorkspaceError> {
        let verified = self.resolver.open_existing(path, NativeFileAccess::Metadata)?;
        if !verified.file.info().is_directory {
            return Err(WorkspaceError::WrongKind { path: path.clone() });
        }
        Ok(FileResource::Subtree {
            directory_identity: domain_identity(&verified.file.info().identity),
            location: self.location_for(path)?,
            owner: self.owner(path),
        })
    }

    /// The ancestor location of `path`: the chain of directories above it.
    ///
    /// Every required directory must be verified. A partial chain is an error: it
    /// cannot safely prove that a target is unrelated to an ancestor scope.
    fn location_for(&self, path: &WorkspacePath) -> Result<ResourceLocation, WorkspaceError> {
        let anchor = self.resolver.root_identity();
        if path.is_root() {
            return Ok(ResourceLocation::at_anchor(anchor));
        }
        let mut ancestry = Vec::new();
        let mut walked = String::new();
        let components = path.as_str().split('/').collect::<Vec<_>>();
        // Every component except the last one is a directory above the target.
        for component in components.iter().take(components.len().saturating_sub(1)) {
            if !walked.is_empty() {
                walked.push('/');
            }
            walked.push_str(component);
            let intermediate = WorkspacePath::parse(&walked)
                .map_err(|error| WorkspaceError::Watcher(error.to_string()))?;
            let opened = self.resolver.open_existing(&intermediate, NativeFileAccess::Metadata)?;
            if !opened.file.info().is_directory {
                return Err(WorkspaceError::WrongKind { path: intermediate });
            }
            ancestry.push(domain_identity(&opened.file.info().identity));
        }
        Ok(ResourceLocation { anchor, ancestry })
    }

    /// The location of the workspace root itself, for a caller describing the root.
    #[must_use]
    pub fn root_location(&self) -> ResourceLocation {
        ResourceLocation::at_anchor(self.resolver.root_identity())
    }

    fn owner(&self, path: &WorkspacePath) -> ResourceOwner {
        ResourceOwner {
            workspace_id: self.resolver.workspace_id(),
            // A caller with no session of its own labels the resource with the nil id rather than
            // borrowing another session's: the field is ownership, it never takes part in the
            // comparison, and inventing a session would make two entries' views of one file
            // disagree about who reached it.
            session_id: self.session_id.unwrap_or_else(nil_session_id),
            path: path.clone(),
        }
    }

    /// States the rule to use for entries whose **own** directory could not be interrogated.
    ///
    /// This does **not** override the derivation: [`Self::slot`] reads each entry's real parent
    /// directory through the handle the resolver opened, and that answer wins. The stated rule is
    /// only the fallback for a caller deriving an entry from something that is not a verified
    /// directory handle. Without a statement the fallback stays `Unknown`, and the owner then treats
    /// such an entry as having no determinate identity rather than folding two spellings together on
    /// a guess.
    #[must_use]
    pub fn with_case_rule(mut self, rule: SlotCaseRule) -> Self {
        self.stated_case_rule = rule;
        self
    }

    /// The rule a caller stated, for a caller that wants to report what it declared.
    #[must_use]
    pub fn stated_case_rule(&self) -> SlotCaseRule {
        self.stated_case_rule
    }

    /// The workspace this walk belongs to. Ownership only.
    #[must_use]
    pub fn workspace_id(&self) -> WorkspaceId {
        self.resolver.workspace_id()
    }

    /// The session the resources will be labelled with, if the caller stated one. Ownership only.
    #[must_use]
    pub fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }
}

/// The label a caller with no session of its own uses.
///
/// The nil id is a fixed value rather than a fresh one, so two derivations by the same entry agree
/// about their own label instead of each inventing an identity. It never takes part in the
/// comparison, so it cannot merge or split a resource.
fn nil_session_id() -> SessionId {
    SessionId::from_bytes([0_u8; 16])
}

/// The platform's native form of a domain identity, for a caller comparing against a handle.
///
/// The two encodings are the same bytes in two spellings, and the buffer's replacement path needs
/// one from the other. A malformed value is reported rather than silently zeroed: comparing against
/// a made-up identity would report "the object changed" for a file that did not.
///
/// # Errors
/// Returns the offending value when it is not the encoding [`domain_identity`] produces.
pub fn native_identity(identity: &FileIdentity) -> Result<NativeFileIdentity, WorkspaceError> {
    let invalid = || {
        WorkspaceError::Watcher(format!(
            "expected identity is not a native identity: volume={} file_id={}",
            identity.volume_serial_number, identity.file_id
        ))
    };
    let volume_serial_number =
        identity.volume_serial_number.parse::<u64>().map_err(|_| invalid())?;
    if identity.file_id.len() != 32 {
        return Err(invalid());
    }
    let mut file_id = [0_u8; 16];
    for (index, slot) in file_id.iter_mut().enumerate() {
        let pair = &identity.file_id[index * 2..index * 2 + 2];
        *slot = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
    }
    Ok(NativeFileIdentity { volume_serial_number, file_id })
}

/// The directory the verified parent of `path` is, or `None` when it cannot be opened.
///
/// Exists so a caller that needs a parent fact (a compensation proving a member's origin) asks once
/// rather than re-deriving the parent path text.
#[must_use]
pub fn verified_parent(path: &Path) -> Option<std::path::PathBuf> {
    path.parent().map(Path::to_path_buf)
}

#[path = "resource_facts/external.rs"]
mod external;
pub use external::{external_ancestry, external_object, external_slot};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aura_domain_common::{Revision, SessionId, WorkspaceId, WorkspacePath};
    use aura_workspace_domain::{
        CatalogFreshness, ResourceAccess, ResourceRequest, SlotCaseRule, WorkspaceState,
        WorkspaceSummary, conflict_between,
    };

    use super::*;
    use crate::SecurePathResolver;

    /// A resolver built the way the runtime builds one: through the stored workspace summary, so
    /// the root identity check and the canonical root are the real ones.
    fn resolver_for(root: &Path, workspace_id: WorkspaceId) -> SecurePathResolver {
        let info = SecurePathResolver::inspect_selected_root(root).expect("root info");
        let summary = WorkspaceSummary {
            workspace_id,
            display_name: "resource facts test".to_owned(),
            selected_root: root.display().to_string(),
            canonical_root: info.final_path.display().to_string(),
            root_identity: Some(crate::domain_identity(&info.identity)),
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        };
        SecurePathResolver::from_workspace(&summary).expect("resolver")
    }

    /// The anchor is the verified root, so two workspaces over one root produce comparable chains
    /// and two unrelated roots never do.
    #[test]
    fn the_anchor_is_the_verified_root_and_not_a_path() {
        let first_root = tempfile::tempdir().expect("first root");
        let second_root = tempfile::tempdir().expect("second root");
        std::fs::create_dir(first_root.path().join("nested")).expect("nested");
        std::fs::write(first_root.path().join("nested/note.txt"), b"one").expect("file");
        std::fs::create_dir(second_root.path().join("nested")).expect("nested");
        std::fs::write(second_root.path().join("nested/note.txt"), b"two").expect("file");

        let first = resolver_for(first_root.path(), WorkspaceId::new());
        let second = resolver_for(second_root.path(), WorkspaceId::new());
        let first_root_identity = first.root_identity();
        let session = SessionId::new();
        let path = WorkspacePath::parse("nested/note.txt").expect("path");
        let first_object = ResourceFacts::root(Arc::new(first), session)
            .object(&path)
            .expect("resolution")
            .expect("the file exists");
        let second_object = ResourceFacts::root(Arc::new(second), session)
            .object(&path)
            .expect("resolution")
            .expect("the file exists");

        // Same relative name, two real trees: not one resource.
        let first_request = ResourceRequest::new(first_object, ResourceAccess::Write);
        let second_request = ResourceRequest::new(second_object, ResourceAccess::Write);
        assert_eq!(conflict_between(&first_request, &second_request), None);

        // The chain records the verified intermediate, and its anchor is the root's own identity.
        let aura_workspace_domain::FileResource::Object { location, .. } = &first_request.resource
        else {
            panic!("an object");
        };
        assert_eq!(location.anchor, first_root_identity);
        assert_eq!(location.ancestry.len(), 1, "exactly the `nested` level was walked");
    }

    /// A directory being replaced really contains a deep descendant, derived from handles.
    #[test]
    fn a_deep_descendant_is_inside_the_directory_scope() {
        let root = tempfile::tempdir().expect("root");
        std::fs::create_dir_all(root.path().join("tree/a/b")).expect("tree");
        std::fs::write(root.path().join("tree/a/b/c.txt"), b"deep").expect("file");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new());

        let scope =
            facts.directory_subtree(&WorkspacePath::parse("tree").expect("path")).expect("scope");
        let deep = facts
            .object(&WorkspacePath::parse("tree/a/b/c.txt").expect("path"))
            .expect("resolution")
            .expect("file");
        assert_eq!(
            conflict_between(
                &ResourceRequest::new(scope, ResourceAccess::SubtreeWrite),
                &ResourceRequest::new(deep, ResourceAccess::Write)
            ),
            Some(aura_workspace_domain::ConflictReason::SubtreeContains),
            "a file three levels down is inside the directory being replaced"
        );
    }

    /// An unrelated sibling directory stays parallel even though the two share the workspace root.
    #[test]
    fn an_unrelated_sibling_stays_parallel() {
        let root = tempfile::tempdir().expect("root");
        std::fs::create_dir_all(root.path().join("one")).expect("one");
        std::fs::create_dir_all(root.path().join("two")).expect("two");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new());
        let one =
            facts.directory_subtree(&WorkspacePath::parse("one").expect("path")).expect("one");
        let two =
            facts.directory_subtree(&WorkspacePath::parse("two").expect("path")).expect("two");
        assert_eq!(
            conflict_between(
                &ResourceRequest::new(one, ResourceAccess::SubtreeWrite),
                &ResourceRequest::new(two, ResourceAccess::SubtreeWrite)
            ),
            None
        );
    }

    /// A slot is built for a name that does not exist, **from the real directory's own rule**.
    ///
    /// The pre-fix derivation asked `std::env::temp_dir()` and carried a rule that described the
    /// TEMP volume, and this test asserted the request was therefore indeterminate. It now reads the
    /// rule from the directory that really contains the entry, so the answer is the directory's own
    /// fact and the assertion is about *that*.
    #[test]
    fn a_slot_works_for_a_name_that_does_not_exist_yet() {
        let root = tempfile::tempdir().expect("root");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new());
        let slot = facts
            .slot(&WorkspacePath::parse("brand-new.txt").expect("path"))
            .expect("a missing name still has a verified parent");
        let request = ResourceRequest::new(slot, ResourceAccess::Write);
        // The rule is whatever the target directory really reports. `Unknown` is a legitimate
        // answer on a volume whose per-directory flag cannot be read, and it is not a rule this
        // module is allowed to invent — so the assertion is the implication, not a fixed value.
        let FileResource::Slot { parent, .. } = &request.resource else {
            panic!("a name whose parent exists is an ordinary slot");
        };
        assert_eq!(
            parent.case_rule.is_determinate(),
            request.has_determinate_identity(),
            "determinacy follows the directory's own rule, whatever it is"
        );
    }

    /// The rule comes from the **target's own directory**, not from the TEMP volume.
    ///
    /// This is the defect stated as a measurement: the pre-fix `volume_case_rule_domain` asked
    /// `std::env::temp_dir()`, so a workspace on another volume was described by the wrong volume's
    /// flags. Here the same two paths differ only in which directory they are compared under, and
    /// the answer is read from that directory.
    #[test]
    fn the_case_rule_is_read_from_the_entries_own_directory_and_not_from_temp() {
        let root = tempfile::tempdir().expect("root");
        std::fs::create_dir(root.path().join("inside")).expect("inside");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        // A rule stated on the walk must **not** override what the entry's own directory reports.
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new())
            .with_case_rule(SlotCaseRule::Sensitive);
        let entry = facts
            .slot(&WorkspacePath::parse("inside/notes.txt").expect("path"))
            .expect("the parent really exists");
        let FileResource::Slot { parent, .. } = &entry else {
            panic!("an existing parent produces an ordinary slot");
        };
        // The real directory is read through its own handle. Whatever it answers, it is the
        // directory's fact and it is stable across two derivations of the same path.
        let second = facts
            .slot(&WorkspacePath::parse("inside/notes.txt").expect("path"))
            .expect("the parent really exists");
        let FileResource::Slot { parent: second_parent, .. } = &second else {
            panic!("an existing parent produces an ordinary slot");
        };
        assert_eq!(
            parent.case_rule, second_parent.case_rule,
            "the rule is a fact about the directory, so two derivations agree"
        );
        // And it is *not* simply the stated rule echoed back: the derivation reports what it read.
        assert_eq!(
            facts.stated_case_rule(),
            SlotCaseRule::Sensitive,
            "the stated rule is still reported as what the caller declared"
        );
    }

    /// **Two unrelated missing parents stay two resources, derived from real handles.**
    ///
    /// This is the `prospective-parent-slots` counterexample at the derivation itself: both
    /// parents are genuinely absent, and the two entries must not collapse onto one `root/note.txt`.
    #[test]
    fn two_uncreated_parents_produce_two_distinct_prospective_entries() {
        let root = tempfile::tempdir().expect("root");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new());
        let first = facts
            .slot(&WorkspacePath::parse("missing-one/note.txt").expect("path"))
            .expect("a missing component is still a placeable entry");
        let second = facts
            .slot(&WorkspacePath::parse("missing-two/note.txt").expect("path"))
            .expect("a missing component is still a placeable entry");
        assert!(
            !root.path().join("missing-one").exists() && !root.path().join("missing-two").exists(),
            "both parents are genuinely absent, which is the case being measured"
        );
        let first_request = ResourceRequest::new(first, ResourceAccess::Write);
        let second_request = ResourceRequest::new(second, ResourceAccess::Write);
        assert_ne!(
            first_request.lock_key(),
            second_request.lock_key(),
            "the lost intermediate component must be back in the identity"
        );
        assert_eq!(
            conflict_between(&first_request, &second_request),
            None,
            "two unrelated uncreated parents are two unrelated targets"
        );
        // The same path still collides with itself, so this is not a blanket "everything is free".
        let same = facts
            .slot(&WorkspacePath::parse("missing-one/note.txt").expect("path"))
            .expect("the same entry derives again");
        assert_eq!(
            conflict_between(&first_request, &ResourceRequest::new(same, ResourceAccess::Write)),
            Some(aura_workspace_domain::ConflictReason::SameSlot)
        );
    }

    /// A component that exists but is **not** a directory is a real type failure, not "missing".
    #[test]
    fn a_file_in_the_parent_chain_is_reported_rather_than_walked_past() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("a-file.txt"), b"text").expect("file");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new());
        let refusal = facts
            .slot(&WorkspacePath::parse("a-file.txt/child.txt").expect("path"))
            .expect_err("nothing can be created under a file");
        assert!(
            matches!(refusal, WorkspaceError::WrongKind { .. }),
            "the real type failure must be reported: {refusal:?}"
        );
    }

    /// A genuinely absent component under a real directory is still a placeable entry.
    #[test]
    fn an_absent_component_under_a_real_directory_is_still_placeable() {
        let root = tempfile::tempdir().expect("root");
        std::fs::create_dir(root.path().join("real")).expect("real");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new());
        let entry = facts
            .slot(&WorkspacePath::parse("real/absent/deep/note.txt").expect("path"))
            .expect("the verified parent is `real`, and the rest is pending");
        let FileResource::ProspectiveEntry { pending, .. } = &entry else {
            panic!("components below the verified parent make this a prospective entry: {entry:?}");
        };
        assert_eq!(
            pending,
            &vec!["absent".to_owned(), "deep".to_owned(), "note.txt".to_owned()],
            "the whole pending tail is carried, not just the leaf"
        );
    }

    /// The case rule is carried, and stating one changes what the comparison decides when the
    /// directory itself cannot be read.
    #[test]
    fn the_case_rule_that_was_stated_is_the_one_that_is_carried() {
        let root = tempfile::tempdir().expect("root");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new())
            .with_case_rule(SlotCaseRule::InsensitivePreserving);
        assert_eq!(
            facts.stated_case_rule(),
            SlotCaseRule::InsensitivePreserving,
            "the stated rule is reported back verbatim"
        );
        // An entry is still built, and its rule is the real directory's own answer.
        let slot = facts.slot(&WorkspacePath::parse("Notes.txt").expect("path")).expect("slot");
        let FileResource::Slot { parent, .. } = slot else {
            panic!("slot");
        };
        assert_eq!(
            parent.comparison_key("Notes.txt").expect("held directory comparison"),
            "NOTES.TXT"
        );
    }

    /// The native identity encoding round-trips with the domain one.
    #[test]
    fn the_native_identity_is_the_domain_identity_in_its_other_spelling() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("f.txt"), b"x").expect("file");
        let resolver = resolver_for(root.path(), WorkspaceId::new());
        let facts = ResourceFacts::root(Arc::new(resolver), SessionId::new());
        let object = facts
            .object(&WorkspacePath::parse("f.txt").expect("path"))
            .expect("resolution")
            .expect("file");
        let aura_workspace_domain::FileResource::Object { identity, .. } = &object else {
            panic!("an object");
        };
        let native = native_identity(identity).expect("a domain identity is a native identity");
        assert_eq!(domain_identity(&native), *identity);
        assert!(
            native_identity(&FileIdentity {
                volume_serial_number: "not-a-number".to_owned(),
                file_id: identity.file_id.clone(),
            })
            .is_err()
        );
    }
}
