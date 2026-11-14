//! The clock the user sees, for dates a window shows the way the rest of Windows shows them.
use windows_sys::Win32::{
    Foundation::FILETIME, Storage::FileSystem::FileTimeToLocalFileTime,
    System::SystemInformation::GetSystemTimeAsFileTime,
};

/// How far the local clock is ahead of UTC now, in seconds (`28_800` in China). Daylight saving
/// time counts as it is now, so a time near midnight on another date can land a day off.
#[must_use]
pub fn local_utc_offset_seconds() -> Option<i32> {
    let mut utc = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    // SAFETY: the pointer is to a FILETIME on this stack frame.
    unsafe { GetSystemTimeAsFileTime(&mut utc) };
    let mut local = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    // SAFETY: both pointers are to FILETIME values on this stack frame.
    if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } == 0 {
        return None;
    }
    // FILETIME counts 100-nanosecond intervals.
    let ticks =
        |time: FILETIME| (i64::from(time.dwHighDateTime) << 32) | i64::from(time.dwLowDateTime);
    i32::try_from((ticks(local) - ticks(utc)) / 10_000_000).ok()
}

#[cfg(test)]
mod tests {
    use super::local_utc_offset_seconds;

    #[test]
    fn the_local_offset_is_a_whole_quarter_hour_within_a_day() {
        let seconds = local_utc_offset_seconds().expect("Windows converts the time");
        assert!(seconds.abs() <= 14 * 3600, "{seconds}");
        assert_eq!(seconds % 900, 0, "{seconds}");
    }
}
