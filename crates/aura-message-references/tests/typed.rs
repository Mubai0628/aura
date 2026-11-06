//! Paths typed after "@" in a message's words. "@" starts one at the start of a line or after a
//! space; the path ends at a space or at Chinese punctuation, may be quoted when it has spaces,
//! and may be followed by an ordinary full stop or comma. Each is a reference only when it is in
//! the workspace now; otherwise the words are ordinary text. The words themselves stay as typed.
use aura_agent_domain::{AgentMessageReference, ResolvedMessageReference};
use aura_domain_common::WorkspacePath;
use aura_message_references::{Entry, ReferenceSources, resolve_message};
use std::collections::BTreeMap;

struct Workspace(BTreeMap<&'static str, Entry>);

impl ReferenceSources for Workspace {
    fn workspace_entry(&self, path: &WorkspacePath) -> anyhow::Result<Option<Entry>> {
        Ok(self.0.get(path.as_str()).copied())
    }
}

fn workspace() -> Workspace {
    Workspace(BTreeMap::from([
        (".", Entry::Folder),
        ("docs", Entry::Folder),
        ("src/lib.rs", Entry::File),
        ("my docs/a b.md", Entry::File),
        ("文档/说明.md", Entry::File),
    ]))
}

fn file(path: &str) -> ResolvedMessageReference {
    ResolvedMessageReference::WorkspaceFile { path: WorkspacePath::parse(path).expect("path") }
}

fn folder(path: &str) -> ResolvedMessageReference {
    ResolvedMessageReference::WorkspaceFolder { path: WorkspacePath::parse(path).expect("path") }
}

fn typed(text: &str) -> Vec<ResolvedMessageReference> {
    resolve_message(&workspace(), text, &[]).expect("resolved")
}

#[test]
fn a_path_after_at_at_a_line_start_or_after_a_space_is_a_reference_when_it_is_there() {
    assert_eq!(typed("Look at @src/lib.rs and @docs"), vec![file("src/lib.rs"), folder("docs")]);
    assert_eq!(typed("@docs/\nthen\n@src\\lib.rs"), vec![folder("docs"), file("src/lib.rs")]);
    assert_eq!(typed("the root @. and @文档/说明.md"), vec![folder("."), file("文档/说明.md")]);
    // Not after a space, not in the workspace, or not a workspace path: ordinary text.
    assert!(typed("mail me@docs or @missing.rs or @../docs or @/docs or @ docs").is_empty());
}

#[test]
fn a_path_ends_at_chinese_punctuation_or_before_a_full_stop_and_may_be_quoted() {
    assert_eq!(typed("看看 @src/lib.rs，然后改"), vec![file("src/lib.rs")]);
    assert_eq!(typed("see @src/lib.rs. Then @docs, too"), vec![file("src/lib.rs"), folder("docs")]);
    assert_eq!(typed(r#"read @"my docs/a b.md" first"#), vec![file("my docs/a b.md")]);
    assert!(typed(r#"read @"my docs/a b.md first"#).is_empty(), "an unclosed quote is text");
}

#[test]
fn typed_paths_come_first_in_their_order_then_the_chosen_ones_each_once() {
    let chosen = [
        AgentMessageReference::WorkspacePath { path: WorkspacePath::parse("docs").expect("path") },
        AgentMessageReference::WorkspacePath {
            path: WorkspacePath::parse("src/lib.rs").expect("path"),
        },
    ];
    let resolved =
        resolve_message(&workspace(), "@src/lib.rs then @src/lib.rs", &chosen).expect("resolved");
    assert_eq!(resolved, vec![file("src/lib.rs"), folder("docs")]);
    // A chosen one that is not there is refused; a typed one is only text.
    let missing = [AgentMessageReference::WorkspacePath {
        path: WorkspacePath::parse("gone.rs").expect("path"),
    }];
    let error = resolve_message(&workspace(), "@gone.rs", &missing).expect_err("refused");
    assert_eq!(error.to_string(), "references.not_found: 当前工作区里没有 gone.rs");
}

#[test]
fn typed_paths_count_toward_the_twenty() {
    let mut sources = workspace();
    let mut text = String::new();
    for n in 0..21 {
        sources.0.insert(Box::leak(format!("f{n}.txt").into_boxed_str()), Entry::File);
        text.push_str(&format!("@f{n}.txt "));
    }
    let error = resolve_message(&sources, &text, &[]).expect_err("too many");
    assert_eq!(error.to_string(), "references.too_many: 一条消息最多引用 20 项");
}
