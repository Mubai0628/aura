//! What a Shell call returns to
//! the model. It used to be the whole execution view - the script three times (the model's input,
//! the normalized command and the submitted file with Aura's bootstrap), the binary's file binding,
//! every secret variable name, and stdout and stderr as arrays of byte numbers. The model now reads
//! the state, the exit code and the output as text; the full view stays the user's (`agent.shell.query`).
use super::*;

/// Bytes of a long stream's start the model reads.
pub const SHELL_RESULT_HEAD_BYTES: usize = 2 * 1024;
/// Bytes of a long stream's end the model reads: a failure or a summary is usually there.
pub const SHELL_RESULT_TAIL_BYTES: usize = 6 * 1024;
/// The head as read: a character's remaining bytes past the cut are seen, never shown.
pub const SHELL_HEAD_READ_BYTES: usize = SHELL_RESULT_HEAD_BYTES + 3;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShellResultV1 {
    /// This Shell call: the tool_call_id shell.query and shell.stop take.
    pub tool_call_id: aura_domain_common::ToolCallId,
    pub state: ShellCallState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    /// Why the process ended, when it did not exit by itself (a timeout, a stop, the output limit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<RunnerStopReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<BackgroundState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
    /// The output as text, terminal control sequences removed.
    pub stdout: String,
    pub stderr: String,
    /// Bytes kept of each stream.
    pub stdout_total: u64,
    pub stderr_total: u64,
    /// Where the part of a stream not shown starts: shell.query's stdout_offset / stderr_offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_next: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_next: Option<u64>,
    /// The process wrote more than max_output_bytes; the rest was not kept.
    #[serde(default, skip_serializing_if = "is_false")]
    pub output_truncated: bool,
    /// Secret variables removed from the environment that the script names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filtered_environment_names: Vec<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// One stream as the model reads it: the whole of a short one; the start and the end of a long one
/// with the bytes between named. Returns the text and where the part not shown starts.
#[must_use]
pub fn shell_stream_text(bytes: &[u8]) -> (String, Option<u64>) {
    if bytes.len() <= SHELL_RESULT_HEAD_BYTES + SHELL_RESULT_TAIL_BYTES {
        return (shell_output_text(bytes), None);
    }
    let tail_offset = bytes.len() - SHELL_RESULT_TAIL_BYTES;
    shell_stream_text_parts(
        &bytes[..SHELL_HEAD_READ_BYTES],
        &bytes[tail_offset..],
        tail_offset as u64,
    )
}

/// A long stream from its parts: `head` from byte 0 ([`SHELL_HEAD_READ_BYTES`] of it, so the cut
/// can see the character it falls in), `tail` from `tail_offset` to its end. Neither cut splits a
/// character.
#[must_use]
pub fn shell_stream_text_parts(
    head: &[u8],
    tail: &[u8],
    tail_offset: u64,
) -> (String, Option<u64>) {
    let head_end = boundary_back(head, head.len().min(SHELL_RESULT_HEAD_BYTES));
    let skip = boundary_forward(tail, 0);
    let tail_start = tail_offset + skip as u64;
    let text = format!(
        "{}
[… {} bytes not shown (bytes {head_end}–{tail_start}); read them with shell.query]
{}",
        shell_output_text(&head[..head_end]),
        tail_start - head_end as u64,
        shell_output_text(&tail[skip..]),
    );
    (text, Some(head_end as u64))
}

/// Output bytes as text: invalid UTF-8 replaced, ANSI/OSC sequences and other controls removed,
/// CRLF as LF, and a line rewritten with carriage returns (a progress bar) kept as it ended.
#[must_use]
pub fn shell_output_text(bytes: &[u8]) -> String {
    let raw = String::from_utf8_lossy(bytes);
    let mut plain = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek() {
                // CSI: parameters and intermediates, then one final byte.
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            break;
                        }
                    }
                }
                // OSC: up to BEL or ST (ESC \).
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{7}'
                            || (next == '\u{1b}' && chars.next_if_eq(&'\\').is_some())
                        {
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        if ch == '\n' || ch == '\r' || ch == '\t' || !ch.is_control() {
            plain.push(ch);
        }
    }
    plain
        .split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.rsplit('\r').next().unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn boundary_back(bytes: &[u8], mut at: usize) -> usize {
    while at > 0 && at < bytes.len() && bytes[at] & 0xC0 == 0x80 {
        at -= 1;
    }
    at
}

fn boundary_forward(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && bytes[at] & 0xC0 == 0x80 {
        at += 1;
    }
    at
}

#[cfg(test)]
#[path = "shell_result_tests.rs"]
mod tests;
