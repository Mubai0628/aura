//! A registry and synthetic workspace indexes for the index tests.
// Each test file uses the part it needs.
#![allow(dead_code)]
use aura_domain_common::{CorrelationId, Revision, RuntimeInstanceId, WorkspaceId, WorkspacePath};
use aura_persistence::{
    CatalogEntryRecord, CatalogPartitionRecord, PersistenceCoordinator, PersistenceOptions,
    PersistenceStartup,
};
use aura_workspace_domain::{
    CatalogEntryKind, CatalogFreshness, CatalogIndexMode, TextEncoding, WorkspaceState,
    WorkspaceSummary,
};

/// More than ten batches.
pub const ENTRIES: usize = 24_000;

pub fn start(directory: &std::path::Path) -> PersistenceCoordinator {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        directory.join("aura.sqlite3"),
        directory.join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

pub fn workspace(persistence: &PersistenceCoordinator, name: &str) -> WorkspaceId {
    let workspace_id = WorkspaceId::new();
    persistence
        .create_workspace(
            WorkspaceSummary {
                workspace_id,
                display_name: name.to_owned(),
                selected_root: format!("C:\\{name}"),
                canonical_root: format!("C:\\{name}"),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            CorrelationId::new(),
        )
        .expect("workspace");
    workspace_id
}

pub fn entries(count: usize) -> Vec<CatalogEntryRecord> {
    (0..count)
        .map(|index| {
            let path = format!("file{index:06}.txt");
            CatalogEntryRecord {
                path: WorkspacePath::parse(&path).expect("path"),
                parent_path: WorkspacePath::parse(".").expect("root"),
                file_name: path,
                kind: CatalogEntryKind::File,
                partition_path: WorkspacePath::parse(".").expect("root"),
                byte_length: Some(64),
                modified_at: Some(format!("{}", 1_700_000_000_000_u64 + index as u64)),
                content_hash: None,
                text_encoding: Some(TextEncoding::Utf8),
                ignored: false,
                generated: false,
                hidden_from_search: false,
                reparse_tag: None,
                content: Some(format!("synthetic body {index:06} with words to tokenize")),
                change_usn: None,
                volume_serial: None,
                file_id: None,
            }
        })
        .collect()
}

pub fn partition(count: usize) -> Vec<CatalogPartitionRecord> {
    vec![CatalogPartitionRecord {
        partition_path: WorkspacePath::parse(".").expect("root"),
        file_count: count as u64,
        indexable_text_bytes: (count * 64) as u64,
        index_mode: CatalogIndexMode::FullText,
    }]
}
