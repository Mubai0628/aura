#![forbid(unsafe_code)]

use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    // Answered without starting Core: the version and help, the launcher's questions about the
    // stores, a desktop's about where its data is, and the move of a development run's data.
    let (arguments, mut out) = (env::args().collect::<Vec<String>>(), std::io::stdout());
    let answered = aura_core_base::options::version_or_help(&arguments, &mut out)
        .or_else(|| aura_store_maintenance::command(&arguments, &mut out))
        .or_else(|| aura_data_location::command(&arguments, &mut out))
        .or_else(|| {
            aura_data_move::command(&arguments, &mut out, &aura_store_maintenance::check_stores)
        });
    if let Some(code) = answered {
        return ExitCode::from(code);
    }
    match aura_core::CoreOptions::parse(env::args()) {
        Ok(options) => {
            // The acceptance boundary announces itself before the pipe exists, so the isolation
            // matrix can tell "this image carries the adapter" from "the hold never happened".
            // Compiled out of every product build.
            #[cfg(feature = "acceptance-barrier")]
            aura_core_base::acceptance_barrier::announce_startup();
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
