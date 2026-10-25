#![forbid(unsafe_code)]

mod runtime_commands;

use anyhow::Result;
use aura_persistence::PersistenceCoordinator;
use tokio::net::windows::named_pipe::NamedPipeClient;

/// Serves the already-authenticated Core command channel.
///
/// Authentication and process-identity binding are deliberately owned by the
/// binary bootstrap. Keeping this command loop in the library lets controlled
/// Core integration tests exercise the exact production dispatcher without
/// creating a second, test-only command implementation.
pub async fn serve_authenticated_core_commands(
    pipe: NamedPipeClient,
    persistence: PersistenceCoordinator,
) -> Result<()> {
    runtime_commands::serve(pipe, persistence).await
}
