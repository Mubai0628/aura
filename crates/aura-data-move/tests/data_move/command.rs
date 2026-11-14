//! `aura-core --move-data`: only its own arguments are taken, and what it cannot start on is
//! said as one JSON line with exit code 3.
#![allow(clippy::unwrap_used)]
use aura_data_move::{COMMAND, command};

fn run(arguments: &[&str]) -> (Option<u8>, Vec<serde_json::Value>) {
    let arguments: Vec<String> = arguments.iter().map(|argument| (*argument).to_owned()).collect();
    let mut out = Vec::new();
    let code = command(&arguments, &mut out, &|_| Ok(()));
    let lines = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (code, lines)
}

#[test]
fn other_arguments_are_not_this_command() {
    assert_eq!(run(&["aura-core"]), (None, Vec::new()));
    assert_eq!(run(&["aura-core", "--store-check"]), (None, Vec::new()));
    assert_eq!(run(&["aura-core", "--component-manifest=x", COMMAND]), (None, Vec::new()));
}

#[test]
fn a_missing_or_unreadable_manifest_is_refused_before_anything_is_read() {
    let (code, lines) = run(&["aura-core", COMMAND]);
    assert_eq!(code, Some(3));
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["error"]["code"], "arguments");

    let folder = tempfile::tempdir().unwrap();
    let missing = folder.path().join("aura-components.json");
    let argument = format!("--component-manifest={}", missing.display());
    let (code, lines) = run(&["aura-core", COMMAND, &argument]);
    assert_eq!(code, Some(3));
    assert_eq!(lines[0]["error"]["code"], "manifest");

    std::fs::write(&missing, b"not json").unwrap();
    let (code, lines) = run(&["aura-core", COMMAND, &argument]);
    assert_eq!(code, Some(3));
    assert_eq!(lines[0]["error"]["code"], "manifest");
}
