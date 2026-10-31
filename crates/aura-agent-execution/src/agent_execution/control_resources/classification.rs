use crate::agent_execution::file_targets::native_scope::within as final_path_within;
use aura_windows_platform::{NativeFileAccess, NativePathInfo, open_verified_existing};
use std::{
    path::{Path, PathBuf},
    sync::RwLock,
};

/// Trusted runtime registrations. Names in a project never register authority.
#[derive(Default)]
pub struct ControlResources {
    pub(super) paths: RwLock<Vec<PathBuf>>,
    /// Aura's own data folders. Whatever a model
    /// reaches in them asks every time, like the user's credential stores.
    pub(super) data_roots: RwLock<Vec<PathBuf>>,
}
impl ControlResources {
    pub(super) fn register(&self, path: PathBuf) {
        let mut paths = self.paths.write().unwrap_or_else(|e| e.into_inner());
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    pub(super) fn register_data_root(&self, path: PathBuf) {
        let mut roots = self.data_roots.write().unwrap_or_else(|e| e.into_inner());
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    /// Aura's data folders that exist now, found where they really are.
    pub(in crate::agent_execution) fn data_folders(&self) -> Vec<NativePathInfo> {
        self.data_roots
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|root| aura_windows_platform::inspect_existing_path(root, true).ok())
            .filter(|root| root.is_directory)
            .collect()
    }
    /// Whether `path` - found where it really is, or where it would be created - lies in one of
    /// Aura's data folders.
    pub(in crate::agent_execution) fn in_aura_data(&self, path: &Path) -> bool {
        let Some(info) = path
            .ancestors()
            .find_map(|p| aura_windows_platform::inspect_existing_path(p, true).ok())
        else {
            return false;
        };
        in_folders(&self.data_folders(), &info)
    }
    /// Whether the user may view a file
    /// outside the workspace that the conversation names — not in Aura's data, nor in one of the
    /// user's credential stores.
    pub fn viewable_outside(&self, final_path: &Path) -> bool {
        use crate::agent_execution::protected_paths::{ExternalPathClass, classify};
        !self.in_aura_data(final_path) && classify(final_path) != ExternalPathClass::SensitiveUser
    }
    /// A recursive grant does not reach into Aura's data unless it was given inside it.
    pub(in crate::agent_execution) fn grant_escapes_into_data(
        &self,
        grant_root: &Path,
        path: &Path,
    ) -> bool {
        self.in_aura_data(path) && !self.in_aura_data(grant_root)
    }
    pub(in crate::agent_execution) fn contains(&self, info: &NativePathInfo) -> bool {
        self.overlaps(info, false)
    }
    pub(in crate::agent_execution) fn overlaps(
        &self,
        info: &NativePathInfo,
        mutating: bool,
    ) -> bool {
        self.paths.read().unwrap_or_else(|e| e.into_inner()).iter().any(|path| {
            match aura_windows_platform::inspect_existing_path(path, true) {
                Ok(root) => {
                    root.identity == info.identity
                        || root.is_directory && final_path_within(&root, info)
                        || mutating && info.is_directory && final_path_within(info, &root)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    same_entry(path, &info.final_path)
                }
                // An unreadable registered range is still registered. The
                // comparison checks native parent identities, never lowercase.
                Err(_) => same_entry(path, &info.final_path),
            }
        })
    }
    pub(in crate::agent_execution) fn contains_missing(&self, path: &Path) -> bool {
        self.paths.read().unwrap_or_else(|e| e.into_inner()).iter().any(|root| {
            same_entry(root, path)
                || aura_windows_platform::inspect_existing_path(root, true).ok().is_some_and(|r| {
                    r.is_directory
                        && path
                            .parent()
                            .and_then(|p| {
                                aura_windows_platform::inspect_existing_path(p, true).ok()
                            })
                            .is_some_and(|p| final_path_within(&r, &p))
                })
        })
    }
}
/// Whether `info` is one of `folders` or lies in one.
pub(in crate::agent_execution) fn in_folders(
    folders: &[NativePathInfo],
    info: &NativePathInfo,
) -> bool {
    folders.iter().any(|root| root.identity == info.identity || final_path_within(root, info))
}
fn same_entry(left: &Path, right: &Path) -> bool {
    let (Some(lp), Some(rp), Some(ln), Some(rn)) =
        (left.parent(), right.parent(), left.file_name(), right.file_name())
    else {
        return false;
    };
    let (Ok(parent), Ok(other)) = (
        open_verified_existing(lp, NativeFileAccess::Metadata),
        open_verified_existing(rp, NativeFileAccess::Metadata),
    ) else {
        return false;
    };
    parent.info().identity == other.info().identity
        && match aura_windows_platform::directory_case_rule(&parent) {
            aura_windows_platform::VolumeCaseRule::Sensitive => ln == rn,
            aura_windows_platform::VolumeCaseRule::InsensitivePreserving => {
                ln.to_str().zip(rn.to_str()).is_some_and(|(ln, rn)| {
                    aura_windows_platform::directory_name_comparison(&parent)
                        .is_some_and(|names| names.key(ln) == names.key(rn))
                })
            }
            aura_windows_platform::VolumeCaseRule::Unknown => false,
        }
}
