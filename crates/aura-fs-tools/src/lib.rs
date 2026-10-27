#![forbid(unsafe_code)]

mod agent_find;
mod agent_list;
pub mod download;
mod error;
mod external;
pub mod external_faults;
mod held_rename;
mod inspect;
mod mutation;
mod publication_receipt;
/// Named, default-off interference points inside the staging area's own operations.
///
/// Exported so an acceptance test drives the **same** production code path it is testing rather
/// than a test-only copy of it. Nothing here is armed unless a caller arms it.
pub use mutation::staging_fault;
pub use publication_receipt::{FilePublicationReceipt, with_file_publications};
mod read;
mod read_scope;
pub use read_scope::{FileReadScope, FileReadScopeGuard};
mod resource_wait;
mod search;
mod service;
mod types;
mod unindexed;
pub mod vault;

pub use error::{ExternalPublishStage, ExternalWriteError, FsToolError};
pub use external::{
    ExternalFindTarget, ExternalListTarget, ExternalNameMatch, ExternalSearchTarget,
    ExternalTextReadTarget, ExternalWriteDestination, ExternalWriteKind, ExternalWriteTarget,
    FsExternalFindMatch, FsExternalFindRequest, FsExternalFindResult, FsExternalListEntry,
    FsExternalListRequest, FsExternalListResult, FsExternalPartialListing, FsExternalPatchRequest,
    FsExternalPending, FsExternalSearchMatch, FsExternalSearchRequest, FsExternalSearchResult,
    FsExternalTextReadRequest, FsExternalTextReadResult, FsExternalWriteRequest,
    FsExternalWriteResult, external_identity_revision, external_scope_revision,
};
pub use read::LISTED;
/// The cancellation intent that reaches a **not-yet-started resource wait**.
///
/// Exported so the real `aura.cancel` route records its intent where every entry's waiter can
/// observe it, rather than each entry inventing its own cancel path.
pub use resource_wait::{
    cancellation_requested as resource_wait_cancelled, clear_cancel as clear_resource_wait_cancel,
    request_cancel as cancel_resource_wait,
};
/// Test-only fault injection for the search engine's disk stage. Present only
/// with the non-default `search-test-faults` feature, which no product build
/// enables: it lets a test hold one file's real scan open, so an integration test
/// can exercise a real worker instead of a stand-in.
#[cfg(feature = "search-test-faults")]
pub use search::test_faults;
pub use search::{
    EngineDiagnostics, NeverStarted, SearchCancel, SearchCompletion, SearchPoolLoad,
    StageDiagnosticsSnapshot, cancel_search, orphan_search, register_search, release_search_flight,
    release_unstarted_search, search_finished, search_flight, search_flight_count,
    search_orphan_count, search_pool_load, set_orphan_reporter, track_search_after_handler,
    wait_for_search_orphans, wait_for_search_orphans_of,
};
pub use service::FsToolService;
pub use types::*;
#[cfg(all(feature = "acceptance-barrier", debug_assertions))]
pub mod file_acceptance;

mod execution_observer;
pub use execution_observer::{
    FileExecutionObserver, observe_file_execution, with_file_execution_observer,
};
