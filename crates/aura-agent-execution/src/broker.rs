//! The ledger's broker as the Agent execution uses it, with the two broker functions Core's
//! Main broker shares with it: a workspace file's verified facts and the Git read backend
//! descriptor.
pub use aura_core_broker::broker::*;
pub use journal::CompletionVerdict;
pub mod files;
pub mod git;
pub use git::descriptor as git_descriptor;
