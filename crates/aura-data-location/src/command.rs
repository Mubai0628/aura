//! `aura-core --data-locations --component-manifest=<path>`: the desktop asks this when it was
//! started without the launcher's arguments (a development run, an end-to-end test, `Aura.exe`
//! started by hand), so it still takes its locations from this crate. One JSON line on stdout:
//! `{"locations": …}` and exit 0, or `{"error": {"code", "message", "location"}}` and exit 3;
//! `location` is the data folder that is not there, else null.
use std::{io::Write, path::Path};

use aura_runtime_contract::BuildComponentManifest;
use serde_json::json;

use crate::{InstallCopy, LocationError, Locations, Roots};

pub const COMMAND: &str = "--data-locations";
const MANIFEST: &str = "--component-manifest=";

/// `None` when `arguments` are not this command.
pub fn command(arguments: &[String], out: &mut dyn Write) -> Option<u8> {
    if arguments.get(1).map(String::as_str) != Some(COMMAND) {
        return None;
    }
    let manifest = arguments.iter().skip(2).find_map(|argument| argument.strip_prefix(MANIFEST));
    let (line, code) = match manifest.ok_or_else(|| "missing --component-manifest".to_owned()) {
        Err(message) => (json!({ "error": { "code": "arguments", "message": message } }), 3),
        Ok(manifest) => match locations_for(Path::new(manifest)) {
            Ok(locations) => (json!({ "locations": locations }), 0),
            Err(Failure::Manifest(message)) => {
                (json!({ "error": { "code": "manifest", "message": message } }), 3)
            }
            Err(Failure::Location(error)) => (
                json!({ "error": {
                    "code": error.code(),
                    "message": error.to_string(),
                    "location": error.missing_location(),
                } }),
                3,
            ),
        },
    };
    let _ = writeln!(out, "{line}");
    Some(code)
}

enum Failure {
    Manifest(String),
    Location(LocationError),
}

fn locations_for(manifest: &Path) -> Result<Locations, Failure> {
    let bytes = std::fs::read(manifest).map_err(|error| Failure::Manifest(error.to_string()))?;
    let manifest_contents: BuildComponentManifest =
        serde_json::from_slice(&bytes).map_err(|error| Failure::Manifest(error.to_string()))?;
    let install = InstallCopy::for_manifest(manifest).map_err(Failure::Location)?;
    let roots = Roots::from_system().map_err(Failure::Location)?;
    Locations::resolve(&roots, &manifest_contents.bundle_namespace, &install)
        .map_err(Failure::Location)
}
