//! What older versions and other installations left in Aura's folders and nothing uses any more,
//! for the user to remove: the layout of the versions before namespaces, the folders of a
//! namespace no installation uses, and removals that did not finish. Finding only reads; removing
//! finds again and takes only what is still found.
//!
//! Never found: the namespace of the Aura that asks, the released product's, the ones the
//! installations in Settings > Apps use, the Core log every installation at the default places
//! writes, and any folder that holds a name Aura does not write, a link, or a working copy.
mod contract;
mod find;
mod installed;
mod names;
mod remove;
mod walk;

pub use contract::{
    OldDataEntry, OldDataFailure, OldDataKind, OldDataListing, OldDataRemoval, OldDataRemoveRequest,
};
pub use find::{Kind, OldData, find};
pub use installed::Installed;
pub use names::REMOVING_SUFFIX;
pub use remove::{Removed, remove};

use aura_data_location::Roots;
use aura_runtime_contract::BundleNamespace;

/// Where to look and what is in use.
#[derive(Clone, Copy, Debug)]
pub struct Scope<'a> {
    pub roots: &'a Roots,
    /// The namespace of the Aura that asks, or the one the setup program just installed.
    pub current: &'a BundleNamespace,
    pub installed: &'a Installed,
}
