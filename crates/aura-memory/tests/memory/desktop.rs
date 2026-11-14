use aura_domain_common::{RuntimeInstanceId, WorkspaceId, WorkspacePath};
use aura_memory::*;
fn setup() -> (tempfile::TempDir, MemoryService, MemoryWorkspace) {
    let temp = tempfile::tempdir().expect("desktop fixture");
    let roots = VaultRoots { global: temp.path().join("global"), cache: temp.path().join("cache") };
    let workspace =
        MemoryWorkspace { workspace_id: WorkspaceId::new(), root: temp.path().join("workspace") };
    std::fs::create_dir(&workspace.root).expect("desktop fixture");
    let service = MemoryService::new(roots);
    (temp, service, workspace)
}
#[test]
fn desktop_reads_and_empty_exports_never_create_a_vault() {
    let (_temp, service, workspace) = setup();
    let status = service
        .desktop_snapshot(Some(&workspace), RuntimeInstanceId::new())
        .expect("desktop fixture");
    assert_eq!(status.vaults.len(), 2);
    assert!(status.vaults.iter().all(|v| !v.exists && v.valid == 0 && v.invalid == 0));
    let vault =
        service.desktop_vault(MemoryScope::Project, Some(&workspace)).expect("desktop fixture");
    let export = service.prepare_export(&vault).expect("desktop fixture");
    assert_eq!(export.entries, 0);
    assert!(!vault.root().exists());
    assert!(!service.roots().global.exists());
    assert!(service.desktop_vault(MemoryScope::Project, None).is_err());
    assert!(
        service
            .desktop_snapshot(None, RuntimeInstanceId::new())
            .expect("desktop fixture")
            .vaults
            .len()
            == 1
    );
}
#[test]
fn export_preserves_invalid_files_layout_and_frozen_bytes() {
    let (_temp, service, workspace) = setup();
    let vault = service.vault(MemoryScope::Project, &workspace).expect("desktop fixture");
    vault
        .write(
            &[MemoryWrite {
                path: WorkspacePath::parse("rules/note.md").expect("desktop fixture"),
                note: MemoryDocument {
                    metadata: MemoryMetadata::new(MemoryScope::Project, "rules".into()),
                    body: "# 中文约定\n".into(),
                },
                expected_hash: None,
            }],
            &WriteActor::default(),
        )
        .expect("desktop fixture");
    let bad = vault.root().join("rules/invalid.md");
    std::fs::write(&bad, b"not frontmatter\n").expect("desktop fixture");
    let preview = vault.preview_repair(&WriteActor::default()).expect("desktop fixture");
    assert!(!preview.preview_token.is_empty());
    let before = std::fs::read(&bad).expect("desktop fixture");
    let handle = service.prepare_export(&vault).expect("desktop fixture");
    std::fs::write(&bad, b"changed after snapshot").expect("desktop fixture");
    let mut names = vec![];
    let mut invalid_index = None;
    for index in 0..handle.entries {
        let entry = service
            .export_entry(&MemoryExportEntryRequest { export_id: handle.export_id.clone(), index })
            .expect("desktop fixture");
        if entry.path == "rules/invalid.md" {
            invalid_index = Some(index);
            assert_eq!(entry.sha256, aura_domain_common::ContentHash::sha256(&before).hex);
        }
        names.push(entry.path);
    }
    for name in [".gitignore", "README.md", "views", "rules/note.md", "rules/invalid.md"] {
        assert!(names.iter().any(|n| n == name), "missing {name}");
    }
    let index = invalid_index.expect("desktop fixture");
    let mut bytes = vec![];
    let mut offset = 0;
    loop {
        let chunk = service
            .export_chunk(&MemoryExportChunkRequest {
                export_id: handle.export_id.clone(),
                index,
                offset,
                max_bytes: 3,
            })
            .expect("desktop fixture");
        bytes.extend(chunk.bytes);
        offset = chunk.next_offset;
        if chunk.eof {
            break;
        }
    }
    assert_eq!(bytes, before);
    assert!(
        service
            .export_chunk(&MemoryExportChunkRequest {
                export_id: handle.export_id.clone(),
                index,
                offset: 0,
                max_bytes: 65537
            })
            .is_err()
    );
    assert!(
        service
            .export_chunk(&MemoryExportChunkRequest {
                export_id: handle.export_id.clone(),
                index,
                offset: 10000,
                max_bytes: 1
            })
            .is_err()
    );
    service.close_export(&MemoryExportCloseRequest { export_id: handle.export_id.clone() });
    assert!(
        service
            .export_entry(&MemoryExportEntryRequest { export_id: handle.export_id, index })
            .is_err()
    );
}
#[test]
fn export_refuses_hardlinks_even_for_non_markdown_files() {
    let (temp, service, workspace) = setup();
    let vault = service.vault(MemoryScope::Project, &workspace).expect("desktop fixture");
    std::fs::create_dir(vault.root()).expect("desktop fixture");
    let outside = temp.path().join("other.bin");
    std::fs::write(&outside, b"temporary test value").expect("desktop fixture");
    std::fs::hard_link(&outside, vault.root().join("linked.bin")).expect("desktop fixture");
    assert!(service.prepare_export(&vault).is_err());
    assert_eq!(std::fs::read(outside).expect("desktop fixture"), b"temporary test value");
}
