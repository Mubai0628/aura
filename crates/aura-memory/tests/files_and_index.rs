mod support;
use aura_domain_common::WorkspacePath;
use aura_memory::{MemoryError, MemorySearch, MemoryStatus, MemoryWrite, Vault};
use support::{Fixture, actor};

#[test]
fn reads_and_rebuilds_do_not_create_vault_first_write_creates_complete_layout() {
    let f = Fixture::project();
    assert!(!f.root.exists());
    f.vault.rebuild().expect("rebuild absent");
    assert!(f.vault.search(&MemorySearch::default()).expect("search").hits.is_empty());
    assert!(!f.root.exists());
    let write = f.write("conventions/build.md", "# 构建约定\n中文构建命令 cargo test。\n");
    let id = write.note.metadata.id;
    let hash = f.vault.write(&[write], &actor()).expect("first write");
    for dir in [
        "architecture",
        "conventions",
        "decisions",
        "build",
        "commands",
        "modules",
        "known-issues",
        "experience",
        "rules",
        "events/archive",
        "archive",
        "views",
    ] {
        assert!(f.root.join(dir).is_dir(), "{dir}");
    }
    assert_eq!(std::fs::read_to_string(f.root.join(".gitignore")).expect("ignore"), "*\n");
    assert!(!f.temp.path().join("workspace/.gitignore").exists());
    assert!(
        std::fs::read_to_string(f.root.join("README.md")).expect("README").contains("Obsidian")
    );
    let read = f.vault.read_id(id).expect("stable id");
    assert_eq!(read.hash, hash[0]);
    for query in ["约定", "构建命令", "cargo test", "中文构建"] {
        let hits = f
            .vault
            .search(&MemorySearch { query: query.into(), ..Default::default() })
            .expect("search")
            .hits;
        assert_eq!(hits.len(), 1, "{query}");
        assert_eq!(hits[0].metadata.id, id);
    }
}

#[test]
fn rename_and_edit_preserve_id_invalid_or_duplicate_files_are_never_overwritten() {
    let f = Fixture::project();
    let write = f.write("conventions/a.md", "# Existing\noriginal\n");
    let id = write.note.metadata.id;
    f.vault.write(&[write], &actor()).expect("write");
    std::fs::rename(f.root.join("conventions/a.md"), f.root.join("conventions/moved.md"))
        .expect("external rename");
    f.vault.rebuild().expect("rebuild");
    assert_eq!(f.vault.read_id(id).expect("same id").path.as_str(), "conventions/moved.md");
    let valid = std::fs::read_to_string(f.root.join("conventions/moved.md")).expect("note");
    std::fs::write(f.root.join("conventions/duplicate.md"), &valid).expect("duplicate");
    f.vault.rebuild().expect("detect duplicate");
    assert_eq!(f.vault.status().expect("status").invalid, 2);
    assert!(f.vault.read_id(id).is_err());
    std::fs::remove_file(f.root.join("conventions/duplicate.md"))
        .expect("remove fixture duplicate");
    std::fs::write(f.root.join("conventions/moved.md"), "---\nid: broken\n---\nUser body\n")
        .expect("invalid external edit");
    let before = std::fs::read(f.root.join("conventions/moved.md")).expect("invalid bytes");
    assert_eq!(f.vault.validate().expect("validate").len(), 1);
    assert_eq!(std::fs::read(f.root.join("conventions/moved.md")).expect("unchanged"), before);
    assert!(f.vault.search(&MemorySearch::default()).expect("search").hits.is_empty());
    std::fs::write(
        f.root.join("conventions/moved.md"),
        valid.replace("original", "external edited"),
    )
    .expect("repair externally");
    f.vault.rebuild().expect("valid edit");
    assert_eq!(f.vault.status().expect("status").invalid, 0);
    assert_eq!(f.vault.read_id(id).expect("read").history[0].actor, "external");
}

#[test]
fn stale_hash_and_invalid_batch_leave_every_original_byte_unchanged() {
    let f = Fixture::project();
    let first = f.write("conventions/a.md", "# A\n");
    let id = first.note.metadata.id;
    let hashes = f.vault.write(std::slice::from_ref(&first), &actor()).expect("write");
    let mut update = first.clone();
    update.expected_hash = Some(hashes[0].clone());
    update.note.body = "# newer\n".into();
    f.vault.write(&[update], &actor()).expect("update");
    let current = f.vault.read_id(id).expect("current");
    let before = std::fs::read(f.root.join("conventions/a.md")).expect("bytes");
    let mut stale = first;
    stale.expected_hash = Some(hashes[0].clone());
    stale.note.body = "# stale\n".into();
    let new = f.write("conventions/b.md", "# B\n");
    assert!(
        matches!(f.vault.write(&[new,stale],&actor()),Err(MemoryError::Conflict {current_hash}) if current_hash==current.hash)
    );
    assert!(!f.root.join("conventions/b.md").exists());
    assert_eq!(std::fs::read(f.root.join("conventions/a.md")).expect("bytes"), before);
    let mut bad = f.write("conventions/c.md", "# secret\n");
    bad.note.body.push_str("sk-abcdefghijklmnopqrstuvwxyz012345");
    let error = f.vault.write(&[bad], &actor()).expect_err("secret blocked");
    assert!(error.to_string().contains("api_token"));
    assert!(!error.to_string().contains("abcdefghijklmnopqrstuvwxyz"));
    assert!(!f.root.join("conventions/c.md").exists());
}

#[test]
fn independent_vaults_reject_foreign_links_and_cache_reuse() {
    let f = Fixture::project();
    let g = Fixture::project();
    let other = g.write("conventions/other.md", "# Other\n");
    let other_id = other.note.metadata.id;
    g.vault.write(&[other], &actor()).expect("other write");
    let mut local = f.write("conventions/local.md", "# Local\n");
    local.note.metadata.related_ids.push(other_id);
    assert!(f.vault.write(&[local], &actor()).is_err());
    assert!(!f.root.exists());
    assert!(f.vault.read_id(other_id).is_err());
    assert!(
        Vault::open(g.root.clone(), f.vault.scope(), &f.temp.path().join("cache/memory.db"))
            .is_err()
    );
}

#[test]
fn ranking_and_status_filters_keep_conflicts_and_hide_forgotten_archived_superseded() {
    let f = Fixture::project();
    let mut writes = vec![];
    for (name, status, importance) in [
        ("low", MemoryStatus::Active, 1),
        ("high", MemoryStatus::Active, 5),
        ("conflict", MemoryStatus::Conflicted, 3),
        ("forgotten", MemoryStatus::Forgotten, 5),
        ("archive", MemoryStatus::Archived, 5),
        ("superseded", MemoryStatus::Superseded, 5),
    ] {
        let mut w = f.write(&format!("conventions/{name}.md"), "# Rust 构建约定\n");
        w.note.metadata.status = status;
        w.note.metadata.importance = importance;
        writes.push(w);
    }
    let conflict_id = writes[1].note.metadata.id;
    writes[2].note.metadata.conflicts_with_ids.push(conflict_id);
    f.vault.write(&writes, &actor()).expect("batch");
    let result = f
        .vault
        .search(&MemorySearch { query: "约定".into(), ..Default::default() })
        .expect("search");
    assert_eq!(result.hits.len(), 3);
    assert_eq!(result.hits[0].metadata.importance, 5);
    assert_eq!(result.hits[1].metadata.conflicts_with_ids.len(), 1);
    let read = f
        .vault
        .read_path(&WorkspacePath::parse("conventions/high.md").expect("path"))
        .expect("read");
    f.vault.delete(&[(read.note.metadata.id, read.hash)], &actor()).expect("delete");
    assert_eq!(
        f.vault.history(Some(read.note.metadata.id), 10).expect("history")[0].action,
        "delete"
    );
}

#[test]
fn two_sessions_share_single_writer_and_one_stale_update_loses_without_overwrite() {
    let f = Fixture::project();
    let w = f.write("conventions/a.md", "# before\n");
    let id = w.note.metadata.id;
    f.vault.write(&[w], &actor()).expect("write");
    let read = f.vault.read_id(id).expect("read");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|n| {
            let vault = f.vault.clone();
            let barrier = barrier.clone();
            let mut note = read.note.clone();
            let hash = read.hash.clone();
            std::thread::spawn(move || {
                note.body = format!("# writer {n}\n");
                barrier.wait();
                vault.write(
                    &[MemoryWrite {
                        path: WorkspacePath::parse("conventions/a.md").expect("path"),
                        note,
                        expected_hash: Some(hash),
                    }],
                    &actor(),
                )
            })
        })
        .collect::<Vec<_>>();
    let results = threads.into_iter().map(|t| t.join().expect("writer")).collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().filter(|r| matches!(r, Err(MemoryError::Conflict { .. }))).count(),
        1
    );
}
