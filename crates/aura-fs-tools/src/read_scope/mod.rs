mod scope;
pub use scope::{FileReadScope, FileReadScopeGuard};
pub(crate) use scope::{allows, current, unavailable};
