//! Diagnostic sink for failures that must not change the typed error a caller sees.
//!
//! Persistence failures are mapped to stable public codes, so the *step* that failed
//! has no place in the returned error. These lines go to the same per-run log file
//! Core writes (`%LOCALAPPDATA%/Aura/logs/core.jsonl`) with their own component tag,
//! and they carry identifiers, states and bounded reasons only — never secret
//! material, package bytes or user content.

use std::{env, fs, path::Path};

use aura_domain_common::UtcTimestamp;

pub(crate) fn append_persistence_log(event: &str, fields: serde_json::Value) {
    let Some(local) = env::var_os("LOCALAPPDATA") else {
        return;
    };
    let path = Path::new(&local).join("Aura").join("logs").join("core.jsonl");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let entry = serde_json::json!({
        "timestamp": UtcTimestamp::now(),
        "component": "persistence",
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
