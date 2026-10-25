use aura_domain_common::{WorkspaceId, WorkspacePath};
use aura_memory::{
    MemoryDocument, MemoryMetadata, MemoryScope, MemoryWrite, Vault, VaultScope, WriteActor,
};
use std::{path::PathBuf, sync::Arc};
pub struct Fixture {
    #[allow(dead_code)] // Owns the TEMP lifetime even in scenarios that only use the Vault.
    pub temp: tempfile::TempDir,
    pub vault: Arc<Vault>,
    pub root: PathBuf,
}
impl Fixture {
    pub fn project() -> Self {
        let temp = tempfile::tempdir().expect("TEMP");
        let root = temp.path().join("workspace/.memories");
        std::fs::create_dir(temp.path().join("workspace")).expect("workspace");
        let scope = VaultScope::Project { workspace_id: WorkspaceId::new() };
        let vault = Vault::open(root.clone(), scope, &temp.path().join("cache/memory.db"))
            .expect("explicit temporary root");
        Self { temp, vault, root }
    }
    pub fn write(&self, path: &str, body: &str) -> MemoryWrite {
        MemoryWrite {
            path: WorkspacePath::parse(path).expect("relative path"),
            note: MemoryDocument {
                metadata: MemoryMetadata::new(MemoryScope::Project, "conventions".into()),
                body: body.into(),
            },
            expected_hash: None,
        }
    }
}
pub fn actor() -> WriteActor {
    WriteActor { kind: "model_tool".into(), ..Default::default() }
}
