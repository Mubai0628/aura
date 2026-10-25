use logbook::{Level, Limits, parse};

#[test]
fn indentation_attaches_to_the_preceding_header() {
    let entries = parse(
        " orphan\n1 ERROR failure\n  at alpha\n\tat beta\n2 INFO next\n detail",
        Limits::default(),
    );
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].text, "failure\nat alpha\nat beta");
    assert_eq!(entries[1].text, "next\ndetail");
    assert_eq!(entries[0].level, Level::Error);
}

#[test]
fn a_header_and_continuations_share_the_byte_budget() {
    let entries = parse("1 INFO abc\n defgh", Limits::preview(7, 10));
    assert_eq!(entries[0].text, "abc\ndef");
    assert!(entries[0].truncated);
}

#[test]
fn unicode_truncation_is_a_prefix_and_never_panics() {
    let entries = parse("1 INFO a\n 汉字\n x", Limits::preview(4, 10));
    assert_eq!(entries[0].text, "a\n");
    assert!(entries[0].truncated);
    assert!(entries[0].text.len() <= 4);
}

#[test]
fn long_first_lines_are_also_bounded() {
    let entries = parse("1 WARN 甲乙abc", Limits::preview(4, 10));
    assert_eq!(entries[0].text, "甲");
    assert!(entries[0].truncated);
}

#[test]
fn exact_fits_are_not_marked_truncated() {
    let entries = parse("1 INFO a\n 汉", Limits::preview(5, 10));
    assert_eq!(entries[0].text, "a\n汉");
    assert!(!entries[0].truncated);
}

#[test]
fn zero_bytes_retains_metadata_and_reports_omitted_text() {
    let entries = parse("1 ERROR important\n trace", Limits::preview(0, 2));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text, "");
    assert_eq!(entries[0].timestamp, 1);
    assert!(entries[0].truncated);
}

#[test]
fn a_full_record_limit_keeps_the_last_retained_multiline_record() {
    let entries =
        parse("1 INFO first\n second\n2 INFO ignored\n trailing", Limits::preview(100, 1));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text, "first\nsecond");
}

#[test]
fn windows_newlines_and_blank_lines_preserve_continuations() {
    let entries = parse("1 INFO ready\r\n\r\n \tdetail\r\n", Limits::default());
    assert_eq!(entries[0].text, "ready\ndetail");
}
