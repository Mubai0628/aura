//! Packs a release folder's payload into the update package and the setup program:
//!
//!   aura-release-pack <payload folder> <aura-setup.exe> <setup out> <update out>
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let arguments: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [payload, stub, setup, update] = arguments.as_slice() else {
        eprintln!(
            "usage: aura-release-pack <payload folder> <aura-setup.exe> <setup out> <update out>"
        );
        return ExitCode::from(2);
    };
    match aura_setup::pack::pack(payload, stub, setup, update) {
        Ok(packed) => {
            println!("{}", serde_json::to_string_pretty(&packed).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(1)
        }
    }
}
