use super::*;

#[test]
fn a_presented_legacy_approval_expires_once_and_denies_late_choices() {
    use aura_domain_common::human_wait::Activity;

    let directory = tempfile::tempdir().unwrap();
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace_id, session_id) = seed(&coordinator);
    let request = coordinator
        .create_approval_request(
            draft(&coordinator, workspace_id, session_id),
            CorrelationId::new(),
        )
        .unwrap();
    assert_eq!(coordinator.expire_legacy_approval_wait(request.id).unwrap(), None);
    assert!(
        coordinator
            .update_legacy_approval_wait(
                SessionId::new(),
                request.id,
                "wrong".into(),
                Activity::Present
            )
            .is_err()
    );
    let (presented, arm) = coordinator
        .update_legacy_approval_wait(session_id, request.id, "window-a".into(), Activity::Present)
        .unwrap();
    assert!(arm);
    let (repeated, arm) = coordinator
        .update_legacy_approval_wait(session_id, request.id, "window-b".into(), Activity::Present)
        .unwrap();
    assert!(!arm);
    assert_eq!(presented.deadline, repeated.deadline);
    let (focus, _) = coordinator
        .update_legacy_approval_wait(session_id, request.id, "window-a".into(), Activity::Focus)
        .unwrap();
    assert!(focus.deadline > presented.deadline);
    assert_eq!(coordinator.expire_legacy_approval_wait(request.id).unwrap(), Some(focus.deadline));

    // Drive the persisted clock forward in the isolated fixture without sleeping on a real UI.
    let connection = rusqlite::Connection::open(directory.path().join("data/aura.db")).unwrap();
    connection.execute("UPDATE approval_wait_state SET timing_json=json_set(timing_json, '$.deadline', '2020-01-01T00:00:00Z', '$.focus_leases', json('{}')) WHERE approval_request_id=?1", [request.id.to_string()]).unwrap();
    let (choice, expiry) = std::thread::scope(|race| {
        let choice = race.spawn(|| {
            coordinator.resolve_approval(
                request.id,
                request.request_revision,
                ApprovalDecisionKind::AllowSession,
                CorrelationId::new(),
            )
        });
        let expiry = race.spawn(|| coordinator.expire_legacy_approval_wait(request.id));
        (choice.join().unwrap(), expiry.join().unwrap())
    });
    assert!(matches!(
        choice,
        Err(PersistenceError::ApprovalStale | PersistenceError::ApprovalAlreadyResolved)
    ));
    assert_eq!(expiry.unwrap(), None);
    assert_eq!(coordinator.expire_legacy_approval_wait(request.id).unwrap(), None);
    assert!(coordinator.pending_approvals().unwrap().is_empty());
    assert_eq!(coordinator.approval_details(request.id).unwrap().state, ApprovalState::Expired);
    assert_eq!(coordinator.session_permission_state(session_id).unwrap().active_grant_count, 0);
}
