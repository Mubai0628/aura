//! What a Provider's error reply means. One table, shared by every protocol adapter, turns the
//! reply's status, codes and words into a kind of failure and whether the same request may pass
//! when sent again; a Provider with wording of its own adds rows to the table in `rules.rs`,
//! not code.
mod classify;
mod rules;

pub use classify::{
    AccountSetting, ProviderErrorClass, ProviderErrorReply, account_setting,
    classify_provider_error,
};
#[doc(hidden)]
pub use rules::all_phrases;
