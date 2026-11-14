//! Structured IPC tracing for desktop recovery investigations.
//!
//! Only a debug build (development runs, tests and the diagnostics harness) follows
//! `AURA_IPC_TRACE_FILE`, and only when it names a writable path; a release build never writes
//! a trace, whatever its environment says. Every component
//! that traces goes through this single writer, so lines from the Core and from
//! the IPC queue can never interleave in the same file. The trace records
//! identities, sizes, queue depths and sequence numbers only — never
//! credentials or full payloads — so a lost response can be followed from the
//! Core handler to the renderer bridge in one run.

use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::Write as _,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static TRACE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

/// The file the trace goes to, given the variable's value and whether this is a debug build.
fn trace_path(variable: Option<OsString>, debug_build: bool) -> Option<PathBuf> {
    variable.filter(|_| debug_build).map(PathBuf::from)
}

fn trace_file() -> Option<&'static Mutex<std::fs::File>> {
    TRACE
        .get_or_init(|| {
            let path = trace_path(std::env::var_os("AURA_IPC_TRACE_FILE"), cfg!(debug_assertions))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_debug_build_follows_the_trace_variable() {
        let named = || Some(OsString::from(r"C:\trace\core-trace.log"));
        assert_eq!(trace_path(named(), true), Some(PathBuf::from(r"C:\trace\core-trace.log")));
        assert_eq!(trace_path(named(), false), None, "a release build never writes a trace");
        assert_eq!(trace_path(None, true), None);
    }
}
