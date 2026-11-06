//! The supervisor in the user's child session: `aura-desktop-host.exe --child <pipe> <token>`.
//!
//! Windows starts it when the child session signs in, from the one-time startup entry Core wrote
//! for this opening. It connects to Core's pipe, says hello with the opening's token, and from
//! then on runs the desktop host of the child session on Core's orders: the same program without
//! arguments, which speaks the desktop protocol on its standard input and output. A host that
//! ends is started again on Core's next order, so the user need not sign in again. When Core
//! closes the pipe, the supervisor ends its host and itself. A pipe that does not exist means no
//! Aura waits for this opening, such as a startup entry left over from one that went wrong: the
//! supervisor ends at once.
mod relay;
mod worker;

pub use relay::{Started, Workers, relay};
pub use worker::ProgramWorkers;

use crate::native::pipe::Pipe;
use aura_desktop_domain::child::{valid_child_pipe, valid_child_token};
use std::ffi::OsString;
use std::process::ExitCode;

/// Runs the supervisor with the arguments after `--child`.
#[must_use]
pub fn run(arguments: &[OsString]) -> ExitCode {
    let [pipe, token] = arguments else {
        return ExitCode::FAILURE;
    };
    let (Some(pipe), Some(token)) = (pipe.to_str(), token.to_str()) else {
        return ExitCode::FAILURE;
    };
    if !valid_child_pipe(pipe) || !valid_child_token(token) {
        return ExitCode::FAILURE;
    }
    let Ok(connection) = Pipe::connect(pipe) else {
        return ExitCode::SUCCESS;
    };
    let Ok(workers) = ProgramWorkers::this_program() else {
        return ExitCode::FAILURE;
    };
    match relay(connection.clone(), Box::new(connection), token, &workers) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
