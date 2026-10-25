//! The environment block Aura hands to a process it starts itself.
//!
//! `CreateProcessW` with a null environment pointer does not *copy* the parent's
//! environment — it makes the child inherit the parent's live block, every entry
//! of it, with no boundary where Aura can decide what its own child should see.
//! That is what let a compatibility layer injected into a user's shell reach the
//! headless browser Aura starts for rendering: the browser is launched through
//! `shimeng`, the layer makes it exit successfully with no DOM at all, and the
//! render is reported as unavailable with nothing in the process to point at the
//! cause.
//!
//! This module gives a launch boundary the one thing it needs: the parent's own
//! environment, as the operating system holds it, with named variables removed.
//! Everything else is preserved exactly — hidden per-drive current-directory
//! entries included, because an explicit block replaces inheritance entirely and
//! Windows does not add those back — and names are matched the way the operating
//! system matches them, case-insensitively.
use std::{ffi::OsString, io, os::windows::ffi::OsStringExt};

use windows_sys::Win32::System::Environment::{FreeEnvironmentStringsW, GetEnvironmentStringsW};
use zeroize::Zeroizing;

/// The parent's environment, minus every variable whose name matches one of
/// `excluded` case-insensitively.
///
/// The result is a complete environment block: sorted by name (case-insensitively,
/// with the hidden `=X:` entries kept in front, where the system puts them),
/// NUL-separated, and terminated by the empty entry `CreateProcessW` requires.
/// It is zeroed on drop: it carries the same secrets the parent's own block does.
pub(crate) fn inherited_block_without(excluded: &[&str]) -> io::Result<Zeroizing<Vec<u16>>> {
    // SAFETY: the returned block is owned by this process until it is freed, and
    // is only read while it is alive.
    let block = unsafe { GetEnvironmentStringsW() };
    if block.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut entries = Vec::new();
    let mut index = 0_usize;
    loop {
        // SAFETY: the block is a sequence of NUL-terminated strings ending in an
        // empty one, so this walk stays inside it.
        let start = unsafe { block.add(index) };
        let mut length = 0_usize;
        // SAFETY: same walk; the terminator is guaranteed by the API contract.
        while unsafe { *start.add(length) } != 0 {
            length += 1;
        }
        if length == 0 {
            break;
        }
        // SAFETY: the entry is `length` non-NUL UTF-16 units starting at `start`.
        entries.push(unsafe { std::slice::from_raw_parts(start, length) }.to_vec());
        index += length + 1;
    }
    // SAFETY: the block came from this API and is freed exactly once.
    unsafe {
        let _ = FreeEnvironmentStringsW(block);
    }

    let mut hidden = Vec::new();
    let mut named = Vec::new();
    for entry in entries {
        let text = OsString::from_wide(&entry);
        let Some((name, _value)) = split_entry(&text) else {
            continue;
        };
        if excluded.iter().any(|excluded| name.eq_ignore_ascii_case(excluded)) {
            continue;
        }
        if name.starts_with('=') {
            // Per-drive current directory: a name Windows treats as hidden, and
            // one an explicit block has to carry itself.
            hidden.push(entry);
        } else {
            named.push(entry);
        }
    }
    named.sort_by_key(|entry| {
        let text = OsString::from_wide(entry);
        split_entry(&text).map(|(name, _)| name.to_uppercase()).unwrap_or_default()
    });

    let mut block = Zeroizing::new(Vec::new());
    for entry in hidden.into_iter().chain(named) {
        block.extend(entry);
        block.push(0);
    }
    // An environment block always ends with an empty entry, even when it has none.
    block.push(0);
    Ok(block)
}

/// One `NAME=VALUE` entry from the raw block. A hidden per-drive entry
/// (`=C:=C:\dir`) has no value beyond its own text, so it is reported as a name
/// with an empty value rather than rejected.
fn split_entry(entry: &OsString) -> Option<(String, String)> {
    let text = entry.to_string_lossy();
    if text.is_empty() {
        return None;
    }
    match text.find('=') {
        Some(0) => {
            let rest = &text[1..];
            let end = rest.find('=')? + 1;
            Some((text[..end].to_string(), rest[end..].to_string()))
        }
        Some(index) => Some((text[..index].to_string(), text[index + 1..].to_string())),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn entries(block: &[u16]) -> Vec<String> {
        let mut result = Vec::new();
        let mut start = 0_usize;
        while start < block.len() && block[start] != 0 {
            let mut end = start;
            while block[end] != 0 {
                end += 1;
            }
            result.push(String::from_utf16_lossy(&block[start..end]));
            start = end + 1;
        }
        result
    }

    /// The block the parent really has, minus the one name asked for. Everything
    /// else survives byte for byte: this is an inheritance boundary, not a
    /// rewrite of the user's environment.
    #[test]
    fn the_parent_block_is_preserved_except_for_the_named_variable() {
        let block = inherited_block_without(&["__COMPAT_LAYER"]).unwrap();
        assert_eq!(block.last(), Some(&0), "the block is NUL-terminated");
        let present = entries(&block);
        assert!(present.iter().all(|entry| !entry.to_uppercase().starts_with("__COMPAT_LAYER=")));
        assert_eq!(
            block.len(),
            present.iter().map(|entry| entry.encode_utf16().count() + 1).sum::<usize>() + 1,
            "every entry is NUL-terminated and the block ends with one more NUL"
        );
    }

    /// Names match the way the operating system matches them, and the block keeps
    /// the hidden per-drive entries an explicit environment replaces.
    #[test]
    fn exclusion_is_case_insensitive_and_hidden_entries_survive() {
        let parent = inherited_block_without(&[]).unwrap();
        let hidden = entries(&parent).into_iter().filter(|entry| entry.starts_with('=')).count();
        for name in ["__COMPAT_LAYER", "__compat_layer", "__ComPat_LaYeR"] {
            let filtered = inherited_block_without(&[name.to_uppercase().as_str()]).unwrap();
            assert_eq!(
                entries(&filtered).into_iter().filter(|entry| entry.starts_with('=')).count(),
                hidden,
                "hidden per-drive entries are not environment variables to drop"
            );
        }
        // The raw block carries the same entries in both spellings of the filter.
        let upper = inherited_block_without(&["__COMPAT_LAYER"]).unwrap();
        let lower = inherited_block_without(&["__compat_layer"]).unwrap();
        assert_eq!(upper, lower);
    }

    /// Unicode names and values survive the round trip, including the values a
    /// render child needs for a non-ASCII workspace path.
    #[test]
    fn unicode_names_and_values_round_trip() {
        let block = inherited_block_without(&["__COMPAT_LAYER"]).unwrap();
        let present = entries(&block);
        assert!(present.iter().all(|entry| !entry.to_uppercase().starts_with("__COMPAT_LAYER=")));
        for entry in std::env::vars_os() {
            let (name, value) = (entry.0.to_string_lossy().into_owned(), entry.1.to_string_lossy());
            if name.is_empty() || name.contains('=') || name.eq_ignore_ascii_case("__COMPAT_LAYER")
            {
                continue;
            }
            let expected = format!("{name}={value}");
            assert!(
                present.iter().any(|entry| entry == &expected),
                "`{name}` did not survive the environment boundary"
            );
        }
        const CHILD: &str = "AURA_CHILD_ENVIRONMENT_UNICODE_FIXTURE";
        const NAME: &str = "AURA_环境边界";
        const VALUE: &str = "C:\\工作区\\ß.txt;𐐨;🦀";
        if std::env::var_os(CHILD).is_some() {
            assert_eq!(std::env::var_os(NAME), Some(OsString::from(VALUE)));
            assert!(present.iter().any(|entry| entry == &format!("{NAME}={VALUE}")));
        } else {
            // Give a private child real Unicode and an excluded compatibility
            // variable; never change the test process or the host environment.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "child_environment::tests::unicode_names_and_values_round_trip",
                    "--exact",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .env(NAME, VALUE)
                .env("__ComPat_LaYeR", "DetectorsAppHealth")
                .output()
                .unwrap();
            assert!(output.status.success(), "isolated Unicode fixture failed: {}", output.status);
        }
    }
}
