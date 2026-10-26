//! A stand-in for `Aura.exe` in the launcher's tests. It reads `fake-mode.txt` in its working
//! folder (the version folder the launcher starts it in):
//! - `ready` reports the release ready the way the desktop does and exits 0;
//! - `crash` exits 3;
//! - `crash-after-writing` writes its version into the data store first, as a migration would;
//! - `update-and-restart` lets the version named in `fake-next.txt` wait, as an applied update
//!   does, and asks the launcher to start again (exit 75);
//! - anything else exits 0 without reporting.
//!
//! Copied in as `aura-core.exe` it answers `--store-migration-check` with the
//! `fake-migration.json` beside it, as the new version's Core tells the launcher which stores
//! it would migrate.
use std::{env, fs, path::PathBuf, process::ExitCode};

use aura_runtime_contract::{ActiveInstallState, BuildComponentManifest};

fn data_root(manifest: &BuildComponentManifest) -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default())
        .join("Aura")
        .join("namespaces")
        .join(manifest.bundle_namespace.object_name_hash())
}

fn main() -> ExitCode {
    if env::args().nth(1).as_deref() == Some("--store-migration-check") {
        let answer = env::current_exe()
            .and_then(|program| fs::read_to_string(program.with_file_name("fake-migration.json")));
        return match answer {
            Ok(answer) => {
                print!("{answer}");
                ExitCode::SUCCESS
            }
            Err(_) => ExitCode::from(7),
        };
    }
    let mode = fs::read_to_string("fake-mode.txt").unwrap_or_default();
    let Ok(manifest) = fs::read("component-manifest.json")
        .map_err(|_| ())
        .and_then(|bytes| serde_json::from_slice::<BuildComponentManifest>(&bytes).map_err(|_| ()))
    else {
        return ExitCode::from(4);
    };
    match mode.trim() {
        "crash" => ExitCode::from(3),
        "crash-after-writing" => {
            let store = data_root(&manifest).join("data").join("aura.db");
            let written = fs::create_dir_all(store.parent().unwrap_or(&store)).and_then(|()| {
                fs::write(&store, format!("written by {}", manifest.product_version))
            });
            if written.is_err() {
                return ExitCode::from(6);
            }
            ExitCode::from(3)
        }
        "update-and-restart" => {
            let active = PathBuf::from("../../active.json");
            let next = fs::read_to_string("fake-next.txt").unwrap_or_default();
            let Ok(mut state) = fs::read(&active).map_err(|_| ()).and_then(|bytes| {
                serde_json::from_slice::<ActiveInstallState>(&bytes).map_err(|_| ())
            }) else {
                return ExitCode::from(5);
            };
            state.pending = Some(next.trim().to_owned());
            if fs::write(&active, serde_json::to_vec(&state).unwrap_or_default()).is_err() {
                return ExitCode::from(6);
            }
            ExitCode::from(75)
        }
        "ready" => {
            let launcher = data_root(&manifest).join("launcher");
            let ready = serde_json::json!({ "release_bundle_id": manifest.release_bundle_id });
            if fs::create_dir_all(&launcher).is_err()
                || fs::write(launcher.join("ready.v1.json"), ready.to_string()).is_err()
            {
                return ExitCode::from(6);
            }
            ExitCode::SUCCESS
        }
        _ => ExitCode::SUCCESS,
    }
}
