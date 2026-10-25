//! Only the remote Provider is substituted. It receives real compiled requests
//! over a normally configured loopback Endpoint and returns ordinary SSE.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
pub struct Provider {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub raw_requests: Arc<Mutex<Vec<Vec<u8>>>>,
    pub release_window: Arc<Notify>,
    /// Signalled after a parked scenario has frozen its first request and before
    /// that request is answered.
    pub parked: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}
/// The token the external-read fixture file contains, so the test can assert the
/// real result reached the next model request.
pub const EXTERNAL_BODY_TOKEN: &str = "AURA_EXTERNAL_READ_BODY";
/// The file name the external-list fixture directory contains.
pub const EXTERNAL_LIST_TOKEN: &str = "aura_external_list_token";
/// The exact bytes the Responses binding fixture writes in its first step. A
/// second dispatch of the same call would be visible as a different file.
pub const RESPONSES_WRITE_TOKEN: &str = "written exactly once by the first model step";
/// The Provider call ID of that one write, so a later request can be checked for
/// the result of the call it already completed.
pub const RESPONSES_WRITE_CALL_ID: &str = "call_write_once";
/// The text the external-write fixture asks the model to write, so the test can
/// assert the real bytes landed outside the workspace.
pub const EXTERNAL_WRITE_TOKEN: &str = "aura_external_write_body";
impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Provider {
    pub async fn new(write: bool) -> Self {
        Self::scenario(if write { Scenario::Write } else { Scenario::Text }).await
    }
}
mod chat_reply;
mod responses;
mod server;
use responses::*;
mod wire;
use wire::*;
pub(super) use wire::{flat_call, recovery_response, send_with, state};
mod scenario;
use super::activity_provider;
pub use scenario::{
    COMPACTION_SOURCE_FILE, COMPACTION_TOOL_FILE, HOOK_SOURCE, INIT_COMMAND, INIT_EVIDENCE_FILE,
    Scenario,
};
