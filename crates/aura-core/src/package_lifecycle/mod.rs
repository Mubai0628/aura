//! V2 package lifecycle composition. Callers supply already resolved user inputs
//! and run blocking preparation on Core's bounded synchronous boundary.
pub(crate) mod materialize;
mod owner;
mod preparation;
mod recovery;
mod run;
mod types;
pub use owner::PackageConformanceOwner;
pub use preparation::PreparedPackageConformance;
pub use types::{PackageConformanceHandle, PackageRecovery, PackageRunOptions, PackageRunStatus};

#[cfg(test)]
mod tests;
