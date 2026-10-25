//! A Shell call's row says what it ran. It showed only `.`, the working directory the model left to
//! its default, so the command was visible only in the audit view.
use serde_json::Value;

pub(super) const NAMES: [&str; 2] = ["shell.powershell", "shell.cmd"];
const PREVIEW_CHARS: usize = 120;
const SCRIPT_CHARS: usize = 4000;

fn script(arguments: Option<&Value>) -> Option<&str> {
    arguments?.get("script")?.as_str().map(str::trim).filter(|script| !script.is_empty())
}

/// The script's first line, cut to fit the row, with `…` when more follows.
pub(super) fn target(arguments: Option<&Value>) -> Option<String> {
    let mut lines = script(arguments)?.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = lines.next()?;
    let mut shown: String = first.chars().take(PREVIEW_CHARS).collect();
    if first.chars().count() > PREVIEW_CHARS {
        shown.push('…');
    } else if lines.next().is_some() {
        shown.push_str(" …");
    }
    Some(shown)
}

/// The whole script, and the working directory when the model chose one.
pub(super) fn details(arguments: Option<&Value>) -> Vec<String> {
    let mut details = vec![];
    if let Some(script) = script(arguments) {
        let mut shown: String = script.chars().take(SCRIPT_CHARS).collect();
        if script.chars().count() > SCRIPT_CHARS {
            shown.push('…');
        }
        details.push(format!("命令：{shown}"));
    }
    let directory = arguments.and_then(|value| value.get("working_directory")?.as_str());
    if let Some(directory) = directory.filter(|directory| !matches!(*directory, "" | ".")) {
        details.push(format!("目录：{}", directory.chars().take(256).collect::<String>()));
    }
    details
}
