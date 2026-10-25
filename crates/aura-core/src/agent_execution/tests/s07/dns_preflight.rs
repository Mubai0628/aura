//! A target name is resolved only after the user's answer; resolving it is already egress.
use super::*;

#[tokio::test]
async fn s07_n2_readonly_asks_before_resolving_the_target_name() {
    // `.invalid` never resolves (RFC 6761): resolving first refuses the call without asking.
    let f = fixture(PermissionMode::ReadOnly, &["http.request"]).await;
    let batch =
        f.admit(&[("http.request", json!({"url":"http://s07-dns-before-approval.invalid/"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(
        matches!(f.execute_rejecting(batch).await, Some(BatchOutcome::AwaitingApproval { .. })),
        "the user answers before the name is sent to DNS"
    );
    no_attempt(&f, id);
}
