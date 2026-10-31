#[cfg(test)]
mod tests {
    use crate::r03_tests::{Fixture, request, success};
    use serde_json::json;

    #[tokio::test]
    async fn defaults_only_affect_new_sessions_and_reject_stale_writes() {
        let fixture = Fixture::new();
        let state = fixture.start().await;
        let root = fixture.root.path().join("permission-defaults-workspace");
        std::fs::create_dir(&root).expect("workspace");
        let workspace = success(
            &state,
            "workspace.create",
            json!({"selected_root":root,"display_name":"defaults"}),
        )
        .await;
        let workspace_id = &workspace["workspace"]["workspace_id"];
        let before = success(&state, "permission.get_defaults", json!({})).await;
        let first = success(
            &state,
            "session.create",
            json!({"workspace_id":workspace_id,"title":"existing"}),
        )
        .await;
        let first_permission = success(
            &state,
            "permission.get_session_state",
            json!({"session_id":first["session_id"]}),
        )
        .await;
        let next_mode =
            if before["mode"] == "read_only" { "workspace_read_write" } else { "read_only" };
        let saved = success(
            &state,
            "permission.set_defaults",
            json!({"expected_revision":before["revision"],"mode":next_mode}),
        )
        .await;
        assert_eq!(saved["mode"], next_mode);
        let second =
            success(&state, "session.create", json!({"workspace_id":workspace_id,"title":"new"}))
                .await;
        assert_eq!(
            success(
                &state,
                "permission.get_session_state",
                json!({"session_id":second["session_id"]})
            )
            .await["mode"],
            next_mode
        );
        assert_eq!(
            success(
                &state,
                "permission.get_session_state",
                json!({"session_id":first["session_id"]})
            )
            .await,
            first_permission
        );
        assert!(
            request(
                &state,
                "permission.set_defaults",
                json!({"expected_revision":before["revision"],"mode":"full_access"})
            )
            .await
            .get("error")
            .is_some()
        );
        assert_eq!(success(&state, "permission.get_defaults", json!({})).await, saved);
        drop(state);
        let reopened = fixture.start().await;
        assert_eq!(success(&reopened, "permission.get_defaults", json!({})).await, saved);
        assert_eq!(
            success(
                &reopened,
                "permission.get_session_state",
                json!({"session_id":first["session_id"]})
            )
            .await,
            first_permission
        );
    }
}
