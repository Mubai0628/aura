//! A model that found its terminal taken over waits for the user to hand it back with an empty
//! `terminal.write`, at most its wait, and then reads what the terminal shows.
use super::*;

/// What the model is told while the user has its terminal.
pub(super) const TAKEN_OVER: &str = "用户已接管该终端，模型读取和写入已暂停。要等用户交还，调用 terminal.write，input 为空、wait_ms 为最多等待的毫秒数（不超过 30000）：用户交还时立即返回终端最近的输出，不必等用户另发消息。";
const STILL_HELD: &str = "用户仍在操作该终端（已接管），等待时间已到，没有写入任何内容；可以再用空输入的 terminal.write 继续等待交还，或先做别的。";
/// How much of the terminal's latest output the model reads when the user hands it back.
const LATEST: usize = 8192;
const POLL: Duration = Duration::from_millis(200);

impl AgentTerminals {
    /// Whether the user holds this session's terminal now; refused for another session's.
    pub(super) fn held_by_user(&self, session: SessionId, id: &str) -> Result<bool> {
        let record = self.record(session, id)?;
        Ok(record.view.end.is_none() && record.view.controller == TerminalControllerV1::User)
    }
}

/// Waits for the user to hand the terminal back, then reads its latest output.
pub(super) async fn handed_back(
    owner: Arc<AgentTerminals>,
    session: SessionId,
    id: String,
    wait_ms: u64,
) -> Result<AgentTerminalResultV1> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
    loop {
        let controller = owner
            .entry(&id)?
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?
            .controller;
        if controller == TerminalControllerV1::Agent {
            let reader = owner.clone();
            let terminal = id.clone();
            let latest = owner
                .store
                .run_critical(StoreStep::operation("terminal.read"), StoreAccess::Read, move || {
                    let end = reader.model_read(session, &terminal, u64::MAX, 1)?.next_offset;
                    reader.model_read(session, &terminal, end.saturating_sub(LATEST as u64), LATEST)
                })
                .await;
            // The live terminal changes hands a moment before its saved record does.
            match latest {
                Err(error)
                    if error.to_string().contains(TAKEN_OVER)
                        && tokio::time::Instant::now() < deadline => {}
                answer => return answer,
            }
        } else if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(STILL_HELD);
        }
        tokio::time::sleep_until(deadline.min(tokio::time::Instant::now() + POLL)).await;
    }
}
