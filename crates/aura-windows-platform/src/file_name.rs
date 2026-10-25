//! Windows ordinal filename keys, acquired at a held directory boundary.
//! The OS table is immutable for this process and is data passed to the domains, not a global
//! mutable policy. Directory sensitivity is always read from the actual directory handle.

use std::{os::windows::io::AsRawHandle, sync::OnceLock};

use aura_domain_common::FileNameComparison;
use windows_sys::{
    Wdk::System::SystemServices::RtlUpcaseUnicodeChar,
    Win32::Storage::FileSystem::GetVolumeInformationByHandleW,
};

use crate::{VerifiedFile, VolumeCaseRule, directory_case_rule};

/// A failed rule/table/filesystem query affects only the requested target. No guessed language
/// fold is supplied. Other filesystems may have volume-specific tables (for example exFAT).
#[must_use]
pub fn directory_name_comparison(directory: &VerifiedFile) -> Option<FileNameComparison> {
    if directory_case_rule(directory) != VolumeCaseRule::InsensitivePreserving {
        return None;
    }
    let mut filesystem = [0_u16; 32];
    // SAFETY: the held handle remains alive; the output is an initialized, writable array.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            directory.file().as_raw_handle().cast(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            filesystem.as_mut_ptr(),
            u32::try_from(filesystem.len()).expect("small buffer"),
        )
    };
    if ok == 0 {
        return None;
    }
    let end = filesystem.iter().position(|unit| *unit == 0)?;
    // Windows' non-linguistic ordinal comparison is the documented comparison for NTFS names.
    // Do not extend that statement to a filesystem whose collation we have not established.
    if String::from_utf16(&filesystem[..end]).ok()?.as_str() != "NTFS" {
        return None;
    }
    windows_ordinal_name_comparison()
}

fn windows_ordinal_name_comparison() -> Option<FileNameComparison> {
    static COMPARISON: OnceLock<Option<FileNameComparison>> = OnceLock::new();
    COMPARISON
        .get_or_init(|| {
            let table = (0..=u16::MAX)
                .map(|unit| {
                    // SAFETY: this total OS character mapping takes and returns a single UTF-16 unit.
                    unsafe { RtlUpcaseUnicodeChar(unit) }
                })
                .collect();
            FileNameComparison::from_uppercase_table(table)
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_ordinal_compare_ignore_case;
    use std::cmp::Ordering;

    #[test]
    fn filename_keys_match_windows_ordinal_without_linguistic_expansion() {
        let comparison = windows_ordinal_name_comparison().expect("OS comparison table");
        for (a, b) in [
            ("ß", "ss"),
            ("ﬃ", "ffi"),
            ("\u{10428}", "\u{10400}"),
            ("\u{1f80}", "\u{1f88}"),
            ("note", "NOTE"),
        ] {
            let actual = windows_ordinal_compare_ignore_case(a.as_ref(), b.as_ref()).expect("OS");
            assert_eq!(
                comparison.key(a) == comparison.key(b),
                actual == Ordering::Equal,
                "Windows ordinal comparison for {a:?} / {b:?}"
            );
        }
    }
}
