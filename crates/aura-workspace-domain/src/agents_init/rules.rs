//! Rules that need no file system: structure, sizes, links, local machine state and
//! hedged commands. Core then checks evidence, link targets and secrets.
use super::markdown::{self, has_scheme};
use super::render::{self, AGENTS_PATH, CHAPTERS, DOCUMENT_TITLE, RenderedFile};
use super::{InitEvidence, InitShell, WorkspaceInitCompleteInput as Input};
use aura_domain_common::ToolCallId;
use std::collections::BTreeSet;
use std::fmt;

pub const MAX_ROOT_BYTES: usize = 64 * 1024;
pub const MAX_CHAPTER_BYTES: usize = 16 * 1024;
pub const MAX_GUIDE_BYTES: usize = 32 * 1024;
pub const MAX_GUIDES: usize = 8;
pub const MAX_COMMANDS: usize = 60;
const MAX_COMMAND_CHARS: usize = 400;
const MAX_LINE_CHARS: usize = 300;
const HEDGES_ZH: [&str; 6] = ["可能", "通常", "大概", "也许", "或许", "一般来说"];
const HEDGES_EN: [&str; 6] = ["maybe", "probably", "usually", "might", "perhaps", "possibly"];

/// What Core still checks against the workspace before anything is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitPlan {
    /// Guides first, the root file last: the write order.
    pub files: Vec<RenderedFile>,
    pub links: Vec<InitLink>,
    pub repository_evidence: Vec<RepositoryEvidence>,
    pub executed_evidence: Vec<ExecutedEvidence>,
    /// Workspace-relative directories the commands run in.
    pub directories: BTreeSet<String>,
}

/// A workspace-relative link target and the file that links to it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct InitLink {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryEvidence {
    pub field: String,
    pub path: String,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutedEvidence {
    pub field: String,
    pub tool_call_id: ToolCallId,
    pub command: String,
    /// The shell the command is documented for; the run must have used it.
    pub shell: InitShell,
    /// The documented workspace-relative directory; the run must have used it.
    pub directory: String,
}

/// One broken rule, named precisely enough for the model to fix it and resubmit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitViolation {
    pub field: String,
    pub rule: &'static str,
    pub detail: String,
}
impl fmt::Display for InitViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.field, self.rule, self.detail)
    }
}

#[derive(Default)]
struct Violations(Vec<InitViolation>);
impl Violations {
    fn add(&mut self, field: impl Into<String>, rule: &'static str, detail: impl Into<String>) {
        self.0.push(InitViolation { field: field.into(), rule, detail: detail.into() });
    }
}

/// Everything the rules found: the rendered files and links, and every violation. The
/// files are rendered even when rules are broken, so the workspace checks can still report
/// their own problems in the same answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitCheck {
    pub plan: InitPlan,
    pub violations: Vec<InitViolation>,
}

/// The plan when no rule is broken, otherwise every violation.
pub fn check(input: &Input, forbidden: &[String]) -> Result<InitPlan, Vec<InitViolation>> {
    let InitCheck { plan, violations } = inspect(input, forbidden);
    if violations.is_empty() { Ok(plan) } else { Err(violations) }
}

/// Checks everything that does not need the workspace and renders the files.
///
/// `forbidden` holds local facts that must not appear in the files, such as the Windows
/// user name, the profile directory and the workspace's absolute path; Core supplies them.
pub fn inspect(input: &Input, forbidden: &[String]) -> InitCheck {
    let mut v = Violations::default();
    for (field, body) in [
        ("project_structure", &input.project_structure),
        ("coding_style", &input.coding_style),
        ("testing", &input.testing),
        ("commits_and_pull_requests", &input.commits_and_pull_requests),
    ] {
        text_block(&mut v, field, body, true, 3, MAX_CHAPTER_BYTES);
    }
    text_block(
        &mut v,
        "commands.introduction",
        &input.commands.introduction,
        false,
        3,
        MAX_CHAPTER_BYTES,
    );
    text_block(&mut v, "commands.notes", &input.commands.notes, false, 3, MAX_CHAPTER_BYTES);
    let entries = &input.commands.entries;
    // A workspace with no command yet says so instead of listing one.
    if entries.is_empty()
        && render::normalize(&input.commands.introduction).is_empty()
        && render::normalize(&input.commands.notes).is_empty()
    {
        v.add(
            "commands.entries",
            "required",
            "list at least one command the repository defines or that you ran successfully; if the workspace has none yet, say so in commands.introduction",
        );
    }
    if entries.len() > MAX_COMMANDS {
        v.add("commands.entries", "too_many", format!("at most {MAX_COMMANDS} commands"));
    }
    let mut plan = InitPlan {
        files: Vec::new(),
        links: Vec::new(),
        repository_evidence: Vec::new(),
        executed_evidence: Vec::new(),
        directories: BTreeSet::new(),
    };
    for (index, command) in entries.iter().enumerate() {
        let field = |name: &str| format!("commands.entries[{index}].{name}");
        line(&mut v, &field("command"), &command.command, MAX_COMMAND_CHARS, true);
        line(&mut v, &field("purpose"), &command.purpose, MAX_LINE_CHARS, true);
        line(&mut v, &field("prerequisites"), &command.prerequisites, MAX_LINE_CHARS, false);
        hedged(&mut v, &field("purpose"), &command.purpose);
        hedged(&mut v, &field("prerequisites"), &command.prerequisites);
        let directory = match relative_path(command.working_directory.trim(), "") {
            Ok(directory) => {
                plan.directories.insert(directory.clone());
                directory
            }
            Err(detail) => {
                v.add(field("working_directory"), "workspace_relative", detail);
                ".".to_owned()
            }
        };
        let text = command.command.trim().to_owned();
        match &command.evidence {
            InitEvidence::RepositoryDefined { path } => match relative_path(path.trim(), "") {
                Ok(path) if path != "." => plan.repository_evidence.push(RepositoryEvidence {
                    field: field("evidence"),
                    path,
                    command: text,
                }),
                Ok(_) => v.add(
                    field("evidence.path"),
                    "workspace_file",
                    "name the file that contains the command, not the workspace root",
                ),
                Err(detail) => v.add(field("evidence.path"), "workspace_relative", detail),
            },
            InitEvidence::ExecutionVerified { tool_call_id } => {
                plan.executed_evidence.push(ExecutedEvidence {
                    field: field("evidence"),
                    tool_call_id: *tool_call_id,
                    command: text,
                    shell: command.shell,
                    directory,
                });
            }
        }
    }
    if input.guides.len() > MAX_GUIDES {
        v.add("guides", "too_many", format!("at most {MAX_GUIDES} guides"));
    }
    let mut names = BTreeSet::new();
    for (index, guide) in input.guides.iter().enumerate() {
        let field = |name: &str| format!("guides[{index}].{name}");
        let name = guide.name.trim();
        if !guide_name(name) {
            v.add(
                field("name"),
                "guide_name",
                "use 1-63 lowercase letters, digits or hyphens, starting with a letter or digit",
            );
        } else if name.eq_ignore_ascii_case("agents") {
            v.add(
                field("name"),
                "guide_name",
                "a guide named agents.md would act as a nested AGENTS.md; choose another name",
            );
        } else if !names.insert(name.to_owned()) {
            v.add(field("name"), "duplicate", "each guide needs its own name");
        }
        line(&mut v, &field("title"), &guide.title, MAX_LINE_CHARS, true);
        text_block(&mut v, &field("body"), &guide.body, true, 2, MAX_GUIDE_BYTES);
    }
    plan.files = render::render(input);
    for file in &plan.files {
        if file.path == AGENTS_PATH && file.content.len() > MAX_ROOT_BYTES {
            v.add(
                AGENTS_PATH,
                "too_large",
                format!(
                    "the rendered file exceeds {MAX_ROOT_BYTES} bytes; move detail into guides"
                ),
            );
        }
        local_state(&mut v, &file.path, &file.content, forbidden);
        let directory = file.path.rsplit_once('/').map_or("", |(directory, _)| directory);
        for (number, destination) in markdown::link_destinations(&file.content) {
            let at = format!("{}:{number}", file.path);
            if destination.is_empty() {
                v.add(at, "link", "a link needs a destination");
            } else if destination.starts_with('#') {
                continue;
            } else if has_scheme(&destination) || destination.starts_with(['/', '\\']) {
                v.add(at, "link", format!("{destination} is not a workspace-relative path; link only to files in this workspace"));
            } else {
                let path = destination.split(['#', '?']).next().unwrap_or_default();
                match relative_path(&decode_percent(path), directory) {
                    Ok(target) => plan.links.push(InitLink { source: file.path.clone(), target }),
                    Err(detail) => v.add(at, "link", detail),
                }
            }
        }
    }
    if v.0.is_empty() && !standard_structure(&plan.files[plan.files.len() - 1].content) {
        v.add(AGENTS_PATH, "structure", "the rendered headings are not the fixed five chapters");
    }
    plan.links.sort();
    plan.links.dedup();
    InitCheck { plan, violations: v.0 }
}

/// True when the prose headings are exactly the title and the five chapters in order;
/// level-3 and deeper headings may appear anywhere inside the chapters.
pub fn standard_structure(content: &str) -> bool {
    let top = markdown::headings(content)
        .into_iter()
        .filter(|(_, level, _)| *level <= 2)
        .map(|(_, _, text)| text)
        .collect::<Vec<_>>();
    let expected = std::iter::once(DOCUMENT_TITLE.to_owned())
        .chain(CHAPTERS.iter().map(|title| format!("## {title}")))
        .collect::<Vec<_>>();
    top == expected
}

fn text_block(
    v: &mut Violations,
    field: &str,
    text: &str,
    required: bool,
    shallowest: usize,
    max: usize,
) {
    let text = render::normalize(text);
    if required && text.is_empty() {
        v.add(
            field,
            "required",
            "write this chapter; say what the repository shows, not a placeholder",
        );
    }
    if text.len() > max {
        v.add(field, "too_large", format!("at most {max} bytes; move detail into a guide"));
    }
    controls(v, field, &text, true);
    for (number, level, heading) in markdown::headings(&text) {
        if level < shallowest {
            let allowed = "#".repeat(shallowest);
            v.add(field, "heading_level", format!("line {number} ({heading}) is a level-{level} heading; use {allowed} or deeper here"));
        }
    }
}

fn line(v: &mut Violations, field: &str, text: &str, max: usize, required: bool) {
    let trimmed = text.trim();
    if required && trimmed.is_empty() {
        v.add(field, "required", "this value is required");
    }
    if trimmed.contains(['\n', '\r']) {
        v.add(field, "single_line", "keep this on one line");
    }
    if trimmed.chars().count() > max {
        v.add(field, "too_long", format!("at most {max} characters"));
    }
    controls(v, field, trimmed, false);
}

fn controls(v: &mut Violations, field: &str, text: &str, lines: bool) {
    if text.chars().any(|c| (c < ' ' && c != '\t' && !(lines && c == '\n')) || c == '\u{7f}') {
        v.add(field, "control_character", "remove control characters");
    }
}

fn hedged(v: &mut Violations, field: &str, text: &str) {
    let lower = text.to_lowercase();
    let hedge = HEDGES_ZH
        .iter()
        .find(|word| text.contains(**word))
        .copied()
        .or_else(|| HEDGES_EN.iter().find(|word| whole_word(&lower, word)).copied());
    if let Some(word) = hedge {
        v.add(
            field,
            "hedged",
            format!(
                "\"{word}\" is a guess; state what the evidence shows, or leave the command out"
            ),
        );
    }
}

fn local_state(v: &mut Violations, path: &str, content: &str, forbidden: &[String]) {
    let lower = content.to_lowercase();
    if let Some(found) = timestamp(content) {
        v.add(
            path,
            "volatile",
            format!("{found} is a timestamp; the file must stay true without dates"),
        );
    }
    if let Some(found) = absolute_path(content) {
        v.add(
            path,
            "local_path",
            format!("{found} is a path on this machine; use a workspace-relative path"),
        );
    }
    for token in forbidden {
        let token = token.trim().to_lowercase();
        // A plain word must stand alone; a path-like token matches anywhere.
        let found = if token.chars().all(char::is_alphanumeric) {
            whole_word(&lower, &token)
        } else {
            lower.contains(&token)
        };
        if token.chars().count() >= 3 && found {
            v.add(
                path,
                "local_state",
                "the text names this machine's user or a local directory; remove it",
            );
            break;
        }
    }
}

fn timestamp(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let digits =
        |range: std::ops::Range<usize>| range.into_iter().all(|i| bytes[i].is_ascii_digit());
    (0..bytes.len().saturating_sub(15)).find_map(|i| {
        (digits(i..i + 4)
            && bytes[i + 4] == b'-'
            && digits(i + 5..i + 7)
            && bytes[i + 7] == b'-'
            && digits(i + 8..i + 10)
            && matches!(bytes[i + 10], b'T' | b' ')
            && digits(i + 11..i + 13)
            && bytes[i + 13] == b':'
            && digits(i + 14..i + 16))
        .then(|| text[i..i + 16].to_owned())
    })
}

fn absolute_path(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let boundary = |i: usize| i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
    for i in 0..bytes.len() {
        if !text.is_char_boundary(i) {
            continue;
        }
        let rest = &text[i..];
        let drive = bytes[i].is_ascii_alphabetic()
            && bytes.get(i + 1) == Some(&b':')
            && matches!(bytes.get(i + 2), Some(b'\\' | b'/'))
            && boundary(i);
        let unc = rest.starts_with("\\\\")
            && rest[2..].split('\\').next().is_some_and(|server| {
                !server.is_empty()
                    && server
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            })
            && rest[2..].contains('\\')
            && boundary(i);
        let home = (rest.starts_with("/home/") || rest.starts_with("/Users/")) && boundary(i);
        if drive || unc || home {
            return Some(rest.split_whitespace().next().unwrap_or(rest).to_owned());
        }
    }
    None
}

fn whole_word(haystack: &str, word: &str) -> bool {
    haystack.match_indices(word).any(|(start, _)| {
        let before = haystack[..start].chars().next_back();
        let after = haystack[start + word.len()..].chars().next();
        before.is_none_or(|c| !c.is_alphanumeric()) && after.is_none_or(|c| !c.is_alphanumeric())
    })
}

fn guide_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.len() <= 63
        && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A workspace-relative path, resolved against `base` (a workspace-relative directory,
/// "" for the root). "." is the root itself.
fn relative_path(path: &str, base: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("give a workspace-relative path such as \".\" or \"docs\"".to_owned());
    }
    if has_scheme(path) || path.starts_with(['/', '\\']) || path.contains(':') {
        return Err(format!("{path} is not a workspace-relative path"));
    }
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("{path} leaves the workspace"));
                }
            }
            part if part
                .chars()
                .any(|c| c < ' ' || matches!(c, '<' | '>' | '"' | '|' | '?' | '*')) =>
            {
                return Err(format!("{path} contains characters a Windows path cannot hold"));
            }
            part => parts.push(part),
        }
    }
    Ok(if parts.is_empty() { ".".to_owned() } else { parts.join("/") })
}

fn decode_percent(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(value) =
                path.get(i + 1..i + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(value);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| path.to_owned())
}
