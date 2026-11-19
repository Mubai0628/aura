//! What the model is told when a file tool call is refused before it changes anything.
//!
//! Only recognized refusals are named: a cause the file, workspace or operating-system layer
//! reports for the file the call names, a reason the call was stopped before it started, and the
//! refusals of outside files that the file tools word themselves. Transport and persistence faults
//! stay infrastructure failures, and tool output text is never classified.
//!
//! A write or patch that fails with no effect says why and what
//! to do next, as a read already did. The bare "do not retry an unknown effect" contradicted the
//! no-effect fact Core had recorded, and in evaluation samples it made the model give up on fs.patch.
use aura_domain_common::ErrorCategory;
use aura_fs_tools::FsToolError as F;
use aura_workspace::WorkspaceError as W;
use serde_json::{Value, json};

mod coded;
mod workspace;

/// A long validation text or path must not flood the model's context.
const MAX_MESSAGE_CHARS: usize = 512;
/// How much of another layer's own text (the operating system's, a library's) a message quotes.
const MAX_DETAIL_CHARS: usize = 200;

/// Windows reports a file another program holds without sharing as these codes; the standard
/// library leaves them uncategorized.
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;
/// Codes an outside file's open failure carries only in its text.
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PATH_NOT_FOUND: i32 = 3;
const ERROR_ACCESS_DENIED: i32 = 5;

struct Refusal {
    code: &'static str,
    category: ErrorCategory,
    cause: String,
    next: String,
    retryable: bool,
    /// The cause already says, in its own words, that the call left everything as it was.
    says_effect: bool,
}

impl Refusal {
    fn new(code: &'static str, category: ErrorCategory, cause: impl Into<String>) -> Self {
        Self {
            code,
            category,
            cause: cause.into(),
            next: String::new(),
            retryable: false,
            says_effect: false,
        }
    }

    fn then(mut self, next: impl Into<String>) -> Self {
        self.next = next.into();
        self
    }

    fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    fn says_effect(mut self) -> Self {
        self.says_effect = true;
        self
    }
}

/// The model-safe refusal for a file call that failed, when its cause is known.
///
/// `action` is the operation's action kind and `before` the facts recorded before it ran: the
/// path's kind tells a directory from a file. A write's message says "Nothing was changed", so the
/// caller records the refusal only when the call's side effect is `NoEffect`.
pub(super) fn rejection(action: &str, before: &Value, error: &anyhow::Error) -> Option<Value> {
    // A call stopped before it started, `fs.delete`'s own refusals and the outside-file refusals
    // are named by the code their text begins with.
    if let Some(refusal) = coded::refusal(action, &error.to_string()) {
        return Some(render(action, refusal));
    }
    let mut found = None;
    let mut read_io = false;
    for (depth, cause) in error.chain().enumerate() {
        if let Some(F::InvalidRequest(message)) = cause.downcast_ref::<F>() {
            // The file domain's own validation text is final.
            return Some(render(action, invalid_request(message)));
        }
        // Otherwise the innermost recognized cause wins: it is the most specific. An
        // operating-system error a file or workspace error carries was read there already, with
        // its path and what was being done.
        let named = if let Some(file) = cause.downcast_ref::<F>() {
            file_refusal(file, action, before)
        } else if let Some(inner) = cause.downcast_ref::<W>() {
            workspace::refusal(inner, action, before)
        } else if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            // An error the call's own file operation returned directly is named whatever its
            // kind; one found deeper belongs to a layer this does not speak for.
            let path = named_path(before);
            let failed = (depth == 0).then(|| match &path {
                Some(path) => format!("Windows reported an error for {path}"),
                None => "Windows reported an error".to_owned(),
            });
            if read_io { None } else { io_refusal(io, action, path, failed) }
        } else {
            None
        };
        read_io =
            named.is_some() && cause.source().is_some_and(|source| source.is::<std::io::Error>());
        found = named.or(found);
    }
    found.map(|refusal| render(action, refusal))
}

/// The refusal for a read whose target does not exist.
/// A read whose path was found missing when its facts were taken: the path, and what to do.
pub(super) fn not_found(action: &str, facts: &Value) -> Value {
    let absent = facts["paths"].as_array().and_then(|paths| {
        paths.iter().find(|path| path["absent"].as_bool() == Some(true))?["path"].as_str()
    });
    render(action, missing(action, absent))
}

fn render(action: &str, refusal: Refusal) -> Value {
    let left = if refusal.says_effect { None } else { effect(action) };
    let tail = [left.unwrap_or_default(), refusal.next.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    // The cause gives way first, so what the call changed and what to do next are always read.
    let room = MAX_MESSAGE_CHARS.saturating_sub(tail.chars().count() + 1);
    let message = [bounded(&refusal.cause, room), tail]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    json!({"code":refusal.code, "category":refusal.category, "message":message,
        "retryable":refusal.retryable})
}

/// What a refused call of `action` left as it was. A read changes nothing, so it says nothing.
fn effect(action: &str) -> Option<&'static str> {
    if action == "fs.delete" {
        Some("Nothing was deleted.")
    } else if super::super::workspace_init::writes_files(action) {
        Some("Nothing was changed.")
    } else {
        None
    }
}

fn file_refusal(error: &F, action: &str, before: &Value) -> Option<Refusal> {
    use ErrorCategory::{Cancelled, Conflict, Internal, Validation};
    let conflict = |cause: String| Refusal::new("fs.expected_state_conflict", Conflict, cause);
    Some(match error {
        // `#[error(transparent)]` forwards past the inner error, so the chain never yields it.
        F::Workspace(inner) => return workspace::refusal(inner, action, before),
        F::Io { action: doing, path, source } => {
            let path = shown(path);
            let failed = format!("Windows could not {doing} {path}");
            return io_refusal(source, action, Some(path), Some(failed));
        }
        F::ScopeUnavailable => scope_denied(named_path(before).as_deref()),
        F::Document(line) => {
            let (code, message) = line.split_once(": ")?;
            let category = if code == "fs.pdf_text_unavailable" {
                ErrorCategory::Compatibility
            } else {
                Validation
            };
            Refusal::new(code, category, message)
        }
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
        F::ExpectedDirectory { path } => {
            conflict(format!("{path} is not an existing folder, but this call expected one."))
                .then("Check the path with fs.list, and set expected to what is there now.")
        }
        F::ExternalExpectedState { path, .. } => {
            conflict(format!("{} is not in the state this call expected.", shown(path)))
                .then("Read the file again and base the change on its current content.")
        }
        F::ExternalFileReplaced { path } => replaced(format!(
            "{} is no longer the file the user approved: it was replaced or moved after the \
             approval.",
            shown(path)
        )),
        F::DeferredByEditorActivity => Refusal::new(
            "fs.deferred_by_editor_activity",
            Conflict,
            "The user is editing this file in Aura's editor.",
        )
        .then("Read the file again and re-issue the change."),
        F::ResourceBusy(_) => busy(),
        F::LargeFileRangeRequired => {
            Refusal::new("fs.range_required", Validation, "The file is too large to read whole.")
                .then("Pass a lines or characters range.")
        }
        F::TextRangeTooLarge { line_limit, character_limit } => Refusal::new(
            "fs.range_too_large",
            Validation,
            format!(
                "A range reads at most {} lines or {} characters at a time.",
                grouped(*line_limit),
                grouped(*character_limit)
            ),
        )
        .then("Read a longer part in several ranges."),
        F::BinaryRangeTooLarge => Refusal::new(
            "fs.binary_range_too_large",
            Validation,
            "A binary range is limited to 64 KiB.",
        )
        .then("Ask for a smaller range."),
        F::Cancelled => Refusal::new(
            "fs.cancelled",
            Cancelled,
            "This call was stopped before it finished: Aura was asked to stop it.",
        )
        .then("Call it again if it is still needed."),
        F::Internal(detail) => Refusal::new(
            "fs.internal",
            Internal,
            format!(
                "The file tool stopped on an internal error in Aura: {}.",
                self::detail(detail, MAX_DETAIL_CHARS)
            ),
        )
        .then("Try again; if it fails again, tell the user."),
        // A staged write that could not take hold of its file carries the operating system's
        // code at the end of its text; no step of it applied.
        F::ChangesetFailed { applied_steps: 0, cause, .. } => match os_error(cause) {
            Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION) => file_in_use(),
            Some(ERROR_ACCESS_DENIED) => permission_denied(named_path(before).as_deref()),
            _ => Refusal::new(
                "fs.changeset_failed",
                Conflict,
                format!("The change could not be made: {}.", detail(cause, MAX_DETAIL_CHARS)),
            )
            .then("Read the file again, then try again."),
        },
        _ => return None,
    })
}

/// The refusal for an operating-system error. `path` is where it happened, when known; `failed`
/// says what Windows could not do there, which names an error of a less common kind as well.
fn io_refusal(
    error: &std::io::Error,
    action: &str,
    path: Option<String>,
    failed: Option<String>,
) -> Option<Refusal> {
    if matches!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)) {
        return Some(file_in_use());
    }
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => Some(permission_denied(path.as_deref())),
        std::io::ErrorKind::NotFound => Some(missing(action, path.as_deref())),
        _ => failed.map(|failed| io_failed(&failed, &error.to_string())),
    }
}

fn io_failed(failed: &str, text: &str) -> Refusal {
    Refusal::new(
        "fs.io_failed",
        ErrorCategory::Unavailable,
        format!("{failed}: {}.", detail(text, MAX_DETAIL_CHARS)),
    )
    .then("Check the path; if it is right, try again.")
}

fn target_changed() -> Refusal {
    Refusal::new(
        "fs.target_changed",
        ErrorCategory::Conflict,
        "The target changed after this call was approved: another call of the same response, \
         or another program, changed it at the same moment.",
    )
    .then("Call it again if it is still needed.")
    .retryable()
}

fn file_in_use() -> Refusal {
    Refusal::new(
        "fs.file_in_use",
        ErrorCategory::Conflict,
        "Another program has this file open and does not share it.",
    )
    .then("Try again after that program closes it.")
    .retryable()
}

fn busy() -> Refusal {
    Refusal::new(
        "fs.resource_busy",
        ErrorCategory::Conflict,
        "Another running tool call is using this file.",
    )
    .then("Try again after it finishes.")
    .retryable()
}

/// An outside file or folder that is no longer the one the user approved.
fn replaced(cause: impl Into<String>) -> Refusal {
    Refusal::new("fs.external_file_replaced", ErrorCategory::Conflict, cause).then(
        "Check the path, then call again if it is still needed; the user may be asked to \
         approve it again.",
    )
}

fn scope_denied(path: Option<&str>) -> Refusal {
    let cause = path.map_or_else(
        || "The requested path leads outside the workspace.".to_owned(),
        |path| format!("{path} leads outside the workspace."),
    );
    Refusal::new("fs.scope_denied", ErrorCategory::Permission, cause)
        .then("Use a path inside the workspace, or ask the user to grant access to that location.")
}

fn permission_denied(path: Option<&str>) -> Refusal {
    let cause = path.map_or_else(
        || "Windows denied access to this file or folder.".to_owned(),
        |path| format!("Windows denied access to {path}."),
    );
    Refusal::new("fs.permission_denied", ErrorCategory::Permission, cause).then(
        "It may be read-only, protected or need rights Aura does not have; ask the user to \
         check it if it is needed.",
    )
}

fn missing(action: &str, path: Option<&str>) -> Refusal {
    let cause = match (action, path) {
        ("fs.write", Some(path)) => format!("{path}, or a folder on its path, does not exist."),
        ("fs.write", None) => "The path, or a folder on it, does not exist.".to_owned(),
        (_, Some(path)) => format!("{path} does not exist."),
        ("fs.list" | "fs.find" | "fs.search", None) => {
            "The requested folder does not exist.".to_owned()
        }
        (_, None) => "The requested file does not exist.".to_owned(),
    };
    let next = match action {
        "fs.read" => "Use fs.find or fs.list to locate it.",
        "fs.patch" => "Create it with fs.write, or find the right path with fs.find or fs.list.",
        "fs.list" | "fs.find" | "fs.search" => "Use fs.list on a parent folder to see what exists.",
        "fs.delete" => "Check with fs.list whether it is already gone.",
        _ => "Check the path with fs.find or fs.list.",
    };
    Refusal::new("fs.not_found", ErrorCategory::NotFound, cause).then(next)
}

/// The file layer's own validation text, which names what to change.
fn invalid_request(message: &str) -> Refusal {
    Refusal::new(
        "fs.invalid_request",
        ErrorCategory::Validation,
        format!("The file tool refused this request: {}.", detail(message, 2 * MAX_DETAIL_CHARS)),
    )
}

/// The one path the call names, from the facts recorded before it ran.
fn named_path(before: &Value) -> Option<String> {
    match before["paths"].as_array()?.as_slice() {
        [only] => only["path"].as_str().map(str::to_owned),
        _ => None,
    }
}

/// A native path as a person writes it, without the `\\?\` prefix.
fn shown(path: &std::path::Path) -> String {
    crate::agent_execution::protected_paths::strip_verbatim_prefix(&path.display().to_string())
}

/// The Windows error code an operating-system error's text ends with: "… (os error 32)".
fn os_error(text: &str) -> Option<i32> {
    let (_, code) = text.rsplit_once("(os error ")?;
    code.split(')').next()?.trim().parse().ok()
}

/// Another layer's own text, quoted after Aura's sentence: one line, at most `max` characters.
fn detail(text: &str, max: usize) -> String {
    let line = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>();
    bounded(line.trim().trim_end_matches('.'), max)
}

/// `text` cut to at most `max` characters, marked where it was cut.
fn bounded(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut = text.chars().take(max.saturating_sub(1)).collect::<String>();
    if max > 0 {
        cut.push('…');
    }
    cut
}

/// A count with its thousands grouped, as the tool descriptions write it: 65,536.
fn grouped(count: u64) -> String {
    let digits = count.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

#[cfg(test)]
mod tests;
