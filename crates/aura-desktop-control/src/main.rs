//! `aura-desktop-host`: started by Core when computer use is on; speaks the desktop protocol on
//! its standard input and output and ends when Core closes its input. With `--child <pipe>
//! <token>` it is the supervisor in the user's child session instead (`aura_desktop_pip::child`),
//! and with `--pip …` the window that shows the child session (`aura_desktop_pip::pip`).
//!
//! A program of the Windows subsystem: started from the child session's startup entry it opens no
//! console window; started by Core its standard input and output are the pipes Core hands it.
#![windows_subsystem = "windows"]
use aura_desktop_control::desktop::RealDesktop;
use aura_desktop_control::host::{Output, serve};
use aura_desktop_control::native::dpi;
use aura_desktop_domain::child::CHILD_SWITCH;
use aura_desktop_domain::pip::PIP_SWITCH;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some((first, rest)) = arguments.split_first() {
        return if first == CHILD_SWITCH {
            aura_desktop_pip::child::run(rest)
        } else if first == PIP_SWITCH {
            dpi::per_monitor_aware();
            aura_desktop_pip::pip::run(rest)
        } else {
            ExitCode::FAILURE
        };
    }
    dpi::per_monitor_aware();
    let output: Output = Arc::new(Mutex::new(std::io::stdout()));
    let mut desktop = match RealDesktop::new() {
        Ok(desktop) => desktop.with_output(Arc::clone(&output)),
        Err(error) => {
            eprintln!("aura-desktop-host: {error}");
            return ExitCode::FAILURE;
        }
    };
    match serve(&mut desktop, std::io::stdin().lock(), &output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("aura-desktop-host: {error}");
            ExitCode::FAILURE
        }
    }
}
