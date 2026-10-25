//! `/init` renders the root AGENTS.md in a fixed structure that the rules can recognise.
mod agents_init_support;
use agents_init_support::{input, valid, valid_json};
use aura_workspace_domain::{CHAPTERS, render, standard_structure};

#[test]
fn the_root_file_is_the_title_and_the_five_chapters_in_order_and_is_written_last() {
    let files = render(&valid());
    assert_eq!(
        files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["docs/agent-guides/style.md", "AGENTS.md"]
    );
    let root = &files[1].content;
    assert!(
        root.starts_with(
            "# Repository Guidelines\n\n## Project Structure & Module Organization\n\n"
        )
    );
    let headings = root.lines().filter(|l| l.starts_with("## ")).collect::<Vec<_>>();
    assert_eq!(headings, CHAPTERS.iter().map(|t| format!("## {t}")).collect::<Vec<_>>());
    assert!(root.contains("\n### 桌面端\n"), "deeper headings stay inside their chapter");
    assert!(root.ends_with("一个提交围绕一项完整改动。\n"));
    assert!(standard_structure(root));
}

#[test]
fn commands_carry_directory_shell_prerequisites_and_evidence_in_the_chosen_language() {
    let root = render(&valid()).pop().expect("root").content;
    assert!(root.contains(
        "- `cargo test -p aura-core`：运行 Core 测试\n  - 目录：`.`；Shell：PowerShell\n  - 依据：仓库定义（`docs/dev/testing.md`）"
    ));
    assert!(root.contains(
        "- `corepack pnpm dev`：启动桌面开发版\n  - 目录：`apps/desktop`；Shell：PowerShell 或 CMD；前置条件：已执行 corepack pnpm install\n  - 依据：初始化时已执行验证"
    ));
    assert!(!root.contains("tcl_"), "a tool call identity is session state, not repository fact");
    let mut english = valid_json();
    english["language"] = "english".into();
    let root = render(&input(english)).pop().expect("root").content;
    assert!(root.contains(
        "- `cargo test -p aura-core` — 运行 Core 测试\n  - Directory: `.`; shell: PowerShell\n  - Source: defined in `docs/dev/testing.md`"
    ));
    assert!(root.contains("  - Source: verified by running it during initialization"));
}

#[test]
fn a_guide_has_its_title_as_its_only_level_one_heading() {
    let guide = &render(&valid())[0];
    assert_eq!(guide.content, "# 代码风格\n\n## Rust\n\n使用 rustfmt。\n");
}

#[test]
fn backticks_in_a_command_still_render_as_one_code_span() {
    let mut value = valid_json();
    value["commands"]["entries"][0]["command"] = "Write-Host `\"ok`\"".into();
    let root = render(&input(value)).pop().expect("root").content;
    assert!(root.contains("- `` Write-Host `\"ok`\" ``："));
}

#[test]
fn rendering_is_deterministic_and_normalizes_line_endings_and_trailing_space() {
    let mut value = valid_json();
    value["testing"] = "\r\n先写测试。  \r\n\r\n再修实现。\r\n\r\n".into();
    let first = render(&input(value.clone()));
    assert_eq!(first, render(&input(value)));
    let root = &first.last().expect("root").content;
    assert!(root.contains("## Testing Guidelines\n\n先写测试。\n\n再修实现。\n\n## Commit"));
    assert!(!root.contains('\r'));
}

#[test]
fn only_the_exact_fixed_structure_counts_as_standard() {
    let root = render(&valid()).pop().expect("root").content;
    assert!(standard_structure(&root));
    let missing = root.replace("## Testing Guidelines\n", "### Testing Guidelines\n");
    assert!(!standard_structure(&missing));
    let extra = format!("{root}\n## Notes\n\nmore\n");
    assert!(!standard_structure(&extra));
    let reordered = root.replace("## Coding Style", "## Testing Guide").replacen(
        "## Testing Guidelines",
        "## Coding Style & Naming Conventions",
        1,
    );
    assert!(!standard_structure(&reordered));
    let fenced = root.replace("先写能捕捉错误", "```\n## not a heading\n```\n先写能捕捉错误");
    assert!(standard_structure(&fenced), "fenced code is not structure");
}

/// A commands chapter with no command is its explanation alone, without stray blank lines.
#[test]
fn a_commands_chapter_without_commands_is_its_explanation() {
    let mut value = valid_json();
    value["commands"]["entries"] = serde_json::json!([]);
    value["commands"]["introduction"] = "这个工作区还没有构建、测试或开发命令。".into();
    value["commands"]["notes"] = "".into();
    let root = render(&input(value)).pop().expect("root").content;
    assert!(
        root.contains("## Build, Test, and Development Commands\n\n这个工作区还没有构建、测试或开发命令。\n\n## "),
        "{root}"
    );
    assert!(standard_structure(&root));
}
