//! Why the managed Shell refused a command before it started, in words the model reads. Each
//! refusal keeps its code first; the cause found in the underlying error follows, then that
//! nothing ran and what to do next. Only Aura's own codes are matched: OS text, paths and the
//! command itself never reach the reason.

pub(super) const RUNTIME_UNAVAILABLE: &str = "compat.runtime_unavailable";
pub(super) const UNSUPPORTED: &str = "compat.unsupported";
pub(super) const SCOPE_CHANGED: &str = "compat.scope_changed";
pub(super) const PREPARATION_FAILED: &str = "compat.preparation_failed";
pub(super) const REVALIDATION_FAILED: &str = "compat.dispatch_revalidation_failed";
/// Added by the preparation to the working folder's own failure.
pub(super) const WORKING_DIRECTORY_UNAVAILABLE: &str = "compat.working_directory_unavailable";

/// Codes found in the underlying error, most specific first; the cause each names and the next
/// step.
const CAUSES: &[(&[&str], &str, &str)] = &[
    (
        &[
            "compat.runtime_verification_failed",
            "compat.prepare_worker_unavailable",
            "compat.prepare_worker_lost",
        ],
        "Aura's managed shell failed its installation check in this run of Aura",
        "Tell the user; restarting or reinstalling Aura may be needed.",
    ),
    (
        &["command.workspace_not_online"],
        "the workspace is offline",
        "Ask the user to open the workspace again, then call it again.",
    ),
    (
        &["command.workspace_identity_changed", "command.workspace_identity_missing"],
        "the workspace folder was moved, replaced or deleted",
        "Ask the user to locate the workspace again in Aura, then call it again.",
    ),
    (
        &["command.mode_changed"],
        "the session's permission mode changed",
        "Call it again; it is checked against the new mode.",
    ),
    (
        &["command.native_consent_changed"],
        "the user's approval for it changed",
        "Call it again; it may ask for approval.",
    ),
    (
        &[WORKING_DIRECTORY_UNAVAILABLE],
        "its working_directory could not be opened (it may not exist)",
        "Give an existing folder inside the workspace as working_directory.",
    ),
    (
        &["compat.directory_required"],
        "its working_directory is a file, not a folder",
        "Give an existing folder inside the workspace as working_directory.",
    ),
    (
        &["compat.working_directory_changed"],
        "its working_directory is outside the folders it may use",
        "Give a folder inside the workspace as working_directory.",
    ),
    (
        &["compat.preparation_changed"],
        "it waited too long to start (a prepared command is valid for 10 minutes), or what runs it changed",
        "Call it again.",
    ),
    (
        &["command.scope_changed", SCOPE_CHANGED],
        "the folders it may use changed after it was prepared",
        "Call it again.",
    ),
    (
        &["compat.replay_or_capacity"],
        "32 commands are already running, or this call is already running",
        "Wait for a running command to finish, then call it again.",
    ),
    (
        &["persistent revision or identity conflict"],
        "the turn it belonged to was stopped or changed before it started",
        "Call it again if it is still needed.",
    ),
    (
        &[
            "compat.shell_changed",
            "compat.builtin_shell_required",
            "compat.credential_binding_not_supported",
            "compat.network_declaration",
        ],
        "the call does not match what this shell can run",
        "Call shell.powershell or shell.cmd again with only its documented arguments.",
    ),
];

/// The refusal reason for `code`, naming the cause found in `error` when Aura knows it.
pub(super) fn reason(code: &str, error: Option<&anyhow::Error>) -> String {
    let chain = error
        .map(|error| error.chain().map(ToString::to_string).collect::<Vec<_>>().join(": "))
        .unwrap_or_default();
    let (cause, next) = CAUSES
        .iter()
        .find(|(codes, ..)| codes.iter().any(|known| chain.contains(known)))
        .map_or_else(|| fallback(code), |(_, cause, next)| (*cause, *next));
    format!(
        "{code}: The command did not start: {cause}. Nothing ran and nothing was changed. {next}"
    )
}

fn fallback(code: &str) -> (&'static str, &'static str) {
    match code {
        RUNTIME_UNAVAILABLE => (
            "Aura's managed shell is not available in this run of Aura",
            "Tell the user; restarting Aura may be needed.",
        ),
        UNSUPPORTED => (
            "the shell set up for this workspace cannot run this kind of call",
            "Do it another way.",
        ),
        SCOPE_CHANGED => (
            "Aura could not confirm the workspace folders the command may use",
            "Call it again; if it fails again, tell the user.",
        ),
        PREPARATION_FAILED => (
            "Aura could not prepare the shell to run it",
            "Try again; if it fails again, tell the user.",
        ),
        _ => (
            "the workspace, its permissions or what runs it changed after it was prepared",
            "Call it again.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_keeps_its_code_and_names_the_cause_it_found() {
        let error = anyhow::anyhow!("os error 3").context(WORKING_DIRECTORY_UNAVAILABLE);
        let said = reason(PREPARATION_FAILED, Some(&error));
        assert!(
            said.starts_with(
                "compat.preparation_failed: The command did not start: its working_directory"
            ),
            "{said}"
        );
        assert!(said.contains("Nothing ran") && !said.contains("os error"), "{said}");

        let offline =
            reason(REVALIDATION_FAILED, Some(&anyhow::anyhow!("command.workspace_not_online")));
        assert!(offline.contains("the workspace is offline"), "{offline}");
        let runtime = reason(
            RUNTIME_UNAVAILABLE,
            Some(&anyhow::anyhow!("compat.runtime_verification_failed")),
        );
        assert!(
            runtime.contains("installation check") && runtime.contains("Tell the user"),
            "{runtime}"
        );
    }

    #[test]
    fn an_unknown_cause_still_says_why_in_the_codes_own_words() {
        let said = reason(UNSUPPORTED, None);
        assert!(
            said.starts_with("compat.unsupported: The command did not start: the shell"),
            "{said}"
        );
        let said = reason(REVALIDATION_FAILED, Some(&anyhow::anyhow!("something else")));
        assert!(said.contains("changed after it was prepared"), "{said}");
        for code in [
            RUNTIME_UNAVAILABLE,
            UNSUPPORTED,
            SCOPE_CHANGED,
            PREPARATION_FAILED,
            REVALIDATION_FAILED,
        ] {
            let said = reason(code, None);
            assert!(said.len() <= 512 && !said.contains(['{', '}']), "{said}");
        }
    }
}
