//! A Git or package call recorded Unknown gives back its locks and its capacity slot once no
//! process it started is left. A group of processes Aura saw stop leaves nothing behind, so its
//! call gives them back at once; a group that may still have processes in it is checked every few
//! seconds, and they are given back once it is empty. The result stays Unknown and is never
//! replayed. Until then, a Git call of the session that waits behind it says what it waits for.
use super::*;
use aura_execution_contract::AttemptRef;
use std::time::Duration;

/// How often a group that may still have processes in it is checked.
const EVERY: Duration = Duration::from_secs(2);
/// How long it is checked; after that its locks stay until Core restarts.
const LIMIT: Duration = Duration::from_secs(30 * 60);

/// What a Git call of the session reads while it waits behind one whose processes were not seen
/// to stop.
pub const WAITING_FOR_GIT: &str = "在等之前一个还没确认停下的 Git 进程";

/// Gives back the locks of `call`, recorded Unknown, once no process it started is left: at once
/// when there is none, else from a background check. Only Git calls and package programs.
pub(super) fn release_when_stopped(
    service: Arc<AgentExecution>,
    call: &CallContext,
    attempt: AttemptRef,
) {
    let git = call.operation.content.action_kind.starts_with("git.");
    if !git && !super::package_tools::program::is_program(&call.operation.content) {
        return;
    }
    let operation = attempt.operation_id;
    let scope = operation.to_bytes();
    let session = call.snapshot.session_id;
    let release = move |service: &AgentExecution| {
        // Each release checks the backend that ran the call: Core's broker, or a native process.
        let released = service
            .orchestrator
            .release_core_call(&attempt)
            .or_else(|_| service.orchestrator.release_managed_call(&attempt));
        aura_core_base::append_core_log(
            "tool.unknown_call_released",
            serde_json::json!({
                "operation_id": attempt.operation_id.to_string(),
                "released": released.is_ok(),
            }),
        );
    };
    if aura_backend_windows_user::lingering::settled(scope) {
        release(&service);
        return;
    }
    let mark = git.then(|| {
        service.active_calls.hold(
            operation,
            super::active_calls::Hold { session, family: "git.", words: WAITING_FOR_GIT },
        )
    });
    tokio::spawn(async move {
        let _mark = mark;
        if until_stopped(|| aura_backend_windows_user::lingering::settled(scope), EVERY, LIMIT)
            .await
        {
            release(&service);
        }
    });
}

/// Waits, checking every `every` for at most `limit`, until `stopped` says no process is left.
async fn until_stopped(
    mut stopped: impl FnMut() -> bool,
    every: Duration,
    limit: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        if stopped() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(every).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(1);

    #[tokio::test]
    async fn processes_seen_stopped_give_their_locks_back_at_once() {
        let mut checks = 0;
        let started = std::time::Instant::now();
        assert!(
            until_stopped(
                || {
                    checks += 1;
                    true
                },
                Duration::from_secs(60),
                Duration::from_secs(60),
            )
            .await
        );
        assert_eq!(checks, 1);
        assert!(started.elapsed() < Duration::from_secs(5), "no wait before the first check");
    }

    #[tokio::test]
    async fn processes_that_may_still_run_keep_their_locks() {
        let mut checks = 0;
        assert!(
            !until_stopped(
                || {
                    checks += 1;
                    false
                },
                TICK,
                Duration::from_millis(20),
            )
            .await
        );
        assert!(checks >= 2, "checked again while it waited: {checks}");
    }

    #[tokio::test]
    async fn locks_come_back_once_the_group_is_empty() {
        let mut checks = 0;
        assert!(
            until_stopped(
                || {
                    checks += 1;
                    checks == 3
                },
                TICK,
                Duration::from_secs(60),
            )
            .await
        );
        assert_eq!(checks, 3);
    }
}
