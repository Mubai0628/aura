//! One relay run: reach the Core installed beside this program, then pass frames both ways until
//! either side closes.
use crate::frame::{read_frame, write_frame};
use anyhow::{Context, Result, ensure};
use aura_chrome_domain::protocol::{
    CHROME_PROTOCOL_VERSION, HostHello, MAX_FROM_CHROME_BYTES, MAX_TO_CHROME_BYTES, pipe_name,
    plain_path,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::NamedPipeClient;

/// Where the relay goes and which program must be serving there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayTarget {
    pub pipe_name: String,
    pub expected_server: PathBuf,
}

/// The pipe of the Core installed in the same folder as this relay, from the relay's canonical
/// path; Core names the pipe from its own the same way.
#[must_use]
pub fn target_beside(relay_executable: &Path) -> RelayTarget {
    let relay_executable = plain_path(relay_executable);
    let folder = relay_executable.parent().unwrap_or(&relay_executable);
    RelayTarget { pipe_name: pipe_name(folder), expected_server: folder.join("aura-core.exe") }
}

/// Core is between two connections for a moment; a busy or missing pipe is tried again briefly.
async fn connect(name: &str) -> Result<NamedPipeClient> {
    let mut last = None;
    for _ in 0..40 {
        match aura_windows_platform::open_secure_byte_pipe_client(name) {
            Ok(pipe) => return Ok(pipe),
            Err(error) => last = Some(error),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(last.map_or_else(|| anyhow::anyhow!("chrome_host.core_unreachable"), Into::into))
        .context("chrome_host.core_unreachable")
}

/// The server must be the expected Core image, in this Windows session.
fn check_server(pipe: &NamedPipeClient, expected: &Path) -> Result<()> {
    let server = aura_windows_platform::named_pipe_server_identity(pipe)
        .context("chrome_host.server_identity_unavailable")?;
    let own = aura_windows_platform::snapshot_process(std::process::id())
        .context("chrome_host.own_identity_unavailable")?;
    let same_image =
        server.executable_path.to_string_lossy().eq_ignore_ascii_case(&expected.to_string_lossy());
    ensure!(
        same_image && server.windows_session_id == own.windows_session_id,
        "chrome_host.server_not_aura_core"
    );
    Ok(())
}

/// Relays between Chrome (`from_chrome`, `to_chrome`) and Core until either side closes.
pub async fn relay<I, O>(
    origin: &str,
    target: &RelayTarget,
    mut from_chrome: I,
    mut to_chrome: O,
) -> Result<()>
where
    I: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
{
    let pipe = connect(&target.pipe_name).await?;
    check_server(&pipe, &target.expected_server)?;
    let (mut from_core, mut to_core) = tokio::io::split(pipe);
    let hello = serde_json::to_vec(&HostHello {
        origin: origin.to_owned(),
        protocol: CHROME_PROTOCOL_VERSION,
    })?;
    write_frame(&mut to_core, &hello, MAX_FROM_CHROME_BYTES).await?;
    let upward = async {
        while let Some(frame) = read_frame(&mut from_chrome, MAX_FROM_CHROME_BYTES).await? {
            write_frame(&mut to_core, &frame, MAX_FROM_CHROME_BYTES).await?;
        }
        Ok::<_, std::io::Error>(())
    };
    let downward = async {
        while let Some(frame) = read_frame(&mut from_core, MAX_TO_CHROME_BYTES).await? {
            write_frame(&mut to_chrome, &frame, MAX_TO_CHROME_BYTES).await?;
        }
        Ok::<_, std::io::Error>(())
    };
    tokio::select! {
        result = upward => result?,
        result = downward => result?,
    }
    Ok(())
}
