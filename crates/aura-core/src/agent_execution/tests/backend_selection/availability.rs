use super::support::*;
use super::*;

#[tokio::test]
async fn unrelated_legacy_binding_state_does_not_block_compat() {
    let (f, context) = case(PermissionMode::FullAccess, "shell.powershell", true).await;
    let compat = descriptor(&f).await;
    for reason in ["runner.core_binding_mismatch.file_id", "runner.core_binding_unreadable"] {
        let mut legacy = legacy_restricted(&compat);
        legacy.readiness = BackendReadiness::RepairRequired { reason: reason.into() };
        assert_compat(
            select(&f, &context, vec![candidate(legacy), candidate(compat.clone())], None).await,
        );
    }
}

#[tokio::test]
async fn compat_availability_never_falls_back_to_legacy_backends() {
    let (f, context) = case(PermissionMode::FullAccess, "shell.cmd", false).await;
    let compat = descriptor(&f).await;
    for readiness in [
        BackendReadiness::Unavailable { reason: "compat.runtime_unavailable".into() },
        BackendReadiness::RepairRequired { reason: "compat.runtime_verification_failed".into() },
        BackendReadiness::SetupRequired { reason: "compat.runtime_missing".into() },
    ] {
        let mut unavailable = compat.clone();
        unavailable.readiness = readiness;
        assert_unavailable(
            select(
                &f,
                &context,
                vec![
                    candidate(unavailable),
                    candidate(legacy_native(&compat)),
                    candidate(legacy_restricted(&compat)),
                ],
                None,
            )
            .await,
        );
    }
}

#[tokio::test]
async fn an_inapplicable_compat_candidate_never_routes_to_legacy() {
    let (f, context) = case(PermissionMode::FullAccess, "shell.cmd", false).await;
    let compat = descriptor(&f).await;
    let mut inapplicable = compat.clone();
    inapplicable.capabilities.clear();
    assert_unavailable(
        select(
            &f,
            &context,
            vec![candidate(inapplicable), candidate(legacy_native(&compat))],
            None,
        )
        .await,
    );
}
