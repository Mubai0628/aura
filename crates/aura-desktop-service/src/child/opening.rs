//! One opening of the child session: a pipe name and a token nobody else knows, and the command
//! line that starts its supervisor.
use aura_desktop_domain::child::{CHILD_PIPE_PREFIX, CHILD_SWITCH};
use std::fmt::Write as _;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildOpening {
    pipe: String,
    token: String,
}

impl ChildOpening {
    /// A new opening, from Windows' random numbers.
    ///
    /// # Errors
    ///
    /// When Windows gives no random numbers.
    pub fn new() -> Result<Self, String> {
        let mut bytes = [0_u8; 48];
        getrandom::fill(&mut bytes).map_err(|error| format!("没有拿到随机数：{error}"))?;
        let (pipe, token) = bytes.split_at(16);
        Ok(Self { pipe: format!("{CHILD_PIPE_PREFIX}{}", hex(pipe)), token: hex(token) })
    }

    #[must_use]
    pub fn pipe(&self) -> &str {
        &self.pipe
    }

    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// What Windows runs in the child session: the host program as the opening's supervisor.
    #[must_use]
    pub fn command_line(&self, program: &Path) -> String {
        format!("\"{}\" {CHILD_SWITCH} {} {}", program.display(), self.pipe, self.token)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
