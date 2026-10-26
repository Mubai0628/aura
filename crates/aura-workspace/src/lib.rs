#![forbid(unsafe_code)]

mod agents;
mod buffer;
mod catalog;
mod diagnostics;
mod error;
mod path;
mod publication;
mod resource_facts;
mod service;

pub use agents::{
    AgentsInstructionDocument, AgentsScopeError, AgentsScopeResolution, AgentsScopeResolver,
    AgentsScopeTarget, AgentsScopeTargetKind,
};
pub use buffer::{
    BufferCommitFact, BufferCommitPhase, BufferCommitPublisher, BufferLifecyclePreflight,
    BufferStore, DiskStaleReport, DiskTextSnapshot, LOGICAL_TEXT_STREAM_CHUNK_BYTES,
    UncommittedEditorBuffer, stream_verified_text,
};
pub use catalog::{
    CatalogBuildPhase, CatalogBuildStatus, CatalogChange, CatalogCommitHold, CatalogEntry,
    CatalogOpenReport, CatalogOptions, CatalogUpdate, CatalogUpdateReport, DecodedText,
    NameMatchMode, PathCatalog, children_beyond_index, decode_text, decode_text_with_encoding,
    encode_text,
};
pub use error::WorkspaceError;
pub use path::{
    ResolvedWorkspacePath, SecurePathResolver, ValidatedParent, VerifiedWorkspaceFile,
    domain_identity,
};
pub use publication::{BufferPublicationReceipt, WorkspacePublicationOutcome};
pub use resource_facts::{
    ResourceFacts, external_ancestry, external_object, external_slot, native_identity,
    verified_parent,
};
pub use service::{
    WatcherBatchHook, WorkspaceOpenReport, WorkspaceRuntime, WorkspaceRuntimeOptions,
};
