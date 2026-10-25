//! The fixed AGENTS.md structure. Rendering is a pure function of the input, so the
//! approved preview and the written bytes are the same text.
use super::WorkspaceInitCompleteInput as Input;
use super::{InitCommand, InitEvidence, InitGuide, InitLanguage, InitShell};

pub const AGENTS_PATH: &str = "AGENTS.md";
pub const GUIDES_DIRECTORY: &str = "docs/agent-guides";
pub const DOCUMENT_TITLE: &str = "# Repository Guidelines";
/// The only level-2 headings of the root file, in this order.
pub const CHAPTERS: [&str; 5] = [
    "Project Structure & Module Organization",
    "Build, Test, and Development Commands",
    "Coding Style & Naming Conventions",
    "Testing Guidelines",
    "Commit & Pull Request Guidelines",
];

/// One file of the batch: the root AGENTS.md or a guide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedFile {
    /// Workspace-relative, forward slashes.
    pub path: String,
    pub content: String,
}

pub fn guide_path(name: &str) -> String {
    format!("{GUIDES_DIRECTORY}/{name}.md")
}

/// Guides first and the root file last, which is also the write order.
pub fn render(input: &Input) -> Vec<RenderedFile> {
    let mut files = input.guides.iter().map(render_guide).collect::<Vec<_>>();
    files.push(RenderedFile { path: AGENTS_PATH.to_owned(), content: render_root(input) });
    files
}

fn render_root(input: &Input) -> String {
    let commands = render_commands(input);
    let chapters = [
        normalize(&input.project_structure),
        commands,
        normalize(&input.coding_style),
        normalize(&input.testing),
        normalize(&input.commits_and_pull_requests),
    ];
    let mut text = format!("{DOCUMENT_TITLE}\n");
    for (title, body) in CHAPTERS.iter().zip(chapters) {
        text.push_str("\n## ");
        text.push_str(title);
        text.push_str("\n\n");
        text.push_str(&body);
        text.push('\n');
    }
    text
}

fn render_commands(input: &Input) -> String {
    let language = input.language;
    let mut blocks = Vec::new();
    let introduction = normalize(&input.commands.introduction);
    if !introduction.is_empty() {
        blocks.push(introduction);
    }
    // A workspace with no command yet has its explanation alone.
    if !input.commands.entries.is_empty() {
        blocks.push(
            input
                .commands
                .entries
                .iter()
                .map(|command| render_command(command, language))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let notes = normalize(&input.commands.notes);
    if !notes.is_empty() {
        blocks.push(notes);
    }
    blocks.join("\n\n")
}

fn render_command(command: &InitCommand, language: InitLanguage) -> String {
    let shell = match command.shell {
        InitShell::PowerShell => "PowerShell",
        InitShell::Cmd => "CMD",
        InitShell::Any => match language {
            InitLanguage::Chinese => "PowerShell 或 CMD",
            InitLanguage::English => "PowerShell or CMD",
        },
    };
    let directory = code_span(command.working_directory.trim());
    let prerequisites = one_line(&command.prerequisites);
    let evidence = match (&command.evidence, language) {
        (InitEvidence::RepositoryDefined { path }, InitLanguage::Chinese) => {
            format!("依据：仓库定义（{}）", code_span(path.trim()))
        }
        (InitEvidence::RepositoryDefined { path }, InitLanguage::English) => {
            format!("Source: defined in {}", code_span(path.trim()))
        }
        (InitEvidence::ExecutionVerified { .. }, InitLanguage::Chinese) => {
            "依据：初始化时已执行验证".to_owned()
        }
        (InitEvidence::ExecutionVerified { .. }, InitLanguage::English) => {
            "Source: verified by running it during initialization".to_owned()
        }
    };
    let (separator, details) = match language {
        InitLanguage::Chinese => (
            "：",
            if prerequisites.is_empty() {
                format!("目录：{directory}；Shell：{shell}")
            } else {
                format!("目录：{directory}；Shell：{shell}；前置条件：{prerequisites}")
            },
        ),
        InitLanguage::English => (
            " — ",
            if prerequisites.is_empty() {
                format!("Directory: {directory}; shell: {shell}")
            } else {
                format!("Directory: {directory}; shell: {shell}; requires: {prerequisites}")
            },
        ),
    };
    format!(
        "- {}{separator}{}\n  - {details}\n  - {evidence}",
        code_span(command.command.trim()),
        one_line(&command.purpose)
    )
}

fn render_guide(guide: &InitGuide) -> RenderedFile {
    RenderedFile {
        path: guide_path(guide.name.trim()),
        content: format!("# {}\n\n{}\n", one_line(&guide.title), normalize(&guide.body)),
    }
}

/// LF line endings, no trailing spaces at line ends, no leading or trailing blank lines.
pub(super) fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_owned()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A Markdown code span that survives backticks in the text (PowerShell escapes).
pub(super) fn code_span(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for character in text.chars() {
        if character == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat(longest + 1);
    if longest == 0 { format!("{fence}{text}{fence}") } else { format!("{fence} {text} {fence}") }
}
