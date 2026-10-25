//! The owner's four built-in Git Native exceptions; never a generic process permission.
use crate::{PermissionMode, ResolvedCapability as C, SecuritySourceRef};

#[must_use]
pub fn built_in_native_git(source: &SecuritySourceRef, action: &str, caps: &[C]) -> bool {
    if !matches!(source, SecuritySourceRef::BuiltInTool { .. }) {
        return false;
    }
    match action {
        "git.commit" | "git.branch" => caps == [C::GitWrite],
        "git.sync" => {
            caps.contains(&C::GitRemoteRead)
                && caps.iter().all(|cap| matches!(cap, C::GitRemoteRead | C::GitWrite))
        }
        "git.push" => {
            caps.contains(&C::GitRemoteWrite)
                && caps.iter().all(|cap| matches!(cap, C::GitRemoteWrite | C::GitDestructive))
        }
        _ => false,
    }
}

/// Used only after the exact built-in action/capability gate above has passed.
#[must_use]
pub fn native_git_mode_covers(mode: PermissionMode, cap: C) -> bool {
    match cap {
        C::GitWrite | C::GitRemoteRead => mode != PermissionMode::ReadOnly,
        C::GitRemoteWrite => mode == PermissionMode::FullAccess,
        _ => false,
    }
}
