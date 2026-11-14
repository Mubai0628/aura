//! A `path_glob` narrows a text search of a folder outside the workspace the way it narrows
//! one inside: without `/` it names files in any folder, with `/` a path below the searched
//! folder.

use aura_fs_tools::{FsExternalSearchRequest, FsToolError, FsToolPolicy};
use aura_workspace_domain::FsSearchPattern;

use crate::external_read::Harness;

fn tree(harness: &Harness) {
    let root = harness.external.path().join("tree");
    std::fs::create_dir_all(root.join("src/deep")).expect("folders");
    for name in ["notes.txt", "main.rs", "src/lib.rs", "src/deep/mod.rs", "src/readme.md"] {
        std::fs::write(root.join(name), "needle\n").expect("file");
    }
}

fn search(harness: &Harness, path_glob: Option<&str>) -> Result<Vec<String>, FsToolError> {
    let found = harness.service(FsToolPolicy::default()).search_external(
        FsExternalSearchRequest {
            target: harness.search_target("tree"),
            pattern: FsSearchPattern::Literal { text: "needle".to_owned(), case_sensitive: true },
            recursive: true,
            path_glob: path_glob.map(str::to_owned),
            limit: 100,
            scan_budget: 256,
            max_file_bytes: 1024 * 1024,
            byte_budget: 1024 * 1024,
        },
        None,
    )?;
    assert!(found.complete, "{found:?}");
    let mut paths: Vec<String> =
        found.matches.into_iter().map(|found| found.relative_path.as_str().to_owned()).collect();
    paths.sort();
    Ok(paths)
}

#[test]
fn a_name_glob_searches_only_the_files_it_names_in_any_folder() {
    let harness = Harness::new();
    tree(&harness);

    assert_eq!(search(&harness, None).expect("search").len(), 5);
    assert_eq!(
        search(&harness, Some("*.rs")).expect("search"),
        ["tree/main.rs", "tree/src/deep/mod.rs", "tree/src/lib.rs"]
    );
    assert_eq!(
        search(&harness, Some("*.{md,txt}")).expect("search"),
        ["tree/notes.txt", "tree/src/readme.md"]
    );
}

#[test]
fn a_path_glob_names_paths_below_the_searched_folder() {
    let harness = Harness::new();
    tree(&harness);

    assert_eq!(
        search(&harness, Some("src/**/*.rs")).expect("search"),
        ["tree/src/deep/mod.rs", "tree/src/lib.rs"]
    );
    assert_eq!(search(&harness, Some("./src/*.md")).expect("search"), ["tree/src/readme.md"]);
    assert!(search(&harness, Some("docs/**")).expect("search").is_empty());
}

#[test]
fn an_unreadable_glob_is_refused_with_its_reason() {
    let harness = Harness::new();
    tree(&harness);

    match search(&harness, Some("src/[")) {
        Err(FsToolError::InvalidRequest(message)) => {
            assert!(message.contains("path_glob"), "{message}")
        }
        other => panic!("an unreadable glob must be refused: {other:?}"),
    }
}
