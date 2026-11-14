//! Search every new plain-text page while preserving the bounded model response.
use super::*;
use std::time::Duration;

struct Search {
    needle: String,
    tail: String,
    offset: u64,
    /// The typed line's echo has not ended yet: text it names, as `echo DONE` names DONE, is no
    /// answer.
    echo: bool,
}
impl Search {
    fn page(&mut self, page: &AgentTerminalResultV1) -> bool {
        // Retention may advance while a slow call is waiting. Never join text
        // across a discarded range and accidentally manufacture a match.
        if page.start_offset > self.offset {
            self.tail.clear();
        }
        let mut text = page.output.as_str();
        if self.echo {
            match text.find('\n') {
                Some(at) => {
                    text = &text[at + 1..];
                    self.echo = false;
                }
                None => text = "",
            }
        }
        self.tail.push_str(text);
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

/// How long the output stays unchanged before a wait without `until_text` returns it when no
/// prompt came back: long enough for a command that is silent for a moment before it prints.
const SETTLED: Duration = Duration::from_millis(3000);

/// What a write typed, which decides whether a shell's prompt in the output answers it and where
/// an `until_text` is looked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Typed {
    /// Nothing, as when a terminal opens or only Ctrl+C is sent: any prompt answers.
    Nothing,
    /// Text with no line end, which stays on the prompt's line: only a prompt after a line break
    /// answers.
    Partial,
    /// One command line, ended: it is echoed on the line of the prompt it was typed at, so only a
    /// prompt after a line break answers, and an `until_text` counts only after that echo.
    Line,
    /// Several lines, or input after Ctrl+C: the shell shows a prompt between commands, after an
    /// interrupt and inside an open statement, so only the quiet answers. The first line's echo is
    /// skipped as for `Line`.
    Lines,
}

impl Typed {
    /// `text` is the input as written to the terminal, with each line ending in `\r`, after Ctrl+C
    /// when `interrupt` is set.
    pub(super) fn of(text: &str, interrupt: bool) -> Self {
        match text.matches('\r').count() {
            _ if text.is_empty() => Self::Nothing,
            _ if interrupt => Self::Lines,
            0 => Self::Partial,
            1 if text.ends_with('\r') => Self::Line,
            _ => Self::Lines,
        }
    }

    /// Whether the output starts with the echo of a line the shell took.
    fn echoes(self) -> bool {
        matches!(self, Self::Line | Self::Lines)
    }
}

/// Whether `text`, the output since a write, ends on a shell's prompt: an unfinished last line
/// ending in `>`, `$` or `#`, as PowerShell's, CMD's, a Unix shell's over SSH and most REPLs' do.
fn ends_at_prompt(text: &str, typed: Typed) -> bool {
    let (broken, line) = match text.rfind('\n') {
        Some(at) => (true, &text[at + 1..]),
        None => (false, text),
    };
    let line = line.trim_end();
    let placed = match typed {
        Typed::Nothing => true,
        Typed::Partial | Typed::Line => broken,
        Typed::Lines => false,
    };
    placed && !line.trim_start().is_empty() && line.ends_with(['>', '$', '#'])
}

/// Waits for the output after `offset`: until `until` appears after the typed line's echo when
/// given, otherwise until the shell's prompt is back or the output has been quiet for [`SETTLED`];
/// never past `wait_ms`, and at once when the terminal ends or more than a page arrived.
pub(super) async fn wait(
    owner: Arc<AgentTerminals>,
    session: SessionId,
    id: String,
    offset: u64,
    wait_ms: u64,
    until: Option<String>,
    typed: Typed,
) -> Result<AgentTerminalResultV1> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
    let settles = until.is_none();
    let mut seen = offset;
    let mut quiet_since = tokio::time::Instant::now();
    let mut search =
        until.map(|needle| Search { needle, tail: String::new(), offset, echo: typed.echoes() });
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
        let now = tokio::time::Instant::now();
        if output.next_offset != seen {
            seen = output.next_offset;
            quiet_since = now;
        }
        let settled = quiet_since + SETTLED;
        if !output.running
            || found
            || (search.is_none() && output.truncated)
            || now >= deadline
            || (settles && (now >= settled || ends_at_prompt(&output.output, typed)))
        {
            return Ok(output);
        }
        let Some(changed) = &mut changed else {
            return Ok(output);
        };
        let wake = if settles { deadline.min(settled) } else { deadline };
        // On a timeout the output is read once more, then the deadline or the quiet ends the wait.
        if tokio::time::timeout_at(wake, changed.changed()).await.is_err() && wake == deadline {
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
    fn a_shells_prompt_ends_the_wait_once_typed_input_was_accepted() {
        use Typed::{Line, Lines, Nothing};
        // PowerShell, CMD, a Unix shell over SSH and a REPL, each after a line it accepted.
        assert!(ends_at_prompt("Write-Output x\nx\nPS D:\\work> ", Line));
        assert!(ends_at_prompt("call slow.cmd\r\nDONE\r\n\r\nC:\\work>", Line));
        assert!(ends_at_prompt("ls\nnotes.md\nuser@host:~$ ", Line));
        assert!(ends_at_prompt("whoami\nroot\nroot@host:/# ", Line));
        assert!(ends_at_prompt("1 + 1\n2\n>>> ", Line));
        // A statement left open waits for its next line at a continuation prompt.
        assert!(ends_at_prompt("echo 'abc\n>> ", Line));
        // A terminal just opened, or interrupted, shows its prompt with no line typed before it.
        assert!(ends_at_prompt("PS D:\\work> ", Nothing));
        // Typed input is still being echoed on the prompt's line, or the command is running.
        assert!(!ends_at_prompt("echo a >", Line));
        assert!(!ends_at_prompt("call slow.cmd\n", Line));
        assert!(!ends_at_prompt("Download\n50%", Line));
        assert!(!ends_at_prompt("<b>\n", Line));
        // A program asking for input waits for the quiet instead.
        assert!(!ends_at_prompt("ssh host\nPassword: ", Line));
        assert!(!ends_at_prompt("", Nothing));
        // Between several typed lines the shell shows a prompt before it takes the next one.
        assert!(!ends_at_prompt("cd x\nPS D:\\x> ", Lines));
    }

    #[test]
    fn what_a_write_typed_is_counted_in_command_lines() {
        assert_eq!(Typed::of("", false), Typed::Nothing);
        assert_eq!(Typed::of("", true), Typed::Nothing);
        assert_eq!(Typed::of("y", false), Typed::Partial);
        assert_eq!(Typed::of("dir\r", false), Typed::Line);
        assert_eq!(Typed::of("cd x\rdir\r", false), Typed::Lines);
        assert_eq!(Typed::of("cd x\rdi", false), Typed::Lines);
        // Ctrl+C brings a new prompt before the line is taken.
        assert_eq!(Typed::of("dir\r", true), Typed::Lines);
        assert!(Typed::Line.echoes() && Typed::Lines.echoes());
        assert!(!Typed::Nothing.echoes() && !Typed::Partial.echoes());
        // Text with no line end stays on the prompt's line, as a line still being echoed does.
        assert!(!ends_at_prompt("echo a >", Typed::Partial));
        assert!(ends_at_prompt("y\nDone\nPS D:\\work> ", Typed::Partial));
    }

    #[test]
    fn an_until_text_is_looked_for_after_the_echo_of_the_typed_line() {
        let search =
            |echo| Search { needle: "MARK-DONE".into(), tail: String::new(), offset: 0, echo };
        // The echo names the text across two pages; the command prints it on a line of its own.
        let mut typed = search(true);
        assert!(!typed.page(&page("call mark.cmd MARK", 0)));
        assert!(!typed.page(&page("-DONE\r\n", 18)));
        assert!(typed.page(&page("MARK-DONE\r\n", 25)));
        // With nothing typed there is no echo, and the text counts from the start.
        let mut waited = search(false);
        assert!(waited.page(&page("MARK-DONE\r\n", 0)));
    }

    #[test]
    fn l03_b0_until_text_matches_utf8_across_pages_and_never_across_retention_gaps() {
        let search = |needle: &str| Search {
            needle: needle.into(),
            tail: String::new(),
            offset: 0,
            echo: false,
        };
        let mut search_utf8 = search("中文完成");
        assert!(!search_utf8.page(&page("起始中文", 0)));
        assert!(search_utf8.page(&page("完成", 12)));
        let mut search_gap = search("AB");
        assert!(!search_gap.page(&page("A", 0)));
        assert!(!search_gap.page(&page("B", 2)));
    }
}
