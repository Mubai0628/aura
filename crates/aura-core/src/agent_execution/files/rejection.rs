//! What the model is told when a file tool call is refused before it changes anything.
//!
//! Only recognized file-domain refusals are named. Transport, persistence and native faults stay
//! infrastructure failures, and tool output text is never classified.
//!
//! A write or patch that fails with no effect says why and what
//! to do next, as a read already did. The bare "do not retry an unknown effect" contradicted the
//! no-effect fact Core had recorded, and in evaluation samples it made the model give up on fs.patch.
use aura_domain_common::ErrorCategory;
use aura_fs_tools::FsToolError as F;
use aura_workspace::WorkspaceError as W;
use serde_json::{Value, json};

/// A long validation text or path must not flood the model's context.
const MAX_MESSAGE_CHARS: usize = 512;

struct Refusal {
    code: &'static str,
    category: ErrorCategory,
    cause: String,
    next: &'static str,
    retryable: bool,
}

impl Refusal {
    fn new(code: &'static str, category: ErrorCategory, cause: impl Into<String>) -> Self {
        Self { code, category, cause: cause.into(), next: "", retryable: false }
    }

    fn then(mut self, next: &'static str) -> Self {
        self.next = next;
        self
    }
}

/// The model-safe refusal for a file call that failed, when its cause is known.
///
/// `action` is the operation's action kind and `before` the facts recorded before it ran: the
/// path's kind tells a directory from a file. A write's message says "Nothing was changed", so the
/// caller records the refusal only when the call's side effect is `NoEffect`.
pub(super) fn rejection(action: &str, before: &Value, error: &anyhow::Error) -> Option<Value> {
    // The target changed between approval and start: nothing ran, and the call can be made again.
    if matches!(error.to_string().as_str(), "agent.source_or_scope_changed" | "agent.file_changed")
    {
        return Some(render(action, target_changed()));
    }
    // `fs.delete`'s own refusals are final: they say why nothing was deleted and what to
    // do next (the Recycle Bin could not take the item: ask the user before deleting for good).
    if action == "fs.delete" {
        let text = error.to_string();
        for (code, category) in [
            ("fs.delete.not_recyclable", ErrorCategory::Unavailable),
            ("fs.delete.link", ErrorCategory::Validation),
            ("fs.delete.replaced", ErrorCategory::Conflict),
            ("fs.delete.workspace_root", ErrorCategory::Validation),
        ] {
            if let Some(cause) = text.strip_prefix(code).and_then(|rest| rest.strip_prefix(": ")) {
                return Some(render(action, Refusal::new(code, category, cause)));
            }
        }
    }
    let mut found = None;
    for cause in error.chain() {
        if let Some(F::InvalidRequest(message)) = cause.downcast_ref::<F>() {
            // The file domain's own validation text is final.
            return Some(render(
                action,
                Refusal::new("fs.invalid_request", ErrorCategory::Validation, message.as_str()),
            ));
        }
        // Otherwise the innermost recognized cause wins: it is the most specific.
        let named = if let Some(file) = cause.downcast_ref::<F>() {
            file_refusal(file, action, before)
        } else if let Some(workspace) = cause.downcast_ref::<W>() {
            workspace_refusal(workspace, action, before)
        } else if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            io_refusal(io, action)
        } else {
            None
        };
        found = named.or(found);
    }
    found.map(|refusal| render(action, refusal))
}

/// The refusal for a read whose target does not exist.
pub(super) fn not_found() -> Value {
    render("fs.read", missing("fs.read"))
}

fn render(action: &str, refusal: Refusal) -> Value {
    let changed = matches!(action, "fs.write" | "fs.patch").then_some("Nothing was changed.");
    let message = [Some(refusal.cause.as_str()), changed, Some(refusal.next)]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_MESSAGE_CHARS)
        .collect::<String>();
    json!({"code":refusal.code, "category":refusal.category, "message":message,
        "retryable":refusal.retryable})
}

fn file_refusal(error: &F, action: &str, before: &Value) -> Option<Refusal> {
    use ErrorCategory::{Conflict, Validation};
    let conflict = |cause: String| Refusal::new("fs.expected_state_conflict", Conflict, cause);
    Some(match error {
        // `#[error(transparent)]` forwards past the inner error, so the chain never yields it.
        F::Workspace(inner) => return workspace_refusal(inner, action, before),
        F::Io { source, .. } => return io_refusal(source, action),
        F::ScopeUnavailable => scope_denied(),
        F::InvalidPatch(detail) => Refusal::new(
            "fs.invalid_patch",
            Validation,
            format!("The unified diff could not be applied: {detail}."),
        )
        .then(
            "Read the file again and send a corrected diff, or write the whole file with fs.write.",
        ),
        F::PatchTargetMismatch { path } => Refusal::new(
            "fs.invalid_patch",
            Validation,
            format!("The diff's file header names a different file than {path}."),
        )
        .then("Make the ---/+++ lines name the file being patched."),
        // Not "it changed": in an evaluation baseline the file was untouched and the model had copied
        // one character of the hash wrongly, then kept sending that copy after every re-read.
        // DeepSeek edited the file with six PowerShell commands, then wrote it
        // with the hash of its own earlier write; the message names that case too.
        F::ExpectedHash { path, .. } => conflict(format!(
            "{path} does not match the expected hash: the file changed after you last read or wrote \
             it (a shell command that edits it changes it too), or the hash was copied incorrectly."
        ))
        .then("Read the file again, copy its logical_hash exactly, and base the change on its current content."),
        F::ExpectedAbsent { path } => conflict(format!("{path} already exists."))
            .then("To replace it, read it and pass its hash as expected."),
        F::ExpectedDirectory { path } => conflict(format!("The directory {path} does not exist.")),
        F::ExternalExpectedState { .. } => {
            conflict("The file is not in the state this call expected.".into())
                .then("Read the file again and base the change on its current content.")
        }
        F::DeferredByEditorActivity => Refusal::new(
            "fs.deferred_by_editor_activity",
            Conflict,
            "The user is editing this file in Aura's editor.",
        )
        .then("Read the file again and re-issue the change."),
        F::ResourceBusy(_) => Refusal {
            retryable: true,
            ..Refusal::new(
                "fs.resource_busy",
                Conflict,
                "Another running tool call is using this file.",
            )
            .then("Try again after it finishes.")
        },
        F::LargeFileRangeRequired => {
            Refusal::new("fs.range_required", Validation, "The file is too large to read whole.")
                .then("Pass a lines or characters range.")
        }
        F::TextRangeTooLarge => Refusal::new(
            "fs.range_too_large",
            Validation,
            "The requested range is larger than the read limit.",
        )
        .then("Ask for a smaller range."),
        F::BinaryRangeTooLarge => Refusal::new(
            "fs.binary_range_too_large",
            Validation,
            "A binary range is limited to 64 KiB.",
        )
        .then("Ask for a smaller range."),
        // A staged write that could not take hold of a file another program has open carries the
        // operating system's code at the end of its text; no step of it applied.
        F::ChangesetFailed { applied_steps: 0, cause, .. } if in_use(cause) => file_in_use(),
        _ => return None,
    })
}

fn in_use(cause: &str) -> bool {
    [ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION]
        .iter()
        .any(|code| cause.ends_with(&format!("(os error {code})")))
}

fn target_changed() -> Refusal {
    Refusal {
        retryable: true,
        ..Refusal::new(
            "fs.target_changed",
            ErrorCategory::Conflict,
            "The target changed after this call was approved: another call of the same response, \
             or another program, changed it at the same moment.",
        )
        .then("Call it again if it is still needed.")
    }
}

fn file_in_use() -> Refusal {
    Refusal {
        retryable: true,
        ..Refusal::new(
            "fs.file_in_use",
            ErrorCategory::Conflict,
            "Another program has this file open and does not share it.",
        )
        .then("Try again after that program closes it.")
    }
}

fn workspace_refusal(error: &W, action: &str, before: &Value) -> Option<Refusal> {
    match error {
        W::ScopeUnavailable { .. } => Some(scope_denied()),
        W::NotFound { .. } => Some(missing(action)),
        W::WrongKind { .. } => Some(Refusal::new(
            "fs.wrong_kind",
            ErrorCategory::Validation,
            wrong_kind(action, before),
        )),
        W::Io { source, .. } | W::PathResolutionFailed { source, .. } => io_refusal(source, action),
        _ => None,
    }
}

/// Windows reports a file another program holds without sharing as these codes; the standard
/// library leaves them uncategorized.
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;

fn io_refusal(error: &std::io::Error, action: &str) -> Option<Refusal> {
    if matches!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)) {
        return Some(file_in_use());
    }
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => Some(Refusal::new(
            "fs.permission_denied",
            ErrorCategory::Permission,
            "File access was denied.",
        )),
        std::io::ErrorKind::NotFound => Some(missing(action)),
        _ => None,
    }
}

fn scope_denied() -> Refusal {
    Refusal::new(
        "fs.scope_denied",
        ErrorCategory::Permission,
        "The requested path is outside the authorized scope.",
    )
}

fn missing(action: &str) -> Refusal {
    let refusal =
        Refusal::new("fs.not_found", ErrorCategory::NotFound, "The requested file does not exist.");
    if action == "fs.patch" { refusal.then("Create it with fs.write.") } else { refusal }
}

/// The path exists but is the wrong kind of entry for this tool.
fn wrong_kind(action: &str, before: &Value) -> &'static str {
    let directory = before["paths"][0]["kind"] == "directory";
    match action {
        "fs.read" if directory => {
            "This path is a directory, not a file. Use fs.list to see its entries."
        }
        "fs.read" => "The target is not a readable file.",
        "fs.list" | "fs.find" | "fs.search" => {
            "This path is not a directory. Use fs.read to read a file."
        }
        _ if directory => "This path is a directory, so it cannot be written as a file.",
        _ => "A part of this path is a file, not a directory.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A write to a file another program holds (a log Start-Process is writing) says why, as a
    /// read of a file it holds without sharing already did.
    #[test]
    fn a_write_to_a_file_in_use_is_named_as_such() {
        let error = anyhow::Error::from(F::ChangesetFailed {
            failed_step: 0,
            applied_steps: 0,
            compensated_steps: 0,
            cause: r"failed to hold workspace object for the staged move \\?\C:\w\server.log: The process cannot access the file because it is being used by another process. (os error 32)".into(),
            journal_fault: String::new(),
        });
        let refusal = rejection("fs.write", &Value::Null, &error).expect("named");
        assert_eq!(refusal["code"], "fs.file_in_use");
        assert_eq!(refusal["retryable"], true);
        assert!(refusal["message"].as_str().unwrap_or("").contains("Nothing was changed."));
        let applied = anyhow::Error::from(F::ChangesetFailed {
            failed_step: 1,
            applied_steps: 1,
            compensated_steps: 0,
            cause: "(os error 32)".into(),
            journal_fault: String::new(),
        });
        assert!(rejection("fs.write", &Value::Null, &applied).is_none(), "a step had applied");
    }

    /// A target that changed between approval and start says so and that the call may be made
    /// again, not the generic "check the arguments".
    #[test]
    fn a_target_changed_since_approval_is_named_and_retryable() {
        for code in ["agent.source_or_scope_changed", "agent.file_changed"] {
            let refusal =
                rejection("fs.write", &Value::Null, &anyhow::anyhow!(code)).expect("named");
            assert_eq!(refusal["code"], "fs.target_changed");
            assert_eq!(refusal["retryable"], true);
            assert!(refusal["message"].as_str().unwrap_or("").contains("Call it again"));
        }
    }
}
