use super::*;

#[test]
fn s06_e2_loosening_after_reservation_preserves_the_original_prepared_scope() {
    let case = Case::new();
    let session = case.request.operation.content.identity.session_id.unwrap();
    case.port
        .persistence
        .set_session_permission_mode(session, 1, PermissionMode::FullAccess, CorrelationId::new())
        .unwrap();
    let mut registered = false;
    case.port
        .claim(&case.request, &mut || {
            registered = true;
            Ok(())
        })
        .expect("loosening keeps the narrower prepared execution valid");
    assert!(registered, "the original attempt is registered without re-preparing or replay");
}
