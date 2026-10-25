#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("{0}")]
    Format(#[from] aura_memory_domain::MemoryFormatError),
    #[error("{0}")]
    File(#[from] aura_fs_tools::vault::VaultFileError),
    #[error("memory index failed: {0}")]
    Store(#[from] aura_persistence_core::PersistenceCoreError),
    #[error("memory I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("memory not found or invalid")]
    NotFound,
    #[error("invalid memory request: {0}")]
    Invalid(&'static str),
    #[error("memory contains a secret-like value ({kind}); content was not written")]
    Secret { kind: &'static str },
    #[error("memory expected_hash conflict; current hash: {current_hash}")]
    Conflict { current_hash: String },
    #[error("memory watcher unavailable: {0}")]
    Watcher(String),
    #[error("memory writer requires reconciliation")]
    RecoveryRequired,
    /// The batch failed after it began changing files, and every file it had
    /// changed was restored and confirmed: nothing of it remains.
    #[error("memory write failed and every changed file was restored: {cause}")]
    RolledBack { cause: String },
}
pub type MemoryResult<T> = Result<T, MemoryError>;
