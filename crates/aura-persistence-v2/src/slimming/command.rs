//! A run as the launcher asks for it (`aura-core --store-slim <data folder>`), before the desktop
//! starts: one JSON object per line, each step as it goes and then the report or the error.
use std::{io::Write, ops::ControlFlow, path::Path};

use serde_json::json;

use super::{Step, slim};

/// Runs on the execution store under `data_root`, writing the lines to `out`; the exit code.
pub fn command(
    data_root: &Path,
    free_space: &dyn Fn(&Path) -> std::io::Result<u64>,
    out: &mut dyn Write,
) -> u8 {
    let store = data_root.join("data").join("v2-execution");
    if !store.join(crate::DATABASE).is_file() {
        let _ = writeln!(out, "{}", json!({ "report": null }));
        return 0;
    }
    let slimmed = slim(&store, free_space, &mut |step| {
        let line = match step {
            Step::Deleting { done, total } => {
                json!({ "step": "deleting", "done": done, "total": total })
            }
            Step::Rebuilding { kept } => json!({ "step": "rebuilding", "kept": kept }),
        };
        let _ = writeln!(out, "{line}");
        ControlFlow::Continue(())
    });
    match slimmed {
        Ok(report) => {
            let _ = writeln!(out, "{}", json!({ "report": report }));
            0
        }
        Err(error) => {
            let _ = writeln!(out, "{}", json!({ "error": error.to_string() }));
            1
        }
    }
}
