//! How Aura signs in to MCP servers. Secrets the user entered and (later) OAuth
//! tokens are a credential authority of their own: they share the platform secret store and
//! the dedicated secret input with the model and tool credentials, never their records.
mod secrets;
mod source;
mod store;

pub(crate) use secrets::McpSecrets;
pub(crate) use source::{
    CredentialSource, Placement, RequestCredential, StaticSecret, origin, request_credential,
};
#[cfg(any(test, feature = "test-hooks"))]
pub use store::MemorySecretStore;
pub use store::{SecretStore, WindowsSecretStore};
