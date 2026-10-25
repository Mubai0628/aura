#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::{env, process::ExitCode};

use aura_windows_platform::{RecoveryAction, RecoveryChoice, show_recovery_dialog};

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
            let details = format!("{error:#}\n请重新运行 Aura 安装程序修复。");
            let _ = show_recovery_dialog(&RecoveryChoice {
                instruction: "Aura 无法启动",
                details: &details,
                actions: &[RecoveryAction::Exit],
                previous_version: None,
            });
            ExitCode::from(1)
        }
    }
}
