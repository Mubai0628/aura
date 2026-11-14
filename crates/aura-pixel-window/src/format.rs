//! Sizes and times as the windows show them.
use std::time::Duration;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// "< 0.1 MB", "3.5 MB", "875 MB", "8.61 GB".
pub fn size(bytes: u64) -> String {
    match bytes {
        0..104_858 => "< 0.1 MB".to_owned(),
        104_858..10_485_760 => format!("{:.1} MB", bytes as f64 / MIB as f64),
        10_485_760..GIB => format!("{} MB", (bytes as f64 / MIB as f64).round() as u64),
        _ => format!("{:.2} GB", bytes as f64 / GIB as f64),
    }
}

/// Whole megabytes with thousands separators, as in "5,338 MB".
pub fn megabytes(bytes: u64) -> String {
    let digits = (bytes / MIB).to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("{grouped} MB")
}

/// The time left at the rate so far, once there is a rate to go by: "剩余约 40 秒".
pub fn remaining(done: u64, total: u64, elapsed: Duration) -> Option<String> {
    if done == 0 || done >= total || elapsed < Duration::from_secs(2) {
        return None;
    }
    let seconds = ((total - done) as f64 * elapsed.as_secs_f64() / done as f64).ceil() as u64;
    Some(if seconds < 60 {
        format!("剩余约 {} 秒", seconds.max(1))
    } else {
        format!("剩余约 {} 分钟", seconds.div_ceil(60))
    })
}
