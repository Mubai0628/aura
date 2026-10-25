//! A call that leaves out a field gets the default its description states.
//!
//! The search and find tools name their directory in a field Core reads before the
//! input is decoded, so the default has to exist there too, not only in the decoded type.
use super::*;

#[tokio::test]
async fn find_and_search_without_a_directory_search_the_workspace_root() {
    let f =
        Fixture::with_exposure_setup(PermissionMode::ReadOnly, &["fs.find", "fs.search"], |root| {
            std::fs::create_dir_all(root.join("src")).expect("create the nested fixture directory");
            std::fs::write(root.join("src").join("s09-target.txt"), "s09-default-root-marker")
                .expect("write the file both tools must find");
        })
        .await;
    let batch = f.admit(&[
        ("fs.find", json!({"file_names":["S09-TARGET.TXT"]})),
        ("fs.search", json!({"pattern":{"kind":"literal","text":"S09-DEFAULT-ROOT-MARKER"}})),
    ]);
    let ids = batch.batch.calls.iter().map(|call| call.tool_call_id).collect::<Vec<_>>();
    for id in &ids {
        let Some(call) = batch.calls.get(id) else {
            let store = f.service().broker.lock().expect("read the real journal");
            let row = aura_persistence_v2::tool::execution::call(store.database.connection(), *id)
                .expect("the refused call is durable");
            panic!("the call was refused before execution: {:?}", row.result);
        };
        let resources = &call.operation.content.resources;
        assert!(
            resources.iter().any(|resource| matches!(
                resource,
                aura_security_contract::ResourceDescriptor::WorkspacePath { canonical_relative_path, .. }
                    if canonical_relative_path.as_str() == "."
            )),
            "the operation names the workspace root it resolved: {resources:?}"
        );
    }
    f.execute(batch).await;
    // Recursive, exact-name and case-insensitive by default.
    let found = f.output(ids[0]);
    let found_paths = found["matches"]
        .as_array()
        .expect("the real tool result lists its matches")
        .iter()
        .map(|entry| entry["path"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(found_paths, ["src/s09-target.txt"], "{found}");
    // Case-insensitive literal text by default.
    let searched = f.output(ids[1]);
    assert_eq!(
        searched["matches"].as_array().map(Vec::len),
        Some(1),
        "the default search covers the whole workspace: {searched}"
    );
}
