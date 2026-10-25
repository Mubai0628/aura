//! Diagnostic sink for the immutable Package Store.
//!
//! Publish failures are mapped to stable typed errors, so which branch ran and what
//! the store looked like at that moment has no place in the returned error. These
//! lines go to the same per-run log file Core writes
//! (`%LOCALAPPDATA%/Aura/logs/core.jsonl`) with their own component tag, and carry
//! paths, states and bounded reasons only — never package bytes or user content.

use std::{env, fs, path::Path};

pub(crate) fn append_store_log(event: &str, fields: serde_json::Value) {
    let Some(local) = env::var_os("LOCALAPPDATA") else {
        return;
    };
    let path = Path::new(&local).join("Aura").join("logs").join("core.jsonl");
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
