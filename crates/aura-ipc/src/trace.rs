//! Structured IPC tracing for desktop recovery investigations.
//!
//! Disabled unless `AURA_IPC_TRACE_FILE` names a writable path. Every component
//! that traces goes through this single writer, so lines from the Core and from
//! the IPC queue can never interleave in the same file. The trace records
//! identities, sizes, queue depths and sequence numbers only — never
//! credentials or full payloads — so a lost response can be followed from the
//! Core handler to the renderer bridge in one run.

use std::{
    fs::OpenOptions,
    io::Write as _,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static TRACE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

fn trace_file() -> Option<&'static Mutex<std::fs::File>> {
    TRACE
        .get_or_init(|| {
            let path = std::env::var("AURA_IPC_TRACE_FILE").ok()?;
            OpenOptions::new().create(true).append(true).open(path).ok().map(Mutex::new)
        })
        .as_ref()
}

/// Record one trace line. `fields` must stay credential-free. The whole line is
/// formatted before locking so concurrent writers can never interleave.
pub fn trace(stage: &str, fields: &str) {
    let Some(file) = trace_file() else {
        return;
    };
    let micros =
        SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_micros()).unwrap_or(0);
    let line = format!("{micros} {stage} {fields}\n");
    if let Ok(mut file) = file.lock() {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}
