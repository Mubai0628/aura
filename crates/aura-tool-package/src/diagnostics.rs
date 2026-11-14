//! Diagnostic sink for the immutable Package Store.
//!
//! Publish failures are mapped to stable typed errors, so which branch ran and what
//! the store looked like at that moment has no place in the returned error. These
//! lines go to the same log file Core writes (the one routed through
//! `aura_data_location`; none without a route, as in every test) with their own
//! component tag, and carry paths, states and bounded reasons only — never package
//! bytes or user content.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn append_store_log(event: &str, fields: serde_json::Value) {
    let Some(path) = store_log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let entry = serde_json::json!({
        "timestamp": aura_domain_common::UtcTimestamp::now(),
        "component": "tool-package-store",
        "event": event,
        "fields": fields,
    });
    if let Ok(mut bytes) = serde_json::to_vec(&entry) {
        bytes.push(b'\n');
        use std::io::Write;
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(&bytes);
        }
    }
}

/// Core's log file, which these lines share. A test build routes none: tests publish into stores
/// of their own and must never write into the user's real profile.
fn store_log_path() -> Option<PathBuf> {
    aura_data_location::core_log_path().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_build_never_writes_to_the_users_core_log() {
        assert_eq!(super::store_log_path(), None, "a test wrote into the user's real core.jsonl");
    }
}
