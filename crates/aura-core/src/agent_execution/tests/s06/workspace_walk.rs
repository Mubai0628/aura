//! Native object exclusion also precedes catalog text and editor overlays.
use super::*;

#[tokio::test]
async fn s06_d3_workspace_walk_excludes_replaced_authority_objects_and_old_overlay_text() {
    let f = Fixture::with_exposure_setup(
        PermissionMode::ReadOnly,
        &["fs.list", "fs.find", "fs.search"],
        |root| {
            std::fs::write(root.join("a-hidden.txt"), "s06-hidden-overlay-marker")
                .expect("write the fixture content for this permission boundary");
            std::fs::write(root.join("b-visible.txt"), "s06-visible-marker")
                .expect("write the fixture content for this permission boundary");
        },
    )
    .await;
    crate::r03_tests::success(
        &f.runtime,
        crate::method::BUFFER_OPEN,
        json!({
            "workspace_id":f.turn.workspace_id,"session_id":f.turn.session_id,
            "path":"a-hidden.txt","writable":false,
        }),
    )
    .await;
    // Replace only this test's ordinary file with an alias of its isolated Core database.
    // The stale catalog/overlay must not expose content from the excluded entry.
    let alias = f.root.join("a-hidden.txt");
    std::fs::remove_file(&alias).expect("remove the temporary fixture entry");
    std::fs::hard_link(f.service().persistence.database_path(), &alias)
        .expect("create the protected-object alias fixture");
    let batch = f.admit(&[
        ("fs.list", json!({"path":".","limit":1,"cursor":null})),
        ("fs.find", json!({"path":".","file_names":["a-hidden.txt"],
            "recursive":true,"match_mode":"exact","case_sensitive":false,"limit":100,"cursor":null})),
        ("fs.search", json!({"root":".",
            "pattern":{"kind":"literal","text":"s06-hidden-overlay-marker","case_sensitive":true},
            "path_glob":null,"include_ignored":false,"include_generated":false,"limit":100})),
    ]);
    let ids = batch.batch.calls.iter().map(|c| c.tool_call_id).collect::<Vec<_>>();
    f.execute(batch).await;
    let listed = f.output(ids[0]);
    assert_eq!(listed["entries"], json!([]), "excluded first entry must not appear");
    assert!(listed["next_cursor"].is_object(), "an excluded page must allow traversal to continue");
    let found = f.output(ids[1]);
    assert_eq!(found["matches"], json!([]));
    assert_eq!(found["complete"], false);
    let searched = f.output(ids[2]);
    assert_eq!(
        searched["matches"],
        json!([]),
        "excluded native object cannot expose stale overlay text"
    );
    assert_eq!(searched["completion"]["status"], "partial");
    for output in [&listed, &found, &searched] {
        assert!(
            output["skipped_scopes"]
                .as_array()
                .expect("the real tool result contains the expected collection")
                .iter()
                .any(|scope| { scope["reason"] == "control_authority" }),
            "actual excluded object must be identified: {output}"
        );
    }
}
