//! `aura-core --move-data --component-manifest=<path>`: does the move a development run asked
//! for, where no launcher runs; an installation's moves are the launcher's. One JSON line per
//! stage and per tenth of a stage while it goes, then `{"outcome": …}`; exit 0 when the data
//! moved or nothing was asked, 3 otherwise.
use std::{cell::Cell, io::Write, path::Path, sync::Mutex};

use aura_data_location::{InstallCopy, Locations, Roots};
use aura_runtime_contract::{BuildComponentManifest, PRODUCTION_BUNDLE_NAMESPACE};
use serde_json::json;

use crate::{Hooks, Outcome, Stage, perform, resume};

pub const COMMAND: &str = "--move-data";
const MANIFEST: &str = "--component-manifest=";

/// `None` when `arguments` are not this command.
pub fn command(
    arguments: &[String],
    out: &mut dyn Write,
    check_stores: &dyn Fn(&Path) -> Result<(), String>,
) -> Option<u8> {
    if arguments.get(1).map(String::as_str) != Some(COMMAND) {
        return None;
    }
    let out = Mutex::new(out);
    let say = |line: serde_json::Value| {
        if let Ok(mut out) = out.lock() {
            let _ = writeln!(out, "{line}");
        }
    };
    let refuse = |code: &str, message: String| {
        say(json!({ "error": { "code": code, "message": message } }));
        Some(3)
    };
    let Some(manifest) =
        arguments.iter().skip(2).find_map(|argument| argument.strip_prefix(MANIFEST))
    else {
        return refuse("arguments", "missing --component-manifest".to_owned());
    };
    let manifest = Path::new(manifest);
    let namespace =
        match std::fs::read(manifest).map_err(|error| error.to_string()).and_then(|bytes| {
            serde_json::from_slice::<BuildComponentManifest>(&bytes).map_err(|e| e.to_string())
        }) {
            Ok(contents) => contents.bundle_namespace,
            Err(message) => return refuse("manifest", message),
        };
    match InstallCopy::for_manifest(manifest) {
        Ok(InstallCopy::NotInstalled) => {}
        Ok(InstallCopy::Installed(_)) => {
            return refuse(
                "installed",
                "an installation's data is moved by its launcher".to_owned(),
            );
        }
        Err(error) => return refuse(error.code(), error.to_string()),
    }
    let roots = match Roots::from_system() {
        Ok(roots) => roots,
        Err(error) => return refuse(error.code(), error.to_string()),
    };
    let last = Cell::new((None::<Stage>, 0_u64));
    let progress = |stage: Stage, done: u64, total: u64| {
        let tenth = done.saturating_mul(10).checked_div(total).unwrap_or(0);
        if last.get() != (Some(stage), tenth) {
            last.set((Some(stage), tenth));
            say(json!({ "stage": format!("{stage:?}"), "done": done, "total": total }));
        }
    };
    let hooks = Hooks {
        progress: &progress,
        cancelled: &|| false,
        check_stores,
        install_copy: &|_| Ok(()),
        production: namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE,
        fault: None,
    };
    let resumed = resume(&roots, &namespace, &hooks);
    if resumed != Outcome::NothingAsked {
        say(json!({ "resumed": format!("{resumed:?}") }));
    }
    let current = match Locations::resolve(&roots, &namespace, &InstallCopy::NotInstalled)
        .and_then(|locations| locations.with_global_memory(&roots, &namespace))
    {
        Ok(current) => current,
        Err(error) => return refuse(error.code(), error.to_string()),
    };
    let outcome = perform(&roots, &namespace, &current, &hooks);
    let code = match &outcome {
        Outcome::Moved { .. } | Outcome::NothingAsked => 0,
        _ => 3,
    };
    say(json!({ "outcome": format!("{outcome:?}") }));
    Some(code)
}
