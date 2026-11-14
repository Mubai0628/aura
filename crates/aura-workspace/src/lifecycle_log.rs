//! Where a workspace lifecycle operation that hits a failure is recorded.
//!
//! The operation's record keeps only an error code. The error beneath the code, such as the
//! store's own refusal, exists only at the moment of the failure, so it is sent here then. The
//! process that embeds the runtime routes these records to its own log once; until it does, they
//! are dropped.

use std::sync::OnceLock;

use aura_workspace_domain::{WorkspaceLifecycleState, WorkspaceLifecycleSummary};
use serde_json::{Value, json};

/// Receives one record: its event name and its fields.
pub type LifecycleLog = fn(&'static str, Value);

static ROUTE: OnceLock<LifecycleLog> = OnceLock::new();

/// The longest reason kept, in bytes; a longer one is cut at a character boundary.
const REASON_LIMIT: usize = 1024;

/// Sends lifecycle failure records to `log`. The first route set in a process stays.
pub fn route_lifecycle_log(log: LifecycleLog) {
    let _ = ROUTE.set(log);
}

/// Records that `summary`'s operation hit a failure and now stands at `state` with `error_code`.
/// `reason` is the error beneath the code, when there is one.
pub(crate) fn record_failure(
    summary: &WorkspaceLifecycleSummary,
    state: WorkspaceLifecycleState,
    error_code: &str,
    reason: Option<&str>,
) {
    let Some(log) = ROUTE.get() else {
        return;
    };
    let mut fields = json!({
        "operation_id": summary.operation_id,
        "workspace_id": summary.workspace_id,
        "operation": summary.operation,
        "state": state,
        "error_code": error_code,
    });
    if let Some(reason) = reason {
        fields["reason"] = Value::String(bounded(reason));
    }
    log("workspace.lifecycle_failure", fields);
}

fn bounded(reason: &str) -> String {
    if reason.len() <= REASON_LIMIT {
        return reason.to_owned();
    }
    let mut end = REASON_LIMIT;
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &reason[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_reason_is_cut_at_a_character_boundary() {
        assert_eq!(bounded("short"), "short");
        let kept = bounded(&"错".repeat(REASON_LIMIT));
        let body = kept.strip_suffix('…').expect("a cut reason says it was cut");
        assert!(body.len() <= REASON_LIMIT && body.len() > REASON_LIMIT - '错'.len_utf8());
        assert!(body.chars().all(|character| character == '错'));
    }
}
