//! Refusals named by the code a file call's error text begins with: why a call was stopped
//! before it started, `fs.delete`'s own refusals, and the refusals of files outside the workspace
//! that the file tools word themselves.
use super::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_LOCK_VIOLATION, ERROR_PATH_NOT_FOUND,
    ERROR_SHARING_VIOLATION, Refusal, file_in_use, io_failed, missing, os_error, permission_denied,
    replaced, target_changed,
};
use aura_domain_common::ErrorCategory::{
    self, Cancelled, Conflict, Permission, Unavailable, Validation,
};

/// Codes that stop a call before it starts: the code the model reads, its category, whether the
/// same call may succeed later, why it stopped, and what to do.
const STOPPED: &[(&str, ErrorCategory, bool, &str, &str)] = &[
    (
        "agent.permission_withdrawn",
        Permission,
        true,
        "the session's permission mode changed before it started",
        "Call it again if it is still needed; it is checked against the new mode.",
    ),
    (
        "agent.cancelled",
        Cancelled,
        false,
        "the turn it belonged to was being stopped",
        "Call it again if it is still needed.",
    ),
    (
        "agent.not_running_batch",
        Cancelled,
        false,
        "the turn it belonged to was being stopped",
        "Call it again if it is still needed.",
    ),
    (
        "agent.call_already_finished",
        Cancelled,
        false,
        "the turn it belonged to was being stopped",
        "Call it again if it is still needed.",
    ),
    (
        "agent.directory_grant_not_in_force",
        Permission,
        false,
        "the access the user granted to that folder has ended",
        "Ask the user to grant it again if it is still needed.",
    ),
    (
        "agent.registry_changed",
        Conflict,
        true,
        "the tool list changed after this response was written",
        "Call it again.",
    ),
    (
        "agent.tool_disabled",
        Permission,
        false,
        "the user turned this tool off after it was called",
        "Do it another way, or ask the user to turn the tool back on.",
    ),
    (
        "agent.registry_authority_unavailable",
        Unavailable,
        true,
        "Aura could not confirm the tool's registration",
        "Try again in a moment.",
    ),
    (
        "agent.file_parent_not_directory",
        Validation,
        false,
        "part of the folder path is a file, not a folder",
        "Check the path with fs.list.",
    ),
    (
        "agent.file_parent_missing",
        Validation,
        false,
        "the path names no folder to hold the file",
        "Give the full path of the file, including its folder.",
    ),
    (
        "agent.file_leaf_missing",
        Validation,
        false,
        "the path does not end in a file or folder name",
        "Give the full path, including the name.",
    ),
    (
        "agent.native_name_unverified",
        Conflict,
        false,
        "Aura could not confirm the file's real name on disk",
        "Check the path's spelling with fs.list, then call it again.",
    ),
];

/// The refusal a code at the start of `text` names, for a call of `action`.
pub(super) fn refusal(action: &str, text: &str) -> Option<Refusal> {
    // The target changed between approval and start: nothing ran, and the call can be made again.
    if matches!(text, "agent.source_or_scope_changed" | "agent.file_changed") {
        return Some(target_changed());
    }
    // `fs.delete`'s own refusals are final: they say why nothing was deleted and what to do next
    // (the Recycle Bin could not take the item: ask the user before deleting for good).
    if action == "fs.delete" {
        for (code, category) in crate::agent_execution::deletion::REFUSALS {
            if let Some(cause) = text.strip_prefix(code).and_then(|rest| rest.strip_prefix(": ")) {
                return Some(Refusal::new(code, category, cause).says_effect());
            }
        }
    }
    let (code, rest) = text.split_once(':').map_or((text, ""), |(code, rest)| (code, rest.trim()));
    stopped(code, rest).or_else(|| outside(action, code, rest))
}

/// A call stopped before it started: what stopped it, and whether it can be called again.
fn stopped(code: &str, rest: &str) -> Option<Refusal> {
    let (code, category, retryable, why, next) = if code == "agent.scope_refused" {
        // The rest names how the call's task stands; only a live task may start new work.
        let (category, retryable, why, next) = match rest {
            "tool.scope.task_ended" => (
                Cancelled,
                false,
                "the task it belonged to had already ended",
                "Call it again if it is still needed.",
            ),
            "tool.scope.unavailable" => (
                Unavailable,
                true,
                "Aura could not confirm which task it belongs to",
                "Try again in a moment.",
            ),
            _ => (
                Conflict,
                false,
                "it does not belong to a running task of this conversation",
                "Call it again if it is still needed.",
            ),
        };
        ("agent.scope_refused", category, retryable, why, next)
    } else {
        *STOPPED.iter().find(|(known, ..)| *known == code)?
    };
    let refusal =
        Refusal::new(code, category, format!("This call was not started: {why}.")).then(next);
    Some(if retryable { refusal.retryable() } else { refusal })
}

/// A refusal the file tools word for a file or folder outside the workspace.
fn outside(action: &str, code: &str, rest: &str) -> Option<Refusal> {
    Some(match code {
        // A grant on a folder covers the items directly in it, not the contents of its
        // subfolders; the text names the folder the grant covers.
        "agent.external_scope_contents" => {
            let root = rest
                .split_once("the grant covers ")
                .and_then(|(_, tail)| tail.split_once(" and its direct children only"))
                .map(|(root, _)| root);
            let (cause, next) = match root {
                Some(root) => (
                    format!(
                        "The user granted access to {root} and the items directly in it, not to \
                         what is inside its subfolders."
                    ),
                    format!(
                        "Work with the items directly in {root}, or ask the user to grant access \
                         to this folder."
                    ),
                ),
                None => (
                    "The user granted access to a folder and the items directly in it, not to \
                     what is inside its subfolders."
                        .to_owned(),
                    "Work with the items directly in that folder, or ask the user to grant \
                     access to this one."
                        .to_owned(),
                ),
            };
            Refusal::new("fs.scope_denied", Permission, cause).then(next)
        }
        "agent.external_list_cursor_stale" => Refusal::new(
            "fs.cursor_stale",
            Conflict,
            "The cursor no longer fits this listing: the folder changed, or the cursor came from \
             another listing.",
        )
        .then("List it again with cursor set to null."),
        "agent.external_find_cursor_mismatch" => Refusal::new(
            "fs.cursor_stale",
            Conflict,
            "The cursor belongs to another search or another folder.",
        )
        .then("Search again with cursor set to null."),
        "agent.external_list_target_not_a_directory"
        | "agent.external_find_target_not_a_directory" => {
            Refusal::new("fs.wrong_kind", Validation, "The path is a file, not a folder.")
                .then("Use fs.read to read it.")
        }
        "agent.external_object_replaced" => replaced(
            "The file or folder at this path is no longer the one the user approved: it was \
             replaced or moved after the approval.",
        ),
        "agent.external_parent_replaced" => replaced(
            "The folder the file would be created in is no longer the one the user approved: it \
             was replaced or moved after the approval.",
        ),
        // The operating system's own text follows the code; its error code tells the cause.
        "agent.external_file_unavailable" => match os_error(rest)? {
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => missing(action, None),
            ERROR_ACCESS_DENIED => permission_denied(None),
            ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION => file_in_use(),
            _ => io_failed("Windows could not open the file", rest),
        },
        _ => return None,
    })
}
