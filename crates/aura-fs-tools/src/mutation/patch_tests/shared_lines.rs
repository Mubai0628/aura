//! Hunks of nearby changes share unchanged lines, as diff tools write them, and both apply. A line
//! one of them removes, or lines added among the shared ones, still stop the diff.
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

/// Two waves of a game, each spawning enemies a change will scale.
const WAVES: &str = "function nextWave() {\n  if (kind === 1) {\n    for (let i = 0; i < 3; i++) {\n      spawnDart(-30, 40);\n    }\n  } else if (kind === 2) {\n    spawnGunner(-115, -60);\n  }\n}\n";

const SCALED: &str = "function nextWave() {\n  if (kind === 1) {\n    for (let i = 0; i < 3; i++) {\n      spawnDart(-30 * k, 40 * k);\n    }\n  } else if (kind === 2) {\n    spawnGunner(-115 * k, -60 * k);\n  }\n}\n";

#[test]
fn numbered_hunks_whose_context_shares_a_line_both_apply() {
    // The second hunk starts on the line the first one ends on, as in the owner's 11-10 trial.
    let diff = "--- a/game.js\n+++ b/game.js\n@@ -2,4 +2,4 @@\n   if (kind === 1) {\n     for (let i = 0; i < 3; i++) {\n-      spawnDart(-30, 40);\n+      spawnDart(-30 * k, 40 * k);\n     }\n@@ -5,3 +5,3 @@\n     }\n   } else if (kind === 2) {\n-    spawnGunner(-115, -60);\n+    spawnGunner(-115 * k, -60 * k);\n";
    assert_eq!(patched(WAVES, diff).expect("both hunks apply"), SCALED);
}

#[test]
fn hunks_without_numbers_that_share_two_lines_both_apply() {
    let diff = "@@\n     for (let i = 0; i < 3; i++) {\n-      spawnDart(-30, 40);\n+      spawnDart(-30 * k, 40 * k);\n     }\n   } else if (kind === 2) {\n@@\n     }\n   } else if (kind === 2) {\n-    spawnGunner(-115, -60);\n+    spawnGunner(-115 * k, -60 * k);\n   }\n";
    assert_eq!(patched(WAVES, diff).expect("both hunks apply"), SCALED);
}

#[test]
fn lines_added_among_the_shared_lines_are_refused_and_named() {
    // The second hunk adds a line between two lines the first one already covers.
    let diff = "@@\n     for (let i = 0; i < 3; i++) {\n-      spawnDart(-30, 40);\n+      spawnDart(-30 * k, 40 * k);\n     }\n   } else if (kind === 2) {\n@@\n     }\n+    flash();\n   } else if (kind === 2) {\n-    spawnGunner(-115, -60);\n+    spawnGunner(-115 * k, -60 * k);\n";
    let text = message(patched(WAVES, diff));
    assert!(text.contains("hunk #2") && text.contains("hunk #1 also changes"), "{text}");
    assert!(text.contains("one hunk"), "{text}");
}

#[test]
fn a_backslash_the_file_does_not_have_before_a_quote_is_named() {
    let base = "ctx.save();\nctx.font = 'bold 15px \"Consolas\", monospace';\nctx.fill();\n";
    let diff = "@@\n ctx.save();\n-ctx.font = 'bold 15px \\\"Consolas\\\", monospace';\n+ctx.font = 'bold 12px \\\"Consolas\\\", monospace';\n ctx.fill();\n";
    let text = message(patched(base, diff));
    assert!(text.contains("line 2 should be"), "{text}");
    assert!(text.contains("only the backslashes differ"), "{text}");
    assert!(text.contains("the diff has ones the file does not"), "{text}");
    // The same when the first of its lines is the one written so.
    let diff = "@@\n-ctx.font = 'bold 15px \\\"Consolas\\\", monospace';\n+ctx.font = 'bold 12px \\\"Consolas\\\", monospace';\n";
    let text = message(patched(base, diff));
    assert!(text.contains("only with other backslashes, at line 2"), "{text}");
}
