//! Lines split from what a service writes, the wait before a service starts again, and its log
//! kept without its secrets and within the limit.
use aura_plugin_host::{
    Admit, Backoff, ErrorLines, HIDDEN, LOG_LINES_PER_MINUTE, Line, LineReader, LogGate, Redactor,
};
use std::time::{Duration, Instant};

#[test]
fn lines_are_whole_across_chunks_and_a_too_long_one_is_dropped_alone() {
    let mut reader = LineReader::new(10);
    assert_eq!(reader.push(b"{\"a\":"), Vec::<Line>::new());
    assert_eq!(reader.push(b"1}\r\n\n  \n{\"b\"}"), vec![Line::Text(b"{\"a\":1}".to_vec())]);
    assert_eq!(reader.push(b"\n"), vec![Line::Text(b"{\"b\"}".to_vec())]);
    assert_eq!(reader.push(b"0123456789AB"), vec![Line::TooLong]);
    assert_eq!(reader.push(b"still the long line"), Vec::<Line>::new(), "reported once");
    assert_eq!(reader.push(b" end\nnext\n"), vec![Line::Text(b"next".to_vec())]);
    assert_eq!(reader.push(b"0123456789AB\nok\n"), vec![Line::TooLong, Line::Text(b"ok".to_vec())]);
    assert_eq!(reader.push(b"0123456789\r\n"), vec![Line::Text(b"0123456789".to_vec())]);
}

#[test]
fn long_standard_error_lines_are_withheld_without_forwarding_fragments() {
    let mut lines = ErrorLines::new(5);
    let withheld = "日志行过长，内容已隐藏";
    assert_eq!(lines.push(b"abcdefgh\nxy"), vec![withheld.to_owned()]);
    assert_eq!(lines.push(b"z\n\n"), vec!["xyz".to_owned()]);
    assert_eq!(lines.push(b"no end"), Vec::<String>::new());
    assert_eq!(lines.finish(), Some(withheld.to_owned()));
    assert_eq!(lines.push(&[b'q'; 25]), vec![withheld.to_owned()], "a line that never ends");
    assert_eq!(lines.finish(), None, "an oversized line is reported only once");
    assert_eq!(lines.push(b"discarded\nsafe\n"), vec!["safe".to_owned()]);
    assert_eq!(lines.push("中文五字行\n".as_bytes()), vec!["中文五字行".to_owned()]);
}

#[test]
fn a_windows_line_break_ends_a_standard_error_line_and_its_other_whitespace_stays() {
    let mut lines = ErrorLines::new(40);
    assert_eq!(lines.push(b"printed 1024\r\n"), vec!["printed 1024".to_owned()]);
    assert_eq!(lines.push(b"split \r"), Vec::<String>::new());
    assert_eq!(lines.push(b"\na\rb\r\n"), vec!["split ".to_owned(), "a\rb".to_owned()]);
    assert_eq!(lines.push(b" \r\n"), Vec::<String>::new(), "a blank line");
}

#[test]
fn standard_error_cannot_expose_a_secret_prefix_or_trim_a_secret_before_masking() {
    let secret = "sensitive-fixture-token ";
    let redactor = Redactor::new([secret.to_owned()]);
    let mut lines = ErrorLines::new(40);
    let line = format!("{}{secret}\n", "x".repeat(36));
    let forwarded = lines.push(line.as_bytes()).join("\n");
    assert!(!redactor.hide(&forwarded).contains("sens"), "a truncated prefix cannot be masked");
    let forwarded = lines.push(format!("{secret}\n").as_bytes()).join("\n");
    assert_eq!(redactor.hide(&forwarded), HIDDEN, "retain whitespace until masking");
    lines.push(secret.as_bytes());
    assert_eq!(redactor.hide(&lines.finish().expect("last line")), HIDDEN);
}

#[test]
fn the_wait_doubles_to_a_minute_and_a_long_run_starts_it_over() {
    let mut backoff = Backoff::default();
    let waits: Vec<u64> =
        (0..9).map(|_| backoff.ended(Duration::from_secs(10)).as_secs()).collect();
    assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60, 60]);
    assert_eq!(backoff.ended(Duration::from_secs(299)).as_secs(), 60, "not quite five minutes");
    assert_eq!(backoff.ended(Duration::from_secs(300)).as_secs(), 1);
    assert_eq!(backoff.ended(Duration::ZERO).as_secs(), 2);
    backoff.reset();
    assert_eq!(backoff.ended(Duration::ZERO).as_secs(), 1);
}

#[test]
fn secret_values_are_taken_out_whole() {
    let redactor = Redactor::new(["abc".to_owned(), "token-1".to_owned(), "token-123".to_owned()]);
    assert_eq!(
        redactor.hide("abc token-123 token-1"),
        format!("abc {HIDDEN} {HIDDEN}"),
        "a value shorter than four characters would hide ordinary words"
    );
}

#[test]
fn the_log_keeps_so_many_lines_a_minute_and_says_how_many_it_left_out() {
    let mut gate = LogGate::default();
    let start = Instant::now();
    for _ in 0..LOG_LINES_PER_MINUTE {
        assert_eq!(gate.admit(start), Admit::Keep);
    }
    assert_eq!(gate.admit(start + Duration::from_secs(30)), Admit::Drop);
    assert_eq!(gate.admit(start + Duration::from_secs(59)), Admit::Drop);
    assert_eq!(gate.admit(start + Duration::from_secs(60)), Admit::KeepAfter { dropped: 2 });
    assert_eq!(gate.admit(start + Duration::from_secs(61)), Admit::Keep);
}
