//! `--store-slim`: the execution store, then `aura.db`, each on its own. One line per step as it
//! goes (`{"step":"deleting","done","total"}`, `{"step":"rebuilding","kept"}`), then one line with
//! both reports (`{"report":{"execution":…,"legacy":…}}`). A store that is not there reports `null`;
//! one whose run failed reports `{"error":…}`, and the exit code is then 1. A launcher keeps the last
//! line that has a report, so one older than `aura.db`'s slimming logs both reports as well.
use std::{io::Write, ops::ControlFlow, path::Path};

use aura_persistence_v2::slimming::Step;
use serde_json::{Value, json};

fn part<R: serde::Serialize, E: std::fmt::Display>(
    run: Option<Result<R, E>>,
    failed: &mut bool,
) -> Value {
    match run {
        None => Value::Null,
        Some(Ok(report)) => serde_json::to_value(report).unwrap_or(Value::Null),
        Some(Err(error)) => {
            *failed = true;
            json!({ "error": error.to_string() })
        }
    }
}

pub(crate) fn run(
    root: &Path,
    free_space: &dyn Fn(&Path) -> std::io::Result<u64>,
    out: &mut dyn Write,
) -> u8 {
    let execution_root = root.join(crate::EXECUTION);
    let execution = execution_root.join("aura-v2.db").is_file().then(|| {
        aura_persistence_v2::slimming::slim(&execution_root, free_space, &mut |step| {
            let line = match step {
                Step::Deleting { done, total } => {
                    json!({ "step": "deleting", "done": done, "total": total })
                }
                Step::Rebuilding { kept } => json!({ "step": "rebuilding", "kept": kept }),
            };
            let _ = writeln!(out, "{line}");
            ControlFlow::Continue(())
        })
    });
    let legacy_database = root.join(crate::LEGACY);
    let legacy = legacy_database.is_file().then(|| {
        aura_persistence::slimming::slim(
            &legacy_database,
            &root.join(crate::LEGACY_ARTIFACTS),
            free_space,
            &mut |kept| {
                let _ = writeln!(out, "{}", json!({ "step": "rebuilding", "kept": kept }));
                ControlFlow::Continue(())
            },
        )
    });
    let mut failed = false;
    let report = json!({
        "execution": part(execution, &mut failed),
        "legacy": part(legacy, &mut failed),
    });
    let _ = writeln!(out, "{}", json!({ "report": report }));
    u8::from(failed)
}
