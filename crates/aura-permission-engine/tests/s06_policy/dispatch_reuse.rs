use super::*;

#[test]
fn s06_b4_file_approval_revalidates_new_operations_after_profile_change() {
    let mut c = session_file_case();
    c.op.operation_id = aura_domain_common::SecurityOperationId::new();
    c.op.content.action["text"] = serde_json::json!("new content");
    c.refresh();
    c.profile = SecurityProfile::Guided;
    let authority = c.allow();
    let result = dispatch_guard::revalidate(
        c.request(&authority),
        &authority,
        &c.op,
        &c.input(),
        &c.backend,
        &cancel(),
    );
    assert!(
        result.is_ok(),
        "explicit file approval survives mode-derived profile changes and a new operation: {result:?}"
    );
}
