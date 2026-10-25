//! V2 package authority. Import keeps immutable content but grants no trust or enablement.
pub mod catalog;
pub mod conformance;
mod content;
pub mod credentials;
mod imports;
mod install;
pub mod lifecycle;
pub mod references;
mod settings;
pub use content::{LoadedPackageContent, PackageFileBytes, load_content};
pub use imports::import_content;
pub use install::install;
pub use settings::installed_settings;

mod state;
use state::revision_from_sql;
pub use state::{PackageState, state};
