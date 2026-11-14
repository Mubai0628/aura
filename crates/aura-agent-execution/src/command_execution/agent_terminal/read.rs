//! `terminal.read`: a page of the lines a terminal shows. A terminal of an earlier Core has no
//! screen any more, so its saved text is read as lines instead.
use super::*;

/// How much of an earlier Core's terminal text is read: its newest part.
const SAVED_TAIL: u64 = 1024 * 1024;
/// The most a terminal's answer to `terminal.open`, `terminal.write` and `terminal.close` holds:
/// its lines, and room for what it says about the lines left out.
const ANSWER_LINES: usize = 200;
const ANSWER_BYTES: usize = 8192 - 256;

impl AgentTerminals {
    pub(super) fn model_lines(
        &self,
        session: SessionId,
        id: &str,
        line: Option<u64>,
        max_lines: usize,
    ) -> Result<AgentTerminalLinesV1> {
        ensure!(
            (1..=2000).contains(&max_lines),
            "terminal.read_invalid: max_lines 是 1 到 2000 之间的行数（这次是 {max_lines}），没有读取。"
        );
        let record = self.record(session, id)?;
        ensure!(record.view.controller == TerminalControllerV1::Agent, handback::TAKEN_OVER);
        let (page, view) = if self.is_live(id)? {
            let view = self.capture(session, id)?;
            let entry = self.entry(id)?;
            let live = entry.lock()?;
            let shown = live.lines.shown(&live.privacy);
            (live.lines.page(&shown, line, max_lines), view)
        } else {
            let text = self.saved_text(&record)?;
            (lines::ScreenLines::saved(&text).page(&[], line, max_lines), record.view)
        };
        Ok(AgentTerminalLinesV1 {
            terminal_id: id.to_owned(),
            lines: page.lines,
            first_line: page.first,
            last_line: page.last,
            next_line: page.next,
            earliest_line: page.earliest,
            running: view.end.is_none(),
            end: view.end,
        })
    }

    /// `output` as the screen shows it: the lines from line `from` on (where a write began), or
    /// the newest lines without it, at most a page, so a line redrawn in place reads as it ended.
    /// The lines left out are named with where `terminal.read` reads them. A terminal of an
    /// earlier Core keeps the text it saved.
    pub(super) fn on_screen(
        &self,
        id: &str,
        from: Option<u64>,
        mut output: AgentTerminalResultV1,
    ) -> Result<AgentTerminalResultV1> {
        if !self.is_live(id)? {
            return Ok(output);
        }
        let entry = self.entry(id)?;
        let page = {
            let live = entry.lock()?;
            let shown = live.lines.shown(&live.privacy);
            // A screen cleared since the write began shows its last line.
            let from = from.map(|line| line.min(live.lines.last_line(&shown)));
            live.lines.page_within(&shown, from, ANSWER_LINES, ANSWER_BYTES)
        };
        let earlier = if from.is_none() { page.first.saturating_sub(page.earliest) } else { 0 };
        let mut text = page.lines;
        if earlier > 0 {
            text.insert_str(
                0,
                &format!("[前面还有 {earlier} 行没有显示，可以用 terminal.read 读]\n"),
            );
        }
        if let Some(next) = page.next {
            let left = page.last + 1 - next;
            text.push_str(&format!(
                "\n[后面还有 {left} 行没有显示，可以用 terminal.read 从第 {next} 行（line: {next}）接着读]"
            ));
        }
        output.truncated = earlier > 0 || page.next.is_some();
        output.output = text;
        Ok(output)
    }

    /// The newest [`SAVED_TAIL`] bytes of a terminal's saved text, from the start of a line.
    fn saved_text(&self, record: &AgentTerminalRecord) -> Result<String> {
        let journal = self.journal.lock()?;
        let db = journal.database.connection();
        let end = records::read(db, record, u64::MAX, 1)?.next_offset;
        let mut offset = end.saturating_sub(SAVED_TAIL);
        let mut text = String::new();
        let mut cut = false;
        loop {
            let page = records::read(db, record, offset, 65536)?;
            cut |= text.is_empty() && page.start_offset > 0;
            text.push_str(&page.output);
            if page.output.is_empty() || page.next_offset >= end {
                break;
            }
            offset = page.next_offset;
        }
        if cut {
            let from = text.find('\n').map_or(text.len(), |at| at + 1);
            text.replace_range(..from, "[更早的输出没有读出]\n");
        }
        Ok(text)
    }
}
