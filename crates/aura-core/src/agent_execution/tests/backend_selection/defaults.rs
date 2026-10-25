use super::support::*;
use super::*;

#[tokio::test]
async fn healthy_compat_is_selected_even_when_legacy_restricted_is_ready() {
    let (f, context) = case(PermissionMode::FullAccess, "shell.powershell", false).await;
    let compat = descriptor(&f).await;
    assert_compat(
        select(&f, &context, vec![candidate(legacy_restricted(&compat)), candidate(compat)], None)
            .await,
    );
}

#[tokio::test]
async fn both_builtin_shells_select_compat_in_workspace_mode() {
    for tool in ["shell.powershell", "shell.cmd"] {
        let (f, context) = case(PermissionMode::WorkspaceReadWrite, tool, false).await;
        let compat = descriptor(&f).await;
        assert_compat(
            select(
                &f,
                &context,
                vec![candidate(legacy_restricted(&compat)), candidate(compat)],
                None,
            )
            .await,
        );
    }
}

#[tokio::test]
async fn an_explicit_legacy_native_request_is_refused_for_builtin_shells() {
    let (f, context) = case(PermissionMode::FullAccess, "shell.cmd", false).await;
    let compat = descriptor(&f).await;
    assert_unavailable(
        select(
            &f,
            &context,
            vec![candidate(legacy_native(&compat)), candidate(compat.clone())],
            Some(aura_backend_windows_user::NATIVE_BACKEND_ID),
        )
        .await,
    );
    assert_compat(
        select(
            &f,
            &context,
            vec![candidate(compat)],
            Some(aura_backend_windows_user::compat::COMPAT_BACKEND_ID),
        )
        .await,
    );
}

#[tokio::test]
async fn absent_compat_never_falls_back_to_an_old_native_candidate() {
    let (f, context) = case(PermissionMode::FullAccess, "shell.cmd", false).await;
    let compat = descriptor(&f).await;
    assert_unavailable(select(&f, &context, vec![candidate(legacy_native(&compat))], None).await);
}
