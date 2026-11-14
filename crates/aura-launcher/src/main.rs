#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::{env, process::ExitCode};

use aura_launcher::recovery;

fn main() -> ExitCode {
    if env::args().any(|argument| argument == "--version" || argument == "-V") {
        println!("AuraLauncher {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match aura_launcher::run_launcher() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Aura Launcher failed: {error:#}");
            // A windowed launcher has no console: the reason is also shown in a window.
            let _ = recovery::ask(&recovery::fatal(&format!("{error:#}")));
            ExitCode::from(1)
        }
    }
}
