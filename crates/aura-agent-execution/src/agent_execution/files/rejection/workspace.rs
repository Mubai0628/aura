//! Refusals the workspace layer reports for the file a call names, and for the workspace itself.
use super::{MAX_DETAIL_CHARS, Refusal, busy, detail, io_refusal, missing, scope_denied, shown};
use aura_domain_common::ErrorCategory::{Conflict, Unavailable, Validation};
use aura_workspace::WorkspaceError as W;
use serde_json::Value;

pub(super) fn refusal(error: &W, action: &str, before: &Value) -> Option<Refusal> {
    let reopen = "Ask the user to check the workspace and open it again.";
    let reread = "Read the file again and base the change on its current content.";
    Some(match error {
        W::ScopeUnavailable { path } => scope_denied(Some(path.to_string().as_str())),
        W::NotFound { path } => missing(action, Some(path.to_string().as_str())),
        W::WrongKind { path } => {
            let (cause, next) = wrong_kind(action, before, &path.to_string());
            Refusal::new("fs.wrong_kind", Validation, cause).then(next)
        }
        W::Io { action: doing, path, source } => {
            let path = shown(path);
            let failed = format!("Windows could not {doing} {path}");
            return io_refusal(source, action, Some(path), Some(failed));
        }
        W::PathResolutionFailed { path, source } => {
            let path = shown(path);
            let failed = format!("Windows could not open {path}");
            return io_refusal(source, action, Some(path), Some(failed));
        }
        W::EncodingAmbiguous { candidates, .. } => Refusal::new(
            "fs.encoding_ambiguous",
            Validation,
            format!("The file's text encoding could not be told ({}).", candidates.join(", ")),
        )
        .then(
            "If it is text, read it again with explicit_encoding set to one of these. If it is \
             not text (an archive, a program, a document), fs.read cannot show it: open it \
             with a program that reads that kind of file.",
        ),
        W::Offline => Refusal::new(
            "workspace.offline",
            Unavailable,
            "The workspace is offline, so Aura cannot reach its folder.",
        )
        .then("Ask the user to reopen the workspace (or reconnect its drive), then try again.")
        .retryable(),
        W::Archived => Refusal::new(
            "workspace.archived",
            Conflict,
            "The workspace is archived, so its files cannot be used.",
        )
        .then("Ask the user to restore it if it is still needed."),
        W::InvalidRoot => Refusal::new(
            "workspace.invalid_root",
            Conflict,
            "The workspace folder is missing or is no longer a folder.",
        )
        .then(reopen),
        W::RootIdentityChanged => Refusal::new(
            "workspace.root_identity_changed",
            Conflict,
            "The workspace folder was replaced or moved while the workspace was open.",
        )
        .then(reopen),
        W::Busy { reason } => Refusal::new(
            "workspace.busy",
            Conflict,
            format!("The workspace is busy: {}.", detail(reason, MAX_DETAIL_CHARS)),
        )
        .then("Try again in a moment."),
        W::LifecycleTransitionInProgress => Refusal::new(
            "workspace.lifecycle_in_progress",
            Conflict,
            "The workspace is being opened, closed or moved right now.",
        )
        .then("Try again in a moment.")
        .retryable(),
        W::EditorReserved { path, .. } => Refusal::new(
            "workspace.editor_reserved",
            Conflict,
            format!("{path} is being edited in Aura's editor in another conversation."),
        )
        .then("Try again after that edit is finished."),
        W::BufferRevisionConflict { .. } => Refusal::new(
            "fs.expected_state_conflict",
            Conflict,
            "The file's unsaved copy in Aura's editor changed after this call was planned.",
        )
        .then(reread),
        W::FileVersionConflict { path } => Refusal::new(
            "fs.expected_state_conflict",
            Conflict,
            format!("{path} changed on disk after this call was planned."),
        )
        .then(reread),
        W::FileResourceBusy { .. } => busy(),
        // No count of indexed entries: a read's refusal is compared between preparation and
        // start, and a count that moved would read as a changed target.
        W::IndexBuilding { .. } => Refusal::new(
            "workspace.catalog_unavailable",
            Unavailable,
            "The workspace index is still being built, so this search cannot run yet.",
        )
        .then("Try again in a moment, or look through folders one level at a time with fs.list.")
        .retryable(),
        W::CatalogUnavailable => Refusal::new(
            "workspace.catalog_unavailable",
            Unavailable,
            "The workspace index is not available right now.",
        )
        .then("Try again in a moment, or look through folders with fs.list.")
        .retryable(),
        _ => return None,
    })
}

/// The path exists but is the wrong kind of entry for this tool.
fn wrong_kind(action: &str, before: &Value, path: &str) -> (String, &'static str) {
    let directory = before["paths"][0]["kind"] == "directory";
    match action {
        "fs.read" if directory => {
            (format!("{path} is a directory, not a file."), "Use fs.list to see its entries.")
        }
        "fs.read" => (
            format!("{path} is not a regular file."),
            "Use fs.list on its folder to see what it is.",
        ),
        "fs.list" | "fs.find" | "fs.search" => {
            (format!("{path} is not a directory."), "Use fs.read to read a file.")
        }
        _ if directory => (
            format!("{path} is a directory, so it cannot be written as a file."),
            "Give the path of a file inside it.",
        ),
        _ => (
            format!("A part of {path} is a file, not a directory."),
            "Check each folder on the path with fs.list.",
        ),
    }
}
