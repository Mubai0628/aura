//! Hunks in any order, hunks that change nothing, and what a hunk that fits nowhere is told.
use crate::FsToolError;
use crate::mutation::patch::{apply_text, parse_patch, prepare_diff};

fn patched(base: &str, diff: &str) -> Result<String, FsToolError> {
    let prepared = prepare_diff(diff)?;
    let patch = parse_patch(&prepared.text)?;
    apply_text(base, &patch, &prepared.starts)
}

fn message(result: Result<String, FsToolError>) -> String {
    match result {
        Ok(text) => panic!("applied: {text:?}"),
        Err(error) => error.to_string(),
    }
}

/// The shape of a plugin's service file: constants, then a helper, then the sender.
const SERVICE: &str = "const CHANNEL_ID = 'qq-main'\nconst RECONNECT_MAX_MS = 60000\n\nconst aura = await connect()\n\n/** 只发了图片时给一句说明。 */\nasync function tellPlainly(event, isGroup) {\n  const chat = isGroup ? group(event) : person(event)\n  if (!chat.id) return\n}\n\n// ── 发回聊天\n\nasync function sendChatText(chat, text) {\n  await callApi(chat, text)\n}\n";

#[test]
fn hunks_out_of_the_files_order_are_each_placed_where_their_lines_are() {
    // The model's order: the constants, the sender (near the end), then the helper above it.
    let diff = "@@\n const CHANNEL_ID = 'qq-main'\n const RECONNECT_MAX_MS = 60000\n+const MIN_SEND_GAP_MS = 1500\n@@\n-async function sendChatText(chat, text) {\n-  await callApi(chat, text)\n-}\n+function sendChatText(chat, text) {\n+  return queue(chat, text)\n+}\n@@\n async function tellPlainly(event, isGroup) {\n+  if (isGroup) return\n-  const chat = isGroup ? group(event) : person(event)\n+  const chat = person(event)\n   if (!chat.id) return\n";
    assert_eq!(
        patched(SERVICE, diff).expect("every hunk placed"),
        "const CHANNEL_ID = 'qq-main'\nconst RECONNECT_MAX_MS = 60000\nconst MIN_SEND_GAP_MS = 1500\n\nconst aura = await connect()\n\n/** 只发了图片时给一句说明。 */\nasync function tellPlainly(event, isGroup) {\n  if (isGroup) return\n  const chat = person(event)\n  if (!chat.id) return\n}\n\n// ── 发回聊天\n\nfunction sendChatText(chat, text) {\n  return queue(chat, text)\n}\n"
    );
}

#[test]
fn numbered_hunks_out_of_order_are_placed_too() {
    let base = "one\ntwo\nthree\nfour\nfive\nsix\nseven\n";
    let diff = "--- a/n.txt\n+++ b/n.txt\n@@ -6,2 +6,2 @@\n six\n-seven\n+SEVEN\n@@ -1,2 +1,2 @@\n-one\n+ONE\n two\n";
    assert_eq!(
        patched(base, diff).expect("both placed"),
        "ONE\ntwo\nthree\nfour\nfive\nsix\nSEVEN\n"
    );
}

#[test]
fn a_hunk_that_adds_and_removes_nothing_is_passed_over() {
    // A last hunk repeating a heading the hunk before it replaced, as a model once sent.
    let base = "## 三、命令\n\n命令以文字来回。\n\n## 四、排错\n\n看日志。\n";
    let diff = "@@\n 命令以文字来回。\n \n-## 四、排错\n+## 四、降低风险\n+\n+用小号。\n+\n+## 五、排错\n@@\n ## 四、排错\n*** End Patch\n";
    assert_eq!(
        patched(base, diff).expect("the empty hunk is passed over"),
        "## 三、命令\n\n命令以文字来回。\n\n## 四、降低风险\n\n用小号。\n\n## 五、排错\n\n看日志。\n"
    );
}

#[test]
fn new_lines_that_lost_their_plus_are_refused_not_passed_over() {
    let base = "a\nb\n";
    let text = message(patched(base, "@@\n a\n-b\n+B\n@@\n brand new line\n"));
    assert!(text.contains("hunk #2"), "{text}");
    assert!(text.contains("start each added line with +"), "{text}");
}

#[test]
fn two_hunks_on_the_same_lines_are_refused_and_named() {
    let base = "a\nb\nc\nd\n";
    let text = message(patched(base, "@@\n a\n-b\n+B\n c\n@@\n b\n-c\n+C\n"));
    assert!(text.contains("hunk #2") && text.contains("hunk #1 also changes"), "{text}");
    assert!(text.contains("one hunk"), "{text}");
}

#[test]
fn a_hunk_whose_later_line_differs_is_told_which_line_and_where_the_rest_is() {
    let text = message(patched(
        SERVICE,
        "@@\n async function tellPlainly(event, isGroup) {\n-  const chat = isGroup ? group(event) : someone(event)\n+  const chat = person(event)\n",
    ));
    assert!(text.contains("line 8 should be"), "{text}");
    assert!(text.contains("someone(event)") && text.contains("person(event)"), "{text}");
    assert!(text.contains("line 7"), "{text}");
}

#[test]
fn a_hunk_with_other_indentation_is_told_so() {
    let base = "fn main() {\n    run();\n}\n";
    let text = message(patched(base, "@@\n fn main() {\n-  run();\n+  go();\n"));
    assert!(text.contains("only the indentation differs"), "{text}");
    let text = message(patched(base, "@@\n-  run();\n+  go();\n"));
    assert!(text.contains("other indentation, at line 2"), "{text}");
}

#[test]
fn a_first_line_that_is_nowhere_is_named() {
    let text = message(patched(SERVICE, "@@\n-function missing() {}\n+function found() {}\n"));
    assert!(text.contains("its first line «function missing() {}» is not in the file"), "{text}");
}
