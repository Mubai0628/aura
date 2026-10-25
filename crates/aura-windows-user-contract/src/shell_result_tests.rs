use super::*;

#[test]
fn colour_codes_and_controls_go_and_a_progress_line_keeps_its_end() {
    let raw =
        b"\x1b[31;1mParserError: \x1b[0mline 3\r\n\x1b]0;title\x07ok\x00\r\n10%\r50%\r100%\r\ndone";
    assert_eq!(shell_output_text(raw), "ParserError: line 3\nok\n100%\ndone");
}

#[test]
fn utf8_text_is_kept_and_invalid_bytes_are_replaced() {
    assert_eq!(shell_output_text("未找到.txt".as_bytes()), "未找到.txt");
    assert_eq!(shell_output_text(b"a\xa1\xadb"), "a\u{fffd}\u{fffd}b");
}

#[test]
fn a_long_stream_shows_its_start_and_end_and_names_the_rest() {
    let short = vec![b'x'; SHELL_RESULT_HEAD_BYTES + SHELL_RESULT_TAIL_BYTES];
    assert_eq!(shell_stream_text(&short), (String::from_utf8(short.clone()).expect("ascii"), None));
    let mut long = vec![b'a'; SHELL_RESULT_HEAD_BYTES];
    long.extend(vec![b'm'; 10_000]);
    long.extend(vec![b'z'; SHELL_RESULT_TAIL_BYTES]);
    let (text, next) = shell_stream_text(&long);
    assert_eq!(next, Some(SHELL_RESULT_HEAD_BYTES as u64));
    assert!(text.starts_with(&"a".repeat(SHELL_RESULT_HEAD_BYTES)));
    assert!(text.ends_with(&"z".repeat(SHELL_RESULT_TAIL_BYTES)));
    assert!(
        text.contains("[… 10000 bytes not shown (bytes 2048–12048); read them with shell.query]")
    );
    assert!(!text.contains("mm"), "the middle is not shown");
}

#[test]
fn the_cut_never_splits_a_character() {
    // Three-byte characters across both cuts.
    let long = "中".repeat(4_000).into_bytes();
    let (text, next) = shell_stream_text(&long);
    assert!(!text.contains('\u{fffd}'), "no broken character at a cut");
    assert_eq!(next.expect("cut") % 3, 0);
}
