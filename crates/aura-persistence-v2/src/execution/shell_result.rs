//! A Shell call's result as the model reads it ([`ShellResultV1`]), from the user's full
//! execution view ([`super::shell::query`]).
use super::shell::query;
use aura_domain_common::{SessionId, ToolCallId};
use aura_persistence_core::StoreResult;
use aura_windows_user_contract::{
    RunnerStopReason, SHELL_HEAD_READ_BYTES, SHELL_RESULT_HEAD_BYTES, SHELL_RESULT_TAIL_BYTES,
    ShellExecutionView, ShellQueryInput, ShellResultV1, shell_output_text, shell_stream_text_parts,
};
use rusqlite::Connection;

/// What shell.powershell, shell.cmd and shell.stop return: each stream whole, or its start and its
/// end with the bytes between named for shell.query.
pub fn model_result(
    db: &Connection,
    session: SessionId,
    call: ToolCallId,
) -> StoreResult<ShellResultV1> {
    let view = read(db, session, call, (0, 0), 1)?;
    let (stdout, stdout_next) = stream(db, session, call, view.stdout_total, true)?;
    let (stderr, stderr_next) = stream(db, session, call, view.stderr_total, false)?;
    Ok(result(&view, (stdout, stdout_next), (stderr, stderr_next)))
}

/// shell.query's page: the bytes it asked for, as text.
pub fn model_page(
    db: &Connection,
    session: SessionId,
    input: &ShellQueryInput,
) -> StoreResult<ShellResultV1> {
    let view = query(db, session, input)?;
    let stdout = (shell_output_text(&view.stdout), view.stdout_next);
    let stderr = (shell_output_text(&view.stderr), view.stderr_next);
    Ok(result(&view, stdout, stderr))
}

fn result(
    view: &ShellExecutionView,
    (stdout, stdout_next): (String, Option<u64>),
    (stderr, stderr_next): (String, Option<u64>),
) -> ShellResultV1 {
    let script = view.original_input.script.to_ascii_lowercase();
    ShellResultV1 {
        tool_call_id: view.tool_call_id,
        state: view.state,
        exit_code: view.exit_code,
        stop_reason: view.stop_reason.filter(|reason| *reason != RunnerStopReason::Exited),
        background: view.background,
        diagnostic: diagnostic(view),
        stdout,
        stderr,
        stdout_total: view.stdout_total,
        stderr_total: view.stderr_total,
        stdout_next,
        stderr_next,
        output_truncated: view.output_truncated,
        filtered_environment_names: view
            .filtered_environment_names
            .iter()
            .filter(|name| script.contains(&name.to_ascii_lowercase()))
            .cloned()
            .collect(),
    }
}

/// Why Aura stopped a command and what can be done next, before any diagnostic the run recorded.
/// A stopped command's kept output is in the result, so none of these is a failure to read.
fn diagnostic(view: &ShellExecutionView) -> Option<String> {
    let input = &view.original_input;
    let stopped = match view.stop_reason {
        Some(RunnerStopReason::OutputLimit) => format!(
            "Aura stopped the command when its output passed max_output_bytes ({} bytes); the output up to then is kept here. Write large output to a file and read the file, or run the command again with a larger max_output_bytes (at most 1048576).",
            input.max_output_bytes
        ),
        Some(RunnerStopReason::TimedOut) => format!(
            "Aura stopped the command after timeout_ms ({} ms); the output up to then is kept here. If it needs longer, run it again with a larger timeout_ms.",
            input.timeout_ms
        ),
        Some(RunnerStopReason::CoreDisconnected) => "Aura stopped the command because Aura's Core stopped while it ran; it may have done part of its work. Check what it changed before running it again.".to_owned(),
        Some(RunnerStopReason::Cancelled) => {
            "The command was stopped (by shell.stop or by the user) before it finished.".to_owned()
        }
        Some(RunnerStopReason::Exited | RunnerStopReason::Unknown) | None => {
            return view.diagnostic.clone();
        }
    };
    Some(match &view.diagnostic {
        Some(recorded) => format!("{stopped}\n{recorded}"),
        None => stopped,
    })
}

fn stream(
    db: &Connection,
    session: SessionId,
    call: ToolCallId,
    total: u64,
    stdout: bool,
) -> StoreResult<(String, Option<u64>)> {
    let pick = |view: ShellExecutionView| if stdout { view.stdout } else { view.stderr };
    let at = |offset: u64| if stdout { (offset, 0) } else { (0, offset) };
    if total <= (SHELL_RESULT_HEAD_BYTES + SHELL_RESULT_TAIL_BYTES) as u64 {
        let bytes = pick(read(db, session, call, at(0), total)?);
        return Ok((shell_output_text(&bytes), None));
    }
    let head = pick(read(db, session, call, at(0), SHELL_HEAD_READ_BYTES as u64)?);
    let tail_offset = total - SHELL_RESULT_TAIL_BYTES as u64;
    let tail = pick(read(db, session, call, at(tail_offset), SHELL_RESULT_TAIL_BYTES as u64)?);
    Ok(shell_stream_text_parts(&head, &tail, tail_offset))
}

fn read(
    db: &Connection,
    session: SessionId,
    call: ToolCallId,
    (stdout_offset, stderr_offset): (u64, u64),
    bytes: u64,
) -> StoreResult<ShellExecutionView> {
    query(
        db,
        session,
        &ShellQueryInput {
            tool_call_id: call,
            stdout_offset,
            stderr_offset,
            max_bytes: u32::try_from(bytes.clamp(1, 65_536)).unwrap_or(65_536),
        },
    )
}
