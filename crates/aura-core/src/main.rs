#![forbid(unsafe_code)]

use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    if env::args().any(|argument| argument == "--version" || argument == "-V") {
        println!("aura-core {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // Before a newer version's first start the launcher asks this build which stores it would
    // migrate or write records into that the version it replaces (`--replacing`) refuses, and
    // copies only those. Nothing is opened for writing and Core does not start.
    let arguments: Vec<String> = env::args().collect();
    if let Some(at) = arguments.iter().position(|argument| argument == "--store-migration-check") {
        let Some(root) = arguments.get(at + 1).map(std::path::PathBuf::from) else {
            eprintln!("Aura Core argument error: --store-migration-check needs the data folder");
            return ExitCode::from(2);
        };
        let replacing = arguments
            .iter()
            .position(|argument| argument == "--replacing")
            .and_then(|at| arguments.get(at + 1))
            .map(String::as_str);
        let answer = |pending: Result<bool, String>| match pending {
            Ok(pending) => serde_json::json!(pending),
            Err(error) => serde_json::json!({ "error": error }),
        };
        let legacy = aura_persistence::store_migration_pending(&root.join("data").join("aura.db"))
            .map_err(|error| error.to_string());
        let execution = aura_persistence_v2::copy_before_start(
            &root.join("data").join("v2-execution").join("aura-v2.db"),
            replacing,
        )
        .map_err(|error| error.to_string());
        println!(
            "{}",
            serde_json::json!({
                "data/aura.db": answer(legacy),
                "data/v2-execution/aura-v2.db": answer(execution),
            })
        );
        return ExitCode::SUCCESS;
    }
    if env::args().any(|argument| argument == "--help" || argument == "-h") {
        println!(
            "Aura Core P01 Sidecar\n\nAura Core is started by Electron Main with a private bootstrap contract."
        );
        return ExitCode::SUCCESS;
    }
    match aura_core::CoreOptions::parse(env::args()) {
        Ok(options) => {
            // The acceptance boundary announces itself before the pipe exists, so the isolation
            // matrix can tell "this image carries the adapter" from "the hold never happened".
            // Compiled out of every product build.
            #[cfg(feature = "acceptance-barrier")]
            aura_core::acceptance_barrier::announce_startup();
            match aura_core::run_core(options).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("Aura Core failed: {error:#}");
                    ExitCode::from(1)
                }
            }
        }
        Err(error) => {
            eprintln!("Aura Core argument error: {error:#}");
            ExitCode::from(2)
        }
    }
}
