//! Which routes the settings dispatcher answers, and the storage routes before Core has confirmed
//! where its data is: they say so instead of guessing a place.
#![allow(clippy::unwrap_used)]
use std::path::Path;

use aura_core_base::settings_routes::{serves, storage};
use aura_workspace_domain::WorkspaceSessionProjection;
use serde_json::json;

#[test]
fn only_the_settings_and_storage_routes_are_answered_here() {
    for method in [
        "ui.settings.get",
        "browser.settings.update",
        "storage.overview",
        "storage.move.request",
        "storage.old_data.list",
        "storage.old_data.remove",
    ] {
        assert!(serves(method), "{method}");
    }
    for method in ["agent.sessions.list", "workspace.relocate", "storage", "ui.settings"] {
        assert!(!serves(method), "{method}");
    }
}

#[test]
fn storage_routes_wait_until_core_knows_where_its_data_is() {
    let workspaces = || -> anyhow::Result<WorkspaceSessionProjection> {
        unreachable!("the Workspaces are read only once the locations are known")
    };
    for method in [
        "storage.overview",
        "storage.check",
        "storage.move.request",
        "storage.old_data.list",
        "storage.old_data.remove",
    ] {
        let error = storage::dispatch(
            Path::new("component-manifest.json"),
            &workspaces,
            method,
            json!({}),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().starts_with("storage.unavailable"), "{method}: {error:#}");
    }
}
