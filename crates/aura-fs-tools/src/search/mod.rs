mod budget;
#[cfg(test)]
mod budget_regressions;
mod cancel;
mod candidates;
mod classify;
mod commit;
mod diagnostics;
mod disk;
#[cfg(feature = "search-test-faults")]
pub use disk::faults as test_faults;
mod engine;
pub(crate) mod excerpt;
mod flights;
mod matcher;
mod overlay;
mod policy;
mod pool;
mod sink;
mod types;

pub use cancel::{SearchCancel, SearchCompletion};
pub(crate) use diagnostics::StageDiagnostics as StageDiagnosticsImpl;
pub use diagnostics::StageDiagnosticsSnapshot;
pub(crate) use engine::execute;
pub use flights::{
    NeverStarted, active_count as search_flight_count, cancel as cancel_search,
    get as search_flight, is_finished as search_finished, orphan as orphan_search,
    orphan_count as search_orphan_count, register as register_search,
    release_unstarted as release_unstarted_search, set_orphan_reporter,
    take_if_finished as release_search_flight, track_after_handler as track_search_after_handler,
    wait_for_orphans as wait_for_search_orphans, wait_for_orphans_of as wait_for_search_orphans_of,
};
pub use pool::{SearchPoolLoad, search_pool_load};
pub use types::EngineDiagnostics;
