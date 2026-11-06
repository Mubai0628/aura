//! The pipe between Core and the supervisor that Aura's desktop host runs as in the user's child
//! session (`aura-desktop-host.exe --child <pipe> <token>`). The supervisor connects to Core's
//! pipe, says [`SupervisorSays::Hello`] first, and runs the desktop host of the child session on
//! Core's orders, passing its lines through. Each line on the pipe starts with one byte saying
//! whose it is: [`HOST_LINE`] for a line of the desktop protocol, to or from that host;
//! [`SUPERVISOR_LINE`] for the supervisor's own JSON.
use crate::MAX_LINE_BYTES;
use serde::{Deserialize, Serialize};

/// The first byte of a desktop protocol line on the pipe.
pub const HOST_LINE: u8 = b'H';
/// The first byte of a supervisor's line on the pipe.
pub const SUPERVISOR_LINE: u8 = b'S';
/// The protocol both ends speak; the supervisor says it in its hello, and Core drops a
/// supervisor that speaks another.
pub const CHILD_PROTOCOL_VERSION: u32 = 1;
/// The longest line on the pipe: the longest desktop protocol line and its first byte.
pub const MAX_CHILD_LINE_BYTES: usize = MAX_LINE_BYTES + 1;
/// The command line switch that runs the desktop host as the child session's supervisor.
pub const CHILD_SWITCH: &str = "--child";
/// The start of every child session pipe's name; each opening adds a part of its own.
pub const CHILD_PIPE_PREFIX: &str = r"\\.\pipe\aura-desktop-child-";

/// A pipe name Core makes: the prefix, then 16 to 64 lowercase hexadecimal digits.
#[must_use]
pub fn valid_child_pipe(name: &str) -> bool {
    name.strip_prefix(CHILD_PIPE_PREFIX).is_some_and(|part| lower_hex(part, 16..=64))
}

/// A token Core makes: 32 to 128 lowercase hexadecimal digits.
#[must_use]
pub fn valid_child_token(token: &str) -> bool {
    lower_hex(token, 32..=128)
}

fn lower_hex(part: &str, lengths: std::ops::RangeInclusive<usize>) -> bool {
    lengths.contains(&part.len())
        && part.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// What the supervisor says.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "says", rename_all = "snake_case", deny_unknown_fields)]
pub enum SupervisorSays {
    /// First and once: the token of the opening it was started for, from its command line.
    Hello { token: String, protocol: u32 },
    /// The host started; its lines follow.
    Started,
    /// The host could not be started.
    NotStarted { why: String },
    /// The host ended; a later [`SupervisorOrder::Start`] starts another.
    Ended,
}

/// What Core asks of the supervisor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "order", rename_all = "snake_case", deny_unknown_fields)]
pub enum SupervisorOrder {
    /// Starts the host. One that still runs is ended first: its [`SupervisorSays::Ended`] comes
    /// before the new one's [`SupervisorSays::Started`], and lines before that are the old one's.
    Start,
    /// Ends the host, when one runs.
    Stop,
}

/// One line of the pipe, without its line break.
#[derive(Debug, Eq, PartialEq)]
pub enum ChildLine<'a, T> {
    /// A desktop protocol line, as the host or Core wrote it.
    Host(&'a [u8]),
    /// A supervisor's line: what it says, or what it is told.
    Supervisor(T),
}

/// The pipe line of a desktop protocol line: its first byte, then the line.
#[must_use]
pub fn host_line(line: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(line.len() + 2);
    framed.push(HOST_LINE);
    framed.extend_from_slice(line);
    framed.push(b'\n');
    framed
}

/// The pipe line of what the supervisor says or is told.
#[must_use]
pub fn supervisor_line(message: &impl Serialize) -> Vec<u8> {
    let mut framed = vec![SUPERVISOR_LINE];
    // Both message types serialize to plain JSON objects, which never fails.
    framed.extend(serde_json::to_vec(message).unwrap_or_default());
    framed.push(b'\n');
    framed
}

/// Reads one pipe line, without its line break: a desktop protocol line, or a supervisor's line
/// of type `T`.
///
/// # Errors
///
/// An empty line, a first byte that is neither kind's, or a supervisor's line that is not a `T`.
pub fn read_child_line<'a, T: Deserialize<'a>>(line: &'a [u8]) -> Result<ChildLine<'a, T>, String> {
    match line.split_first() {
        Some((&HOST_LINE, rest)) => Ok(ChildLine::Host(rest)),
        Some((&SUPERVISOR_LINE, rest)) => serde_json::from_slice(rest)
            .map(ChildLine::Supervisor)
            .map_err(|error| format!("不是看守的一行：{error}")),
        Some((first, _)) => Err(format!("这一行的开头 {first:#04x} 不对")),
        None => Err("空行".to_owned()),
    }
}
