//! A valid `/init` submission that each test changes in one place.
#![allow(dead_code)]
use aura_workspace_domain::{InitViolation, WorkspaceInitCompleteInput};
use serde_json::{Value, json};

pub const CALL: &str = "tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV";

pub fn valid_json() -> Value {
    json!({
        "language": "chinese",
        "expected": {"expected_kind": "absent"},
        "project_structure": "代码在 `crates/` 下，按领域分 crate。\n\n### 桌面端\n\n`apps/desktop` 是 Electron 应用。",
        "commands": {
            "introduction": "从仓库根目录运行，除非另有说明。",
            "entries": [
                {"command": "cargo test -p aura-core", "purpose": "运行 Core 测试", "shell": "powershell",
                 "evidence": {"kind": "repository_defined", "path": "docs/dev/testing.md"}},
                {"command": "corepack pnpm dev", "purpose": "启动桌面开发版", "working_directory": "apps/desktop",
                 "shell": "any", "prerequisites": "已执行 corepack pnpm install",
                 "evidence": {"kind": "execution_verified", "tool_call_id": CALL}}
            ]
        },
        "coding_style": "Rust 用 rustfmt；TypeScript 用 Prettier。详见[风格指南](docs/agent-guides/style.md)。",
        "testing": "先写能捕捉错误的测试并确认失败，再修实现。",
        "commits_and_pull_requests": "一个提交围绕一项完整改动。",
        "guides": [{"name": "style", "title": "代码风格", "body": "## Rust\n\n使用 rustfmt。",
                    "expected": {"expected_kind": "absent"}}]
    })
}

pub fn input(value: Value) -> WorkspaceInitCompleteInput {
    serde_json::from_value(value).expect("a well-formed submission")
}

pub fn valid() -> WorkspaceInitCompleteInput {
    input(valid_json())
}

/// The violations of a submission that must fail.
pub fn refused(value: Value) -> Vec<InitViolation> {
    aura_workspace_domain::check(&input(value), &[]).expect_err("the submission must be refused")
}

pub fn has(violations: &[InitViolation], rule: &str, field_prefix: &str) -> bool {
    violations.iter().any(|v| v.rule == rule && v.field.starts_with(field_prefix))
}
