//! A message's references are resolved once when it is sent: each in the order the message has
//! it, each once, at most twenty, and only what is there now.
use aura_agent_domain::{AgentMessageReference, ResolvedMessageReference};
use aura_domain_common::WorkspacePath;
use aura_message_references::{Entry, ReferenceSources, resolve};
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
    ]))
}

fn path(value: &str) -> AgentMessageReference {
    AgentMessageReference::WorkspacePath { path: WorkspacePath::parse(value).expect("path") }
}

fn at(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).expect("path")
}

#[test]
fn files_and_folders_are_told_apart_in_the_order_given_and_each_kept_once() {
    let resolved = resolve(
        &workspace(),
        &[path("docs"), path("src/lib.rs"), path("docs"), path("src\\lib.rs"), path(".")],
    )
    .expect("resolved");
    assert_eq!(
        resolved,
        vec![
            ResolvedMessageReference::WorkspaceFolder { path: at("docs") },
            ResolvedMessageReference::WorkspaceFile { path: at("src/lib.rs") },
            ResolvedMessageReference::WorkspaceFolder { path: at(".") },
        ]
    );
    assert!(resolve(&workspace(), &[]).expect("none").is_empty());
}

#[test]
fn a_path_that_is_not_in_the_workspace_now_is_refused_with_its_name() {
    let error = resolve(&workspace(), &[path("src/main.rs")]).expect_err("missing");
    assert_eq!(error.to_string(), "references.not_found: 当前工作区里没有 src/main.rs");
}

#[test]
fn a_message_refers_to_at_most_twenty_things() {
    let many: Vec<_> = (0..21).map(|n| path(&format!("f{n}.txt"))).collect();
    let mut sources = workspace();
    for n in 0..21 {
        sources.0.insert(Box::leak(format!("f{n}.txt").into_boxed_str()), Entry::File);
    }
    assert!(resolve(&sources, &many[..20]).is_ok(), "twenty is allowed");
    let error = resolve(&sources, &many).expect_err("too many");
    assert_eq!(error.to_string(), "references.too_many: 一条消息最多引用 20 项");
}
