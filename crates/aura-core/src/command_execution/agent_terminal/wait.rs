//! Search every new plain-text page while preserving the bounded model response.
use super::*;
use std::time::Duration;

struct Search {
    needle: String,
    tail: String,
    offset: u64,
}
impl Search {
    fn page(&mut self, page: &AgentTerminalResultV1) -> bool {
        // Retention may advance while a slow call is waiting. Never join text
        // across a discarded range and accidentally manufacture a match.
        if page.start_offset > self.offset {
            self.tail.clear();
        }
        self.tail.push_str(&page.output);
        let found = self.tail.contains(&self.needle);
        let keep = self.needle.len().saturating_sub(1);
        let mut start = self.tail.len().saturating_sub(keep);
        while start < self.tail.len() && !self.tail.is_char_boundary(start) {
            start += 1;
        }
        self.tail.drain(..start);
        self.offset = page.next_offset;
        found
    }
}

pub(super) async fn wait(
    owner: Arc<AgentTerminals>,
    session: SessionId,
    id: String,
    offset: u64,
    wait_ms: u64,
    until: Option<String>,
) -> Result<AgentTerminalResultV1> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
    let mut search = until.map(|needle| Search { needle, tail: String::new(), offset });
    let mut changed = {
        let live = owner.entry(&id)?;
        let live = live.lock().map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?;
        live.terminal.as_ref().map(|terminal| terminal.subscribe_output())
    };
    loop {
        let reader = owner.clone();
        let terminal_id = id.clone();
        let (output, updated, found) = owner
            .store
            .run_critical(
                StoreStep::operation("terminal.wait.output"),
                StoreAccess::Write,
                move || {
                    let mut output = reader.model_read(session, &terminal_id, offset, 8192)?;
                    let mut found = false;
                    if let Some(search) = &mut search {
                        loop {
                            // model_read enforces controller/session ownership and
                            // captures only VT-stripped, sensitive-input-masked text.
                            let page =
                                reader.model_read(session, &terminal_id, search.offset, 65536)?;
                            let previous = search.offset;
                            found = search.page(&page);
                            if found
                                || !page.truncated
                                || search.offset == previous
                                || tokio::time::Instant::now() >= deadline
                            {
                                break;
                            }
                        }
                        output = reader.model_read(session, &terminal_id, offset, 8192)?;
                    }
                    Ok((output, search, found))
                },
            )
            .await?;
        search = updated;
        if !output.running
            || found
            || (search.is_none() && output.truncated)
            || tokio::time::Instant::now() >= deadline
        {
            return Ok(output);
        }
        let Some(changed) = &mut changed else {
            return Ok(output);
        };
        if tokio::time::timeout_at(deadline, changed.changed()).await.is_err() {
            return Ok(output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(text: &str, start: u64) -> AgentTerminalResultV1 {
        AgentTerminalResultV1 {
            terminal_id: "fixture".into(),
            output: text.into(),
            start_offset: start,
            next_offset: start + text.len() as u64,
            truncated: false,
            running: true,
            end: None,
        }
    }
    #[test]
    fn l03_b0_until_text_matches_utf8_across_pages_and_never_across_retention_gaps() {
        let mut search = Search { needle: "中文完成".into(), tail: String::new(), offset: 0 };
        assert!(!search.page(&page("起始中文", 0)));
        assert!(search.page(&page("完成", 12)));
        let mut search = Search { needle: "AB".into(), tail: String::new(), offset: 0 };
        assert!(!search.page(&page("A", 0)));
        assert!(!search.page(&page("B", 2)));
    }
}
