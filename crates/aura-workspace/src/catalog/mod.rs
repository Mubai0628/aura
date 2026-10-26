//! The workspace path and text catalog.
//!
//! [`scan`] owns the one bounded directory scanner used by both the first full
//! index and the incremental update path in [`delta`], so ignore rules, reparse
//! policy and per-file text limits have exactly one implementation.
pub(super) mod scan;
pub use scan::{
    CatalogCommitHold, CatalogEntry, CatalogOpenReport, CatalogOptions, DecodedText, NameMatchMode,
    PathCatalog, decode_text, decode_text_with_encoding, encode_text,
};
pub(crate) mod delta;
pub use delta::{CatalogChange, CatalogUpdate, CatalogUpdateReport};
mod artifacts;
pub(crate) mod build;
pub use build::{CatalogBuildPhase, CatalogBuildStatus};
mod boundary;
mod fuzzy;
pub use boundary::children_beyond_index;
mod watch;
pub(crate) use watch::{WatchDescription, changes_for_watch_events};
pub(crate) mod reconcile;
pub use reconcile::CatalogReconcileReport;
#[cfg(test)]
#[path = "tests/watch_description.rs"]
mod watch_description_tests;

#[cfg(test)]
#[path = "tests/publication_artifacts.rs"]
mod publication_artifact_tests;

#[cfg(test)]
#[path = "tests/ignored_directories.rs"]
mod ignored_directory_tests;

#[cfg(test)]
#[path = "tests/background_build.rs"]
mod background_build_tests;

#[cfg(test)]
#[path = "tests/unfinished_index.rs"]
mod unfinished_index_tests;
