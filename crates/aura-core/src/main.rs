#![forbid(unsafe_code)]

use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    if env::args().any(|argument| argument == "--version" || argument == "-V") {
        println!("aura-core {}", env!("CARGO_PKG_VERSION"));
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
