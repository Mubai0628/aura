use std::time::Duration;

use crate::format::{megabytes, remaining, size};

#[test]
fn sizes_read_as_the_design_writes_them() {
    assert_eq!(size(0), "< 0.1 MB");
    assert_eq!(size(50_000), "< 0.1 MB");
    assert_eq!(size(3 * 1024 * 1024 + 512 * 1024), "3.5 MB");
    assert_eq!(size(875 * 1024 * 1024), "875 MB");
    assert_eq!(size(9_244_000_000), "8.61 GB");
    assert_eq!(megabytes(5_338 * 1024 * 1024), "5,338 MB");
    assert_eq!(megabytes(1_664 * 1024 * 1024 + 5), "1,664 MB");
    assert_eq!(megabytes(12 * 1024 * 1024), "12 MB");
    assert_eq!(megabytes(1_234_567 * 1024 * 1024), "1,234,567 MB");
}

#[test]
fn the_time_left_waits_for_a_rate() {
    assert_eq!(remaining(0, 100, Duration::from_secs(10)), None);
    assert_eq!(remaining(50, 100, Duration::from_secs(1)), None, "too early to tell");
    assert_eq!(remaining(100, 100, Duration::from_secs(10)), None, "done");
    assert_eq!(remaining(60, 100, Duration::from_secs(60)), Some("剩余约 40 秒".to_owned()));
    assert_eq!(remaining(10, 100, Duration::from_secs(60)), Some("剩余约 9 分钟".to_owned()));
}
