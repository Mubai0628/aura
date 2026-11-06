//! Waiting for an opening's supervisor on its pipe. The pipe is open to all of the user's
//! sessions, so who connected is checked before anything else is read: the host program beside
//! Core, running in the child session, saying the opening's token in its first line. Any other
//! client is disconnected, and the pipe waits again until the time is up.
//!
//! The pipe is made by [`listen`] before the startup entry that starts the supervisor is written,
//! so that the supervisor never finds it missing.
use super::ChildOpening;
use aura_desktop_domain::child::{
    CHILD_PROTOCOL_VERSION, ChildLine, SupervisorSays, read_child_line,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt as _;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::time::Instant;

/// How long a connected supervisor has to say hello.
pub const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// The longest hello read.
const HELLO_BYTES: usize = 1024;

/// Who may connect to an opening's pipe.
#[derive(Clone)]
pub struct Expected {
    /// The host program beside Core.
    pub program: PathBuf,
    /// The session the supervisor must run in, as Windows reports it when it connects: the child
    /// session of Core's session.
    pub session: Arc<dyn Fn() -> Option<u32> + Send + Sync>,
}

/// A supervisor that was let in: its pipe, and what it wrote after its hello.
pub struct Supervisor {
    pub(super) pipe: NamedPipeServer,
    pub(super) rest: Vec<u8>,
    session: u32,
}

impl Supervisor {
    pub fn session(&self) -> u32 {
        self.session
    }
}

/// An opening's pipe, made and waiting for its supervisor.
pub struct Listening {
    opening: ChildOpening,
    pipe: NamedPipeServer,
}

/// Makes the pipe of `opening`.
///
/// # Errors
///
/// When Windows does not make it, such as when another program holds that name.
pub fn listen(opening: &ChildOpening) -> Result<Listening, String> {
    Ok(Listening { opening: opening.clone(), pipe: make(opening)? })
}

fn make(opening: &ChildOpening) -> Result<NamedPipeServer, String> {
    aura_windows_platform::create_user_byte_pipe_server(opening.pipe())
        .map_err(|error| format!("没能建立子会话的管道：{error}"))
}

impl Listening {
    /// Waits up to `within` for the opening's supervisor.
    ///
    /// # Errors
    ///
    /// Why none was let in: the time was up, with why the last client was turned away, or the
    /// pipe could not be made again after one was.
    pub async fn accept(self, expected: &Expected, within: Duration) -> Result<Supervisor, String> {
        let until = Instant::now() + within;
        let mut pipe = self.pipe;
        let mut turned_away = None;
        loop {
            match tokio::time::timeout_at(until, pipe.connect()).await {
                Err(_) => {
                    return Err(match turned_away {
                        Some(why) => format!("子会话的看守没有连上：{why}"),
                        None => "子会话的看守没有连上".to_owned(),
                    });
                }
                Ok(Err(error)) => turned_away = Some(format!("连接出错：{error}")),
                Ok(Ok(())) => match admit(pipe, &self.opening, expected).await {
                    Ok(supervisor) => return Ok(supervisor),
                    Err(why) => turned_away = Some(why),
                },
            }
            pipe = make(&self.opening)?;
        }
    }
}

async fn admit(
    mut pipe: NamedPipeServer,
    opening: &ChildOpening,
    expected: &Expected,
) -> Result<Supervisor, String> {
    let client = aura_windows_platform::named_pipe_client_identity(&pipe)
        .map_err(|error| format!("认不出连上来的程序：{error}"))?;
    let program = client.executable_path.to_string_lossy();
    if !program.eq_ignore_ascii_case(&expected.program.to_string_lossy()) {
        return Err(format!("连上来的不是 Aura 的操控主机：{program}"));
    }
    if (expected.session)() != Some(client.windows_session_id) {
        return Err(format!("连上来的程序不在子会话里（会话 {}）", client.windows_session_id));
    }
    let (hello, rest) = tokio::time::timeout(HELLO_WITHIN, first_line(&mut pipe))
        .await
        .map_err(|_| "看守没有及时说明自己".to_owned())??;
    match read_child_line::<SupervisorSays>(&hello) {
        Ok(ChildLine::Supervisor(SupervisorSays::Hello { token, protocol })) => {
            if protocol != CHILD_PROTOCOL_VERSION {
                Err(format!("看守的协议版本 {protocol} 不对"))
            } else if token != opening.token() {
                Err("看守的口令不是这次打开的".to_owned())
            } else {
                Ok(Supervisor { pipe, rest, session: client.windows_session_id })
            }
        }
        _ => Err("看守的第一行不是问候".to_owned()),
    }
}

/// The first line, without its break, and what came after it in the same reads.
async fn first_line(pipe: &mut NamedPipeServer) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut read = Vec::new();
    let mut chunk = [0_u8; 256];
    loop {
        if let Some(end) = read.iter().position(|byte| *byte == b'\n') {
            let rest = read.split_off(end + 1);
            read.pop();
            return Ok((read, rest));
        }
        if read.len() > HELLO_BYTES {
            return Err("看守的第一行太长".to_owned());
        }
        let count = pipe.read(&mut chunk).await.map_err(|error| format!("读不到看守：{error}"))?;
        if count == 0 {
            return Err("看守断开了".to_owned());
        }
        read.extend_from_slice(&chunk[..count]);
    }
}
