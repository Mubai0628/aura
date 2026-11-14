//! The save dialog for a diagnostics export; the launcher's and the setup program's other
//! windows are `aura-pixel-window`'s.
use std::{io, mem::size_of, path::PathBuf};

use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetSaveFileNameW, OFN_DONTADDTORECENT, OFN_EXPLORER, OFN_NOCHANGEDIR,
    OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

pub fn choose_diagnostic_export_path(default_name: &str) -> io::Result<Option<PathBuf>> {
    let mut file = vec![0_u16; 32_768];
    let default = wide_null(default_name);
    let copy_length = default.len().min(file.len());
    file[..copy_length].copy_from_slice(&default[..copy_length]);
    let filter: Vec<u16> = "JSON 诊断文件\0*.json\0所有文件\0*.*\0\0".encode_utf16().collect();
    let title = wide_null("导出 Aura P01 诊断");
    let extension = wide_null("json");
    let mut dialog = OPENFILENAMEW {
        lStructSize: u32::try_from(size_of::<OPENFILENAMEW>())
            .map_err(|_| io::Error::other("OPENFILENAMEW size overflow"))?,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: u32::try_from(file.len())
            .map_err(|_| io::Error::other("file dialog buffer size overflow"))?,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_DONTADDTORECENT
            | OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_OVERWRITEPROMPT
            | OFN_PATHMUSTEXIST,
        lpstrDefExt: extension.as_ptr(),
        ..OPENFILENAMEW::default()
    };
    // SAFETY: `dialog` and every pointer it contains remain valid while the
    // modal function runs; its output buffer is writable and sized correctly.
    if unsafe { GetSaveFileNameW(&mut dialog) } == 0 {
        // SAFETY: CommDlgExtendedError has no preconditions and must be called
        // immediately after the failed common-dialog call.
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 { Ok(None) } else { Err(io::Error::from_raw_os_error(error as i32)) };
    }
    let length = file.iter().position(|unit| *unit == 0).unwrap_or(file.len());
    let path = String::from_utf16(&file[..length])
        .map_err(|_| io::Error::other("save path contains invalid UTF-16"))?;
    Ok(Some(PathBuf::from(path)))
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
