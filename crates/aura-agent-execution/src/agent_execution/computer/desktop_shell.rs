//! A Shell or terminal call from a turn whose computer tools work in the 小窗. Commands run on the
//! user's own desktop, so a page, a browser or a folder window a script opens appears there,
//! outside the 小窗 the turn works in. Such a script is not run; the model is told to open the
//! program in the 小窗 instead.
use super::super::{AgentExecution, CallContext};
use aura_core_broker::agent_store::{StoreAccess, StoreStep};
use std::sync::Arc;

const NOTICE: &str = "computer.child_session_desktop: 这一轮的电脑操作在小窗里进行，命令却运行在用户自己的桌面上：这个脚本打开的网页或程序会出现在用户的桌面上，而不在小窗里。脚本没有运行。要在小窗里打开程序，请用 computer.open；要打开网页，先用 computer.open 打开浏览器，再在它的地址栏里输入网址。";

/// Programs that show a window on the desktop as they start.
const WINDOWED: [&str; 8] =
    ["chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "iexplore", "explorer"];
/// Commands that start the program they are given; `[Diagnostics.Process]::Start` is one too.
const LAUNCHERS: [&str; 3] = ["start-process", "saps", "start"];
/// What else opens an address it is given in the default browser.
const OPENERS: [&str; 3] = ["process]::start", "rundll32", "shellexecute"];

impl AgentExecution {
    /// What a Shell or terminal call gets instead of running when it would open a page or a
    /// window on the user's desktop while its turn's computer tools work in the 小窗.
    pub(in crate::agent_execution) async fn child_session_desktop_notice(
        self: &Arc<Self>,
        call: &CallContext,
    ) -> anyhow::Result<Option<String>> {
        let content = &call.operation.content;
        let script = match content.action_kind.as_str() {
            "shell.powershell" | "shell.cmd" => content.action["script"].as_str(),
            "terminal.write" => content.action["input"].as_str(),
            _ => None,
        };
        if !script.is_some_and(opens_on_the_desktop) {
            return Ok(None);
        }
        // Only a step offered the computer tools has the 小窗 to open things in.
        if !call
            .registry
            .catalog_entries
            .iter()
            .any(|e| e.canonical_name.as_str() == "computer.open")
        {
            return Ok(None);
        }
        let run = call.batch.invocation_identity.agent_run_id;
        let service = Arc::clone(self);
        let mode = self
            .section(
                StoreStep::run(run, "agent.dispatch.computer_mode"),
                StoreAccess::Read,
                move || {
                    let journal = service.journal.lock()?;
                    Ok(aura_persistence_v2::agent::application::configuration(
                        journal.database.connection(),
                        run,
                    )?
                    .request
                    .computer_mode)
                },
            )
            .await?;
        Ok(mode.is_some_and(|mode| !mode.is_foreground()).then(|| NOTICE.to_owned()))
    }
}

/// Whether a statement of the script hands an address to a command that opens it, starts a
/// browser or a folder window, or opens something in its default program with Invoke-Item.
pub(super) fn opens_on_the_desktop(script: &str) -> bool {
    aura_persistence_v2::execution::shell_result::opens_address(script)
        || script.to_ascii_lowercase().split(['\n', ';', '|', '&']).any(|statement| {
            if (statement.contains("http://") || statement.contains("https://"))
                && OPENERS.iter().any(|opener| statement.contains(opener))
            {
                return true;
            }
            let words = words(statement);
            let Some(first) = words.first() else { return false };
            if matches!(*first, "invoke-item" | "ii") || WINDOWED.contains(&program(first)) {
                return true;
            }
            words
                .iter()
                .position(|word| LAUNCHERS.contains(word) || word.ends_with("process]::start"))
                .is_some_and(|launcher| {
                    words[launcher + 1..].iter().any(|word| WINDOWED.contains(&program(word)))
                })
        })
}

/// A statement's words; a quoted path with spaces is one word.
fn words(statement: &str) -> Vec<&str> {
    let separator =
        |c: char| c.is_whitespace() || matches!(c, '(' | ')' | '{' | '}' | '=' | ',' | '@');
    let mut words = Vec::new();
    let mut rest = statement;
    loop {
        rest = rest.trim_start_matches(separator);
        let Some(first) = rest.chars().next() else { return words };
        let end = if matches!(first, '"' | '\'') {
            rest[1..].find(first).map_or(rest.len(), |close| close + 2)
        } else {
            rest.find(separator).unwrap_or(rest.len())
        };
        words.push(rest[..end].trim_matches(['"', '\'']));
        rest = &rest[end..];
    }
}

/// The program a word names: its file name without `.exe`.
fn program(word: &str) -> &str {
    let name = word.rsplit(['\\', '/']).next().unwrap_or(word);
    name.strip_suffix(".exe").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::opens_on_the_desktop;

    #[test]
    fn a_script_that_opens_a_page_or_starts_a_browser_or_a_folder_window_is_noticed() {
        for script in [
            "Start-Process chrome.exe https://www.douyin.com/",
            "Start-Process 'https://example.com'",
            "$p = Start-Process -FilePath msedge -PassThru",
            "& \"C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe\" --new-window",
            "start \"\" chrome",
            "cmd /c start firefox",
            "explorer.exe .",
            "ii .\\report.html",
            "if ($false) { Start-Process chrome.exe https://example.invalid/ }",
            "[System.Diagnostics.Process]::Start('https://example.com')",
            "[Diagnostics.Process]::Start(\"msedge\")",
            "rundll32 url.dll,FileProtocolHandler https://example.com",
            "(New-Object -ComObject Shell.Application).ShellExecute('https://example.com')",
        ] {
            assert!(opens_on_the_desktop(script), "{script}");
        }
    }

    #[test]
    fn a_script_that_only_looks_at_a_browser_or_starts_a_console_program_is_not() {
        for script in [
            "Get-Process chrome | Select-Object Id, MainWindowTitle",
            "Stop-Process -Name chrome",
            "Start-Process node -ArgumentList 'server.js'",
            "[System.Diagnostics.Process]::Start('node', 'server.js')",
            "start /b node server.js & curl http://localhost:3000",
            "Invoke-WebRequest https://example.com -OutFile page.html",
            "Write-Output 'open chrome later'",
            "git clone https://github.com/a/b.git",
        ] {
            assert!(!opens_on_the_desktop(script), "{script}");
        }
    }
}
