use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Task {
    pub id: &'static str,
    pub language: &'static str,
    pub title: &'static str,
    pub prompt: &'static str,
    pub implemented: bool,
}
pub fn catalog() -> Vec<Task> {
    [
        ("R01","Rust","分页游标错误","Fix page() so cursor is honored and out-of-range cursors return an empty page. Add tests; keep its public signature.",true),
        ("R02","Rust","配置缺省值","Add safe defaults to the fixture config and tests.",false),
        ("R03","Rust","路径作用域","Fix relative path scope validation and add boundary tests.",false),
        ("R04","Rust","增量目录删除","Fix deleted entries in the incremental catalog.",false),
        ("R05","Rust","作业队列重构","Split the fixture queue without changing its ordering.",false),
        ("R06","Rust","多文件日志解析","Crash-log previews lose indented stack frames and can exceed the configured message byte limit. Fix both multiline parsing and bounded message storage. Headers contain a nonnegative integer timestamp, an uppercase severity and an optional message. Attach space/tab-indented lines to the preceding header, remove their leading indentation and join them with a newline; ignore orphan continuations, blank and malformed unindented lines. Retain source order and the final record at EOF. max_bytes bounds each complete message in UTF-8 bytes (including newlines); keep a valid prefix, set truncated only if text was omitted, and never resume appending after truncation. max_records keeps the first entries, including their continuations. Keep the public API and add regression tests. Use existing local tools to build and test offline.",true),
        ("T01","TypeScript","分页游标错误","Fix page() so cursor is honored and an out-of-range cursor returns an empty page. Add tests; keep its public signature.",true),
        ("T02","TypeScript","来源配置校验","Add origin validation and settings tests.",false),
        ("T03","TypeScript","可空字段","Fix nullable/optional schema decoding.",false),
        ("T04","TypeScript","输出分页","Fix bounded output pagination and truncation.",false),
        ("T05","TypeScript","Reducer 重构","Split state transitions preserving observable behavior.",false),
        ("T06","TypeScript","API 重试","After an order connection drops, the client sometimes repeats a write and reports an ordinary retryable failure even though the first request may have taken effect. Fix retry eligibility and the caller-visible outcome. POST, PUT and DELETE must never replay after a post-send transport failure or HTTP 5xx; report OutcomeUnknown with code outcome_unknown and the actual attempt count, even when the failure is not retryable. GET can retry temporary post-send failures. A retryable pre-send failure and a definite HTTP 429 refusal can retry any method within the existing attempt/backoff limits. Preserve public interfaces and existing known-error behavior. Add regression tests and verify with the local test runner; do not install packages or access the network.",true),
        ("P01","Python","Unicode 搜索位置","Fix find_positions() to return Unicode character positions, not UTF-8 byte offsets. Matches must not overlap; an empty query returns no matches. Add tests and keep its signature.",true),
        ("P02","Python","配置缺省值","Add config defaults and preserve unknown-field refusal.",false),
        ("P03","Python","CSV CLI","Add CSV CLI parsing and error handling.",false),
        ("P04","Python","临时产物过滤","Exclude publication artifacts from ordinary listings.",false),
        ("P05","Python","解析器重构","Split the fixture parser preserving behavior.",false),
        ("P06","Python","依赖图","The build planner sometimes produces a plausible schedule for cyclic prerequisites, and its graph inspection says there is no cycle. Add deterministic cycle detection and make planning (including waves) refuse cyclic input with the existing CycleError type. A witness is a tuple beginning and ending at the same task; inspect roots and dependencies in lexical order and report the first DFS back-edge cycle without any prefix outside it. Self-loops count. Preserve missing-dependency validation before cycle checks, stable acyclic ordering, timings, shared dependencies and public APIs. Add a regression test that catches the original defect and run the existing offline tests.",true),
    ].into_iter().map(|(id,language,title,prompt,implemented)|Task {id,language,title,prompt,implemented}).collect()
}
pub fn task(id: &str) -> Option<Task> {
    catalog().into_iter().find(|task| task.id == id)
}
