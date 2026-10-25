//! Vault file operations reuse the filesystem broker's WorkspacePath parser,
//! SecurePathResolver and handle-bound publication primitives. This narrower
//! root rejects junctions and hardlinks even if their target stays inside it.
use aura_domain_common::{Revision, WorkspaceId, WorkspacePath};
use aura_windows_platform::{
    HandleBoundPublishedFile, NativeFileIdentity, NativePathInfo, VerifiedFile, final_path_within,
    handle_bound_create_directory, handle_bound_create_new, handle_bound_delete,
    handle_bound_replace_checked, inspect_existing_path, open_held_for_move, open_held_for_read,
    open_verified_publish_parent,
};
use aura_workspace::SecurePathResolver;
use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};
use std::{
    ffi::OsStr,
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 1_048_576;

#[derive(Debug, thiserror::Error)]
pub enum VaultFileError {
    #[error("Vault filesystem error: {0}")]
    Io(#[from] io::Error),
    #[error("unsafe Vault path or file identity")]
    UnsafePath,
    #[error("Vault content conflict; current hash: {current_hash:?}")]
    Conflict { current_hash: Option<String> },
    #[error("Vault publication failed: {0}")]
    Publication(#[from] aura_windows_platform::HandlePublishError),
    #[error("Vault rollback failed; reconciliation required")]
    RecoveryRequired,
}

pub struct VaultFileSystem {
    root: PathBuf,
    anchor: NativePathInfo,
    tail: PathBuf,
    root_identity: std::sync::Mutex<Option<NativeFileIdentity>>,
}

pub struct VaultChange {
    pub path: WorkspacePath,
    pub expected_hash: Option<String>,
    pub bytes: Option<Vec<u8>>,
}
pub struct VaultRead {
    pub bytes: Vec<u8>,
    pub hash: String,
    pub identity: NativeFileIdentity,
}
pub struct VaultEntry {
    pub path: WorkspacePath,
    pub directory: bool,
}
struct Parents {
    held: Vec<VerifiedFile>,
    leaf: String,
}
impl Parents {
    fn parent(&self) -> &VerifiedFile {
        self.held.last().expect("held root")
    }
}

impl VaultFileSystem {
    pub fn new(root: PathBuf) -> Result<Self, VaultFileError> {
        if !root.is_absolute() {
            return Err(VaultFileError::UnsafePath);
        }
        reject_ancestor_links(&root)?;
        let mut nearest = root.as_path();
        while !nearest.try_exists()? {
            nearest = nearest.parent().ok_or(VaultFileError::UnsafePath)?;
        }
        if inspect_existing_path(nearest, false)?.is_reparse_point {
            return Err(VaultFileError::UnsafePath);
        }
        let tail =
            root.strip_prefix(nearest).map_err(|_| VaultFileError::UnsafePath)?.to_path_buf();
        let anchor = open_verified_publish_parent(nearest)?;
        if anchor.info().is_reparse_point {
            return Err(VaultFileError::UnsafePath);
        }
        Ok(Self {
            root,
            anchor: anchor.info().clone(),
            tail,
            root_identity: std::sync::Mutex::new(None),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn exists(&self) -> Result<bool, VaultFileError> {
        Ok(self.root.try_exists()?)
    }
    pub fn identity(&self) -> Result<NativeFileIdentity, VaultFileError> {
        let parents = self.parents(&WorkspacePath::parse(".identity").expect("leaf"), false)?;
        Ok(parents.parent().info().identity.clone())
    }
    pub fn ensure_directories(&self, directories: &[&str]) -> Result<(), VaultFileError> {
        for directory in directories {
            let path = WorkspacePath::parse(format!("{directory}/.layout"))
                .map_err(|_| VaultFileError::UnsafePath)?;
            self.parents(&path, true)?;
        }
        Ok(())
    }
    fn resolver(&self, root: &VerifiedFile) -> Result<SecurePathResolver, VaultFileError> {
        let i = &root.info().identity;
        let summary = WorkspaceSummary {
            workspace_id: WorkspaceId::new(),
            display_name: "Vault".into(),
            selected_root: self.root.display().to_string(),
            canonical_root: root.info().final_path.display().to_string(),
            root_identity: Some(aura_domain_common::FileIdentity {
                volume_serial_number: i.volume_serial_number.to_string(),
                file_id: i.file_id.iter().map(|b| format!("{b:02x}")).collect(),
            }),
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        };
        SecurePathResolver::from_workspace(&summary).map_err(|_| VaultFileError::UnsafePath)
    }
    fn parents(&self, path: &WorkspacePath, create: bool) -> Result<Parents, VaultFileError> {
        reject_ancestor_links(&self.root)?;
        if path.is_root() {
            return Err(VaultFileError::UnsafePath);
        }
        let mut held = vec![];
        let mut current = open_verified_publish_parent(&self.anchor.final_path)?;
        if current.info().identity != self.anchor.identity {
            return Err(VaultFileError::UnsafePath);
        }
        for component in self.tail.components() {
            let name = component.as_os_str();
            let next = current.info().final_path.join(name);
            if !next.try_exists()? && create {
                let created = handle_bound_create_directory(&current, name)?;
                drop(created);
            }
            let info = inspect_existing_path(&next, false)?;
            if info.is_reparse_point
                || info.identity.volume_serial_number != self.anchor.identity.volume_serial_number
            {
                return Err(VaultFileError::UnsafePath);
            }
            let child = open_verified_publish_parent(&next)?;
            held.push(current);
            current = child;
        }
        let root_info = current.info().clone();
        let mut identity = self.root_identity.lock().unwrap_or_else(|e| e.into_inner());
        if identity.as_ref().is_some_and(|known| known != &root_info.identity) {
            return Err(VaultFileError::UnsafePath);
        }
        *identity = Some(root_info.identity.clone());
        drop(identity);
        let resolver = self.resolver(&current)?;
        held.push(current);
        let mut components = path.as_str().split('/').collect::<Vec<_>>();
        let leaf = components.pop().ok_or(VaultFileError::UnsafePath)?.to_owned();
        for component in components {
            let parent = held.last().expect("held root");
            let next = parent.info().final_path.join(component);
            if !next.try_exists()? && create {
                let created = handle_bound_create_directory(parent, OsStr::new(component))?;
                drop(created);
            }
            let info = inspect_existing_path(&next, false)?;
            if info.is_reparse_point
                || !final_path_within(&root_info, &info)
                || info.identity.volume_serial_number != root_info.identity.volume_serial_number
            {
                return Err(VaultFileError::UnsafePath);
            }
            held.push(open_verified_publish_parent(&next)?);
        }
        // The same final-path resolver as ordinary fs tools establishes the
        // destination parent. Holding every directory prevents later replacement.
        resolver.validate_parent(path).map_err(|_| VaultFileError::UnsafePath)?;
        Ok(Parents { held, leaf })
    }
    pub fn read(&self, path: &WorkspacePath) -> Result<VaultRead, VaultFileError> {
        let parents = self.parents(path, false)?;
        let absolute = parents.parent().info().final_path.join(&parents.leaf);
        let info = inspect_existing_path(&absolute, false)?;
        if info.is_reparse_point
            || info.is_directory
            || info.number_of_links != 1
            || !final_path_within(parents.parent().info(), &info)
        {
            return Err(VaultFileError::UnsafePath);
        }
        let held = open_held_for_read(&absolute)?;
        checked_read(&held)
    }
    /// Prevalidate the whole batch, publish with held parents and exclusive
    /// hash checks, compensate earlier effects if a later publication fails.
    pub fn apply(&self, changes: &[VaultChange]) -> Result<(), VaultFileError> {
        let mut before = vec![];
        let mut seen = std::collections::BTreeSet::new();
        for change in changes {
            if !seen.insert(change.path.as_str().to_lowercase()) {
                return Err(VaultFileError::UnsafePath);
            }
            let previous = match self.read(&change.path) {
                Ok(r) => Some(r),
                Err(VaultFileError::Io(e)) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            let actual = previous.as_ref().map(|r| r.hash.clone());
            if actual != change.expected_hash {
                return Err(VaultFileError::Conflict { current_hash: actual });
            }
            if change.bytes.as_ref().is_some_and(|b| b.len() as u64 > MAX_BYTES) {
                return Err(VaultFileError::UnsafePath);
            }
            before.push(previous.map(|r| r.bytes));
        }
        for (done, change) in changes.iter().enumerate() {
            if let Err(error) = self.apply_one(change) {
                for (change, old) in changes[..done].iter().zip(&before[..done]).rev() {
                    let expected_hash =
                        change.bytes.as_ref().map(|b| blake3::hash(b).to_hex().to_string());
                    if self
                        .apply_one(&VaultChange {
                            path: change.path.clone(),
                            expected_hash,
                            bytes: old.clone(),
                        })
                        .is_err()
                    {
                        return Err(VaultFileError::RecoveryRequired);
                    }
                }
                return Err(error);
            }
        }
        Ok(())
    }
    fn apply_one(&self, change: &VaultChange) -> Result<(), VaultFileError> {
        let parents = self.parents(&change.path, true)?;
        let parent = parents.parent();
        let target = parent.info().final_path.join(&parents.leaf);
        if let Some(bytes) = &change.bytes {
            let publication: HandleBoundPublishedFile =
                if let Some(expected) = &change.expected_hash {
                    let read = self.read(&change.path)?;
                    handle_bound_replace_checked(
                        parent,
                        OsStr::new(&parents.leaf),
                        &read.identity,
                        expected,
                        bytes,
                    )?
                } else {
                    handle_bound_create_new(parent, OsStr::new(&parents.leaf), bytes)?
                };
            publication.commit()?;
        } else {
            let held = open_held_for_move(&target)?;
            let read = checked_read(&held)?;
            if Some(&read.hash) != change.expected_hash.as_ref() {
                return Err(VaultFileError::Conflict { current_hash: Some(read.hash) });
            }
            // Bind the exact object, denying in-place writes during the final
            // condition check and deletion. No path-based remove_file follows.
            handle_bound_delete(held.file())?;
        }
        Ok(())
    }
    pub fn list_markdown(&self, max_files: usize) -> Result<Vec<WorkspacePath>, VaultFileError> {
        Ok(self.scan(max_files, false)?.into_iter().map(|entry| entry.path).collect())
    }
    pub fn list_all(&self, max_files: usize) -> Result<Vec<VaultEntry>, VaultFileError> {
        self.scan(max_files, true)
    }
    fn scan(&self, max_files: usize, all: bool) -> Result<Vec<VaultEntry>, VaultFileError> {
        if !self.exists()? {
            return Ok(vec![]);
        }
        let held = self.parents(&WorkspacePath::parse(".scan").expect("leaf"), false)?;
        let root = held.parent().info().final_path.clone();
        let mut pending = vec![(root, String::new())];
        let mut files = vec![];
        let mut visited = 0usize;
        while let Some((directory, prefix)) = pending.pop() {
            if inspect_existing_path(&directory, false)?.is_reparse_point {
                return Err(VaultFileError::UnsafePath);
            }
            let directory_held = open_verified_publish_parent(&directory)?;
            if !final_path_within(held.parent().info(), directory_held.info()) {
                return Err(VaultFileError::UnsafePath);
            }
            for entry in std::fs::read_dir(&directory_held.info().final_path)? {
                let entry = entry?;
                // Bound actual scan work, including empty directories and files
                // excluded from the Markdown index, rather than only matches.
                visited += 1;
                if visited > max_files {
                    return Err(VaultFileError::UnsafePath);
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if !all && (name.starts_with('.') || name.eq_ignore_ascii_case("README.md")) {
                    continue;
                }
                let rel = format!("{prefix}{name}");
                let path = WorkspacePath::parse(&rel).map_err(|_| VaultFileError::UnsafePath)?;
                let info = inspect_existing_path(&entry.path(), false)?;
                if info.is_reparse_point {
                    if all {
                        return Err(VaultFileError::UnsafePath);
                    }
                    if name.ends_with(".md") {
                        files.push(VaultEntry { path, directory: false });
                    }
                    continue;
                }
                if info.is_directory {
                    if all {
                        files.push(VaultEntry { path, directory: true });
                    }
                    pending.push((entry.path(), format!("{rel}/")));
                } else if all || name.to_ascii_lowercase().ends_with(".md") {
                    files.push(VaultEntry { path, directory: false });
                }
                if files.len() > max_files || pending.len() > max_files {
                    return Err(VaultFileError::UnsafePath);
                }
            }
        }
        files.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        Ok(files)
    }
}

fn reject_ancestor_links(root: &Path) -> Result<(), VaultFileError> {
    for path in root.ancestors() {
        match inspect_existing_path(path, false) {
            Ok(info) if info.is_reparse_point => return Err(VaultFileError::UnsafePath),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn checked_read(held: &VerifiedFile) -> Result<VaultRead, VaultFileError> {
    if held.info().is_reparse_point
        || held.info().is_directory
        || held.info().number_of_links != 1
        || held.file().metadata()?.len() > MAX_BYTES
    {
        return Err(VaultFileError::UnsafePath);
    }
    let mut bytes = vec![];
    held.file().take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(VaultFileError::UnsafePath);
    }
    Ok(VaultRead {
        hash: blake3::hash(&bytes).to_hex().to_string(),
        bytes,
        identity: held.info().identity.clone(),
    })
}
