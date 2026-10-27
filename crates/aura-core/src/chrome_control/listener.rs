//! The relay's pipe: one connection at a time, from the relay installed beside this Core in this
//! Windows session only.
use super::{Inner, connection};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::net::windows::named_pipe::NamedPipeServer;

pub(super) async fn run(inner: Arc<Inner>) {
    loop {
        let server = match aura_windows_platform::create_secure_byte_pipe_server(
            &inner.options.pipe_name,
            true,
        ) {
            Ok(server) => server,
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };
        if server.connect().await.is_err() {
            continue;
        }
        if !is_relay(&server, &inner.options.relay_executable) {
            continue;
        }
        connection::serve(Arc::clone(&inner), server).await;
    }
}

/// The client must be the relay image, in this Core's own Windows session.
fn is_relay(server: &NamedPipeServer, relay: &Path) -> bool {
    let (Ok(client), Ok(own)) = (
        aura_windows_platform::named_pipe_client_identity(server),
        aura_windows_platform::snapshot_process(std::process::id()),
    ) else {
        return false;
    };
    client.executable_path.to_string_lossy().eq_ignore_ascii_case(&relay.to_string_lossy())
        && client.windows_session_id == own.windows_session_id
}
