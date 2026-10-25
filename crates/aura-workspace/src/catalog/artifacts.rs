//! Private publication artifacts shared by scanning and native events.
use aura_domain_common::WorkspacePath;

/// Aura's own transient artifacts live inside the workspace. They are not user
/// content: a staging directory and a write temp file appear and disappear around
/// every mutation, and a buffer temp file around every save. Indexing them would
/// add churn to the user's own listing, invalidate directory versions and cursors
/// for changes the user never made, and make an incremental result depend on when
/// it was compared with a rebuild. Every scan and every event therefore skips
/// them, and an older index that recorded one drops it on the next removal.
pub(crate) fn is_internal_path(path: &WorkspacePath) -> bool {
    path.as_str().split('/').any(is_internal_component)
}

pub(super) fn is_internal_component(component: &str) -> bool {
    // The native publisher, download staging and an overwrite's parked object use 16 random
    // bytes; lookalike user filenames remain visible.
    let publisher_temp =
        [".aura-publish-", ".aura-download-", ".aura-backup-"].into_iter().any(|prefix| {
            component.strip_prefix(prefix).and_then(|name| name.strip_suffix(".tmp")).is_some_and(
                |nonce| nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()),
            )
        });
    // Buffer and direct file saves retain a ULID-named backup until their native commit.
    let save_backup = component
        .strip_prefix(".aura-buffer-")
        .or_else(|| component.strip_prefix(".aura-write-"))
        .and_then(|name| name.strip_suffix(".bak"))
        .is_some_and(|nonce| ulid::Ulid::from_string(nonce).is_ok());
    component.starts_with(".aura-transaction-")
        || (component.starts_with(".aura-write-") && component.ends_with(".tmp"))
        || (component.starts_with(".aura-buffer-") && component.ends_with(".tmp"))
        || publisher_temp
        || save_backup
}
