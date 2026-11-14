use super::*;
use serde_json::{Value, json};

const HASH: &str = "e64282892d6d6f5d0d3512ee8538927da46f795f599a5cb00374c1d47d447962";

/// A stored envelope of the shape a real fs.read result has.
fn envelope(outcome: Value, side_effect_state: &str) -> Envelope {
    serde_json::from_value(json!({
        "schema_version": 1,
        "tool_call_id": "tcl_01M71NZ4A1ZA288HN6F4Y3C4SK",
        "definition_ref": {
            "tool_id": "tol_01M2DG020980080021AN942001",
            "tool_version": {"semantic_version": null, "definition_revision": "9"},
            "registry_snapshot_ref": {
                "tool_registry_snapshot_id": "trs_01M71NYWHGGDAA5GZN564XNAXS",
                "registry_revision": "1",
                "content_hash": {
                    "algorithm": "sha256",
                    "canonicalization_version": "rfc8785_jcs_v1",
                    "hex": "848e99438985ea705d48319d3672ef904655a8bb98ad5e247bbbca03470a2193"
                }
            }
        },
        "outcome": outcome,
        "side_effect_state": side_effect_state,
        "warnings": [],
        "artifacts": [{
            "artifact_id": "art_01M71NZ4NCP3VK79420TEN0XRD",
            "slot": "result",
            "media_type": "application/vnd.aura.model-tool-outcome+json",
            "byte_length": 12669,
            "content_hash": {
                "algorithm": "sha256",
                "hex": "8b1030f8c9d9fa662acfe8cd4e6f08c36e80d6d823e7dad52033d3ec89c6613d"
            }
        }],
        "completed_at": "2026-11-10T02:53:02.9881216Z"
    }))
    .expect("an envelope")
}

fn succeeded(value: Value) -> Envelope {
    envelope(
        json!({"type": "succeeded", "payload": {"type": "structured_json", "value": value}}),
        "no_effect",
    )
}

/// An fs.read result of `text`, with `extra` fields replacing the usual ones.
fn read(text: &str, range: Value, extra: Value) -> Value {
    let mut value = json!({
        "active_editor_session": null,
        "buffer_revision": null,
        "disk_hash": {"algorithm": "blake3", "hex": HASH},
        "disk_may_be_stale": false,
        "encoding": "utf8",
        "external": null,
        "is_dirty": false,
        "line_ending": "crlf",
        "logical_hash": {"algorithm": "blake3", "hex": HASH},
        "path": "game/fighter.html",
        "returned_range": range,
        "source": "disk",
        "text": text,
        "total_characters": 51782,
        "total_lines": 1818
    });
    if let (Some(value), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
        value.extend(extra.clone());
    }
    value
}

/// The first line as JSON, and the lines after it.
fn split(shown: &str) -> (Value, Vec<&str>) {
    let mut lines = shown.split('\n');
    let head = serde_json::from_str(lines.next().expect("a first line")).expect("JSON first");
    (head, lines.collect())
}

/// The trial's patch escaped the quotes it copied from a read: the lines now come as they are.
#[test]
fn a_range_of_lines_reads_as_the_files_own_lines_numbered_from_one() {
    let text = "ctx.font = 'bold 15px \"Consolas\", monospace';\r\nconst re = /\\d+/;\r\n";
    let range = json!({"range_kind": "lines", "start_line_index": 41, "count": 2});
    let shown = model_text("fs.read", &succeeded(read(text, range, json!({}))));
    let (head, lines) = split(&shown);
    assert_eq!(
        lines,
        ["42\tctx.font = 'bold 15px \"Consolas\", monospace';", "43\tconst re = /\\d+/;"]
    );
    assert!(!shown.contains("\\\"") && !shown.contains("\\n"), "nothing escaped: {shown}");
    assert_eq!(head["path"], "game/fighter.html");
    assert_eq!(head["lines"], json!([42, 43]));
    assert_eq!(head["total_lines"], 1818);
    assert_eq!(head["logical_hash"], json!({"algorithm": "blake3", "hex": HASH}));
    assert_eq!((&head["encoding"], &head["line_ending"]), (&json!("utf8"), &json!("crlf")));
    for noise in ["disk_hash", "active_editor_session", "artifacts", "text", "returned_range"] {
        assert!(head.get(noise).is_none(), "{noise}: {head}");
    }
}

#[test]
fn a_whole_file_is_numbered_from_one_and_an_empty_one_is_its_first_line_alone() {
    let whole = json!({"range_kind": "full"});
    let shown =
        model_text("fs.read", &succeeded(read("a\n\nb", whole.clone(), json!({"total_lines": 3}))));
    let (head, lines) = split(&shown);
    assert_eq!(lines, ["1\ta", "2\t", "3\tb"]);
    assert_eq!(head["lines"], json!([1, 3]));
    let shown = model_text("fs.read", &succeeded(read("", whole, json!({"total_lines": 0}))));
    let (head, lines) = split(&shown);
    assert!(lines.is_empty(), "{shown}");
    assert_eq!(head["lines"], json!([]));
}

#[test]
fn a_range_of_characters_reads_as_it_is_and_an_outside_file_by_its_full_path() {
    let range = json!({"range_kind": "characters", "start_character_offset": 100, "count": 5});
    let extra = json!({
        "external": {"grant_id": null, "display_path": "C:\\Users\\me\\notes.txt"},
        "is_dirty": true
    });
    let shown = model_text("fs.read", &succeeded(read("ab\ncd", range, extra)));
    let (head, lines) = split(&shown);
    assert_eq!(lines, ["ab", "cd"]);
    assert_eq!(head["characters"], json!([101, 105]));
    assert_eq!(head["total_characters"], 51782);
    assert_eq!(head["path"], "C:\\Users\\me\\notes.txt");
    assert_eq!(
        head["external"],
        json!({"grant_id": null, "display_path": "C:\\Users\\me\\notes.txt"}),
        "the authority that let it be read stays named: {head}"
    );
    assert_eq!(head["unsaved_editor_changes"], true);
    assert!(head.get("lines").is_none(), "{head}");
}

/// One match of an fs.search result.
fn found(line: u64, excerpt: &str, path: &str) -> Value {
    json!({
        "buffer_revision": null,
        "column_character_offset": 2,
        "excerpt": excerpt,
        "excerpt_start_character_offset": 0,
        "excerpt_truncated_after": false,
        "excerpt_truncated_before": false,
        "generated": false,
        "ignored": false,
        "line_index": line,
        "logical_hash": {"algorithm": "blake3", "hex": HASH},
        "match_character_length": 6,
        "path": path,
        "source": "disk"
    })
}

/// The trial's searches were 94% metadata: the file's hash came with every match.
#[test]
fn a_search_names_each_files_hash_once_and_each_match_by_its_line() {
    let mut cut = found(1500, "drawBoss(ctx, boss, t, \"glow\")", "game/fighter.html");
    cut["excerpt_truncated_after"] = json!(true);
    let value = json!({
        "candidate_file_count": 2,
        "catalog_revision": "1",
        "completion": {"status": "complete"},
        "external": null,
        "matches": [
            found(76, "  function resize() {", "game/fighter.html"),
            found(76, "  function resize() {", "game/fighter.html"),
            cut,
            found(3, "resize();", "game/boot.js"),
        ],
        "scanned_bytes": 62480,
        "scanned_file_count": 2,
        "skipped_file_count": 0
    });
    let shown = model_text("fs.search", &succeeded(value));
    let (head, lines) = split(&shown);
    assert_eq!(head["completion"], json!({"status": "complete"}));
    assert_eq!(head["match_count"], 4);
    assert!(head.get("matches").is_none() && head.get("external").is_none(), "{head}");
    assert_eq!(lines.len(), 5, "{shown}");
    let file: Value = serde_json::from_str(lines[0]).expect("the file's line");
    assert_eq!(
        file,
        json!({"path": "game/fighter.html", "logical_hash": {"algorithm": "blake3", "hex": HASH}})
    );
    assert_eq!(
        &lines[1..3],
        ["77\t  function resize() {", "1501\tdrawBoss(ctx, boss, t, \"glow\")…"]
    );
    let next: Value = serde_json::from_str(lines[3]).expect("the next file's line");
    assert_eq!(next["path"], "game/boot.js");
    assert_eq!(lines[4], "4\tresize();");
    assert_eq!(shown.matches(HASH).count(), 2, "one hash per file: {shown}");
}

/// A main agent read whole files to see what was around its matches.
#[test]
fn lines_around_the_matches_come_in_their_place_with_a_dash_after_the_number() {
    let around = |line: u64, text: &str| json!({"line_index": line, "text": text});
    let mut cut = around(4, "x");
    cut["truncated"] = json!(true);
    let value = json!({
        "candidate_file_count": 2,
        "catalog_revision": "1",
        "completion": {"status": "complete"},
        "context": [
            {
                "path": "game/fighter.html",
                "lines": [around(74, "  // size"), around(77, "  }"), around(79, "  let w;")]
            },
            {"path": "game/boot.js", "lines": [around(1, "init();"), cut]}
        ],
        "external": null,
        "matches": [
            found(75, "function resize() {", "game/fighter.html"),
            found(78, "resize();", "game/fighter.html"),
            found(3, "resize();", "game/boot.js"),
        ],
        "scanned_bytes": 100,
        "scanned_file_count": 2,
        "skipped_file_count": 0
    });
    let shown = model_text("fs.search", &succeeded(value));
    let (head, lines) = split(&shown);
    assert!(head.get("context").is_none(), "{head}");
    assert_eq!(head["match_count"], 3);
    assert_eq!(lines.len(), 10, "{shown}");
    assert_eq!(
        &lines[1..6],
        ["75-\t  // size", "76\tfunction resize() {", "78-\t  }", "79\tresize();", "80-\t  let w;"]
    );
    let next: Value = serde_json::from_str(lines[6]).expect("the next file's line");
    assert_eq!(next["path"], "game/boot.js");
    assert_eq!(&lines[7..], ["2-\tinit();", "4\tresize();", "5-\tx…"]);
}

#[test]
fn any_other_result_reads_as_before() {
    let picture = succeeded(json!({
        "path": "a.png",
        "media_type": "image/png",
        "byte_length": 3,
        "shown_to_model": true,
        "note": "x"
    }));
    assert_eq!(model_text("fs.read", &picture), picture.model_text());
    let other = succeeded(read("a\n", json!({"range_kind": "full"}), json!({})));
    assert_eq!(model_text("fs.write", &other), other.model_text());
}
