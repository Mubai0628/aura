//! Diagnostic sink for the workspace index.
//!
//! One incremental batch is decided by how many paths it actually re-read, how many
//! of those turned out to be unchanged, and how much of the batch was a duplicate of
//! a path it had already described. Those counts are the difference between "the
//! filesystem was slow" and "the index re-read and rewrote a directory that had not
//! changed", and they have no place in the value a caller receives. These lines go
//! to the owning persistence instance's log directory with their own component
//! tag, and they carry counts, identifiers and bounded
//! reasons only — never file content or a full path list.

use std::{fs, path::Path};

pub(crate) fn append_workspace_log(database_path: &Path, event: &str, fields: serde_json::Value) {
    let Some(database_dir) = database_path.parent() else {
        return;
    };
    // AuraPaths places the database in <local_root>/data/aura.db and logs in
    // <local_root>/logs. Standalone stores keep diagnostics under their own
    // database directory; an unknown layout never falls back to the user root.
    let local_root = if database_dir.file_name().is_some_and(|name| name == "data") {
        database_dir.parent().unwrap_or(database_dir)
    } else {
        database_dir
    };
    let path = local_root.join("logs").join("core.jsonl");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let entry = serde_json::json!({
        "timestamp": aura_domain_common::UtcTimestamp::now(),
        "component": "workspace-index",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_instances_keep_diagnostics_under_their_own_roots() {
        let root = tempfile::tempdir().expect("isolated diagnostic roots");
        let a = root.path().join("a");
        let b = root.path().join("b");
        let start = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for (database, owner) in [(a.join("data/aura.db"), "a"), (b.join("custom.db"), "b")] {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    for number in 0..32 {
                        append_workspace_log(
                            &database,
                            "concurrent-test",
                            serde_json::json!({"owner": owner, "number": number}),
                        );
                    }
                });
            }
        });
        for (directory, owner) in [(a, "a"), (b, "b")] {
            let text = fs::read_to_string(directory.join("logs/core.jsonl"))
                .expect("diagnostics belong to this instance");
            let entries = text
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSONL"))
                .collect::<Vec<_>>();
            assert_eq!(entries.len(), 32);
            assert!(entries.iter().all(|entry| entry["fields"]["owner"] == owner));
            assert!(entries.iter().all(|entry| entry["component"] == "workspace-index"));
        }
        assert!(!root.path().join("logs").exists(), "no shared parent fallback");
    }

    #[test]
    fn a_failed_diagnostic_sink_does_not_redirect_or_affect_another_instance() {
        let root = tempfile::tempdir().expect("isolated diagnostic roots");
        let broken = root.path().join("broken");
        fs::create_dir(&broken).expect("broken instance root");
        fs::write(broken.join("logs"), "a file blocks this log directory")
            .expect("unwritable diagnostic sink");
        append_workspace_log(&broken.join("data/aura.db"), "failed-sink", serde_json::json!({}));
        let healthy = root.path().join("healthy");
        append_workspace_log(&healthy.join("data/aura.db"), "healthy-sink", serde_json::json!({}));
        let text = fs::read_to_string(healthy.join("logs/core.jsonl"))
            .expect("the unrelated instance remains usable");
        let entry: serde_json::Value = serde_json::from_str(text.trim()).expect("valid JSON");
        assert_eq!(entry["event"], "healthy-sink");
        assert!(!text.contains("failed-sink"), "no fallback into another instance");
        assert!(!root.path().join("logs").exists(), "no shared parent fallback");
    }
}
