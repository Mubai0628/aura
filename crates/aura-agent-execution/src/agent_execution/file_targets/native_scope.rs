use aura_windows_platform::{NativePathInfo, inspect_existing_path};

/// Both objects came from native inspection. Check the candidate's actual
/// ancestor object, so a case-sensitive sibling cannot become a descendant.
pub(crate) fn within(root: &NativePathInfo, candidate: &NativePathInfo) -> bool {
    if root.identity.volume_serial_number != candidate.identity.volume_serial_number {
        return false;
    }
    if root.identity == candidate.identity {
        return true;
    }
    if !root.is_directory {
        return false;
    }
    let depth = root.final_path.components().count();
    if candidate.final_path.components().count() <= depth {
        return false;
    }
    let ancestor = candidate.final_path.components().take(depth).collect::<std::path::PathBuf>();
    inspect_existing_path(&ancestor, true).is_ok_and(|info| info.identity == root.identity)
}
