use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::PWSTR;

/// Copies a string the system allocated with the COM allocator, then frees it.
pub fn take_com_string(value: PWSTR) -> String {
    if value.is_null() {
        return String::new();
    }
    // SAFETY: the callers pass a NUL-terminated string the system just returned to them, which
    // nothing else frees; it is read once, then released with the allocator it came from.
    unsafe {
        let text = String::from_utf16_lossy(value.as_wide());
        CoTaskMemFree(Some(value.0 as *const _));
        text
    }
}

/// A NUL-terminated UTF-16 copy of `text`.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The text of a buffer the system filled, up to its first NUL.
pub fn from_buffer(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}
