//! `path_glob` reads as ripgrep's `--glob`: a pattern without `/` names files in any folder.
use super::*;
use aura_workspace_domain::CatalogEntryKind;

fn file(path: &str) -> CatalogEntry {
    let parsed = WorkspacePath::parse(path).expect("path");
    let (parent, name) = path.rsplit_once('/').unwrap_or((".", path));
    CatalogEntry {
        path: parsed,
        parent_path: WorkspacePath::parse(parent).expect("parent"),
        file_name: name.to_owned(),
        kind: CatalogEntryKind::File,
        partition_path: WorkspacePath::parse(".").expect("root"),
        byte_length: Some(1),
        modified_at: None,
        content_hash: None,
        text_encoding: None,
        ignored: false,
        generated: false,
        hidden_from_search: false,
        reparse_tag: None,
        change_usn: None,
        volume_serial: None,
        file_id: None,
    }
}

fn tree() -> Vec<CatalogEntry> {
    [
        "lib.rs",
        "README.md",
        "crates/core/src/lib.rs",
        "crates/core/src/run.rs",
        "crates/core/Cargo.toml",
        "docs/plan.md",
        "apps/ui/main.tsx",
    ]
    .into_iter()
    .map(file)
    .collect()
}

fn found(root: &str, glob: &str) -> Vec<String> {
    let root = WorkspacePath::parse(root).expect("root");
    freeze_candidates(tree(), &root, Some(glob), false, false)
        .expect("candidates")
        .into_iter()
        .map(|candidate| candidate.path.as_str().to_owned())
        .collect()
}

#[test]
fn a_pattern_without_a_slash_names_files_in_any_folder() {
    assert_eq!(found(".", "*.rs"), ["crates/core/src/lib.rs", "crates/core/src/run.rs", "lib.rs"]);
    assert_eq!(found(".", "*.{md,toml}"), ["README.md", "crates/core/Cargo.toml", "docs/plan.md"]);
    assert_eq!(found("crates", "lib.rs"), ["crates/core/src/lib.rs"]);
}

#[test]
fn a_pattern_with_a_slash_names_a_path_below_the_search_root_or_the_workspace_root() {
    assert_eq!(
        found("crates/core", "src/*.rs"),
        ["crates/core/src/lib.rs", "crates/core/src/run.rs"]
    );
    assert_eq!(found(".", "crates/**/*.rs"), ["crates/core/src/lib.rs", "crates/core/src/run.rs"]);
    assert_eq!(found("crates", "crates/core/*.toml"), ["crates/core/Cargo.toml"]);
    assert_eq!(found(".", "./docs/*.md"), ["docs/plan.md"]);
    assert!(found(".", "src/*.rs").is_empty(), "src/ is not a top-level folder");
}
