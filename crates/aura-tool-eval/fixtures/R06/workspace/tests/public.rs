use logbook::{Level, Limits, at_least, parse, preview};

#[test]
fn default_limits_are_stable() {
    let limits = Limits::default();
    assert_eq!(limits.max_bytes, 4096);
    assert_eq!(limits.max_records, 100);
}

#[test]
fn single_record_at_end_of_file() {
    let result = parse("17 ERROR unavailable", Limits::default());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].timestamp, 17);
    assert_eq!(result[0].level, Level::Error);
    assert_eq!(result[0].text, "unavailable");
    assert!(!result[0].truncated);
}

#[test]
fn windows_newlines_are_accepted() {
    let result = parse("1 INFO ready\r\n2 WARN slow\r\n", Limits::default());
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].text, "ready");
    assert_eq!(result[1].text, "slow");
}

#[test]
fn severity_filter_preserves_order() {
    let result = parse("4 ERROR e\n2 INFO i\n3 WARN w", Limits::default());
    let warnings = at_least(&result, Level::Warn);
    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].timestamp, 4);
    assert_eq!(warnings[1].timestamp, 3);
}

#[test]
fn zero_record_limit_does_no_work() {
    assert!(parse("1 INFO ready", Limits::preview(10, 0)).is_empty());
}

#[test]
fn unknown_severity_is_not_an_info_record() {
    assert!(parse("1 TRACE trace", Limits::default()).is_empty());
}

#[test]
fn preview_owns_its_rendered_messages() {
    let result = preview("1 WARN a\n2 ERROR b", Limits::default(), Level::Warn);
    assert_eq!(result, ["1 WARN a", "2 ERROR b"]);
}

#[test]
fn timestamp_overflow_is_ignored() {
    let result = parse("18446744073709551616 INFO invalid\n5 INFO valid", Limits::default());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].timestamp, 5);
}
