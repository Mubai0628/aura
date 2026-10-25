use super::*;

#[test]
fn large_success_responses_use_p1_or_fall_back_to_a_small_stable_error() {
    let within_limit_id = CorrelationId::new();
    let within_limit = response_with_size_fallback(JsonRpcResponse::success(
        within_limit_id,
        json!({ "catalog": "c".repeat(5 * 1024 * 1024) }),
    ))
    .expect("5 MiB catalog response is supported");
    assert!(within_limit.result.is_some());
    assert!(within_limit.error.is_none());

    let oversized_id = CorrelationId::new();
    let oversized = response_with_size_fallback(JsonRpcResponse::success(
        oversized_id,
        json!({ "catalog": "c".repeat(6 * 1024 * 1024) }),
    ))
    .expect("oversized responses produce an encodable fallback");
    assert!(oversized.result.is_none());
    assert_eq!(
        oversized.error.expect("fallback error").data.code.as_str(),
        "ipc.response_too_large"
    );
}
