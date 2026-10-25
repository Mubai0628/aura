use logbook::{Limits, parse};

#[test]
fn imported_stack_frames_belong_to_their_header() {
    let entries = parse("1 ERROR failed\n  at worker\n\tat main", Limits::default());
    assert_eq!(entries[0].text, "failed\nat worker\nat main");
}

#[test]
fn message_storage_obeys_a_utf8_byte_limit() {
    let entries = parse("1 INFO 甲乙丙", Limits::preview(4, 1));
    assert_eq!(entries[0].text, "甲");
    assert!(entries[0].truncated);
}
