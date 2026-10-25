//! The shell's folder picker.
use std::{io, path::PathBuf, ptr};

use windows_sys::Win32::{
    Foundation::HWND,
    System::Com::CoTaskMemFree,
    UI::Shell::{
        BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW,
        SHGetPathFromIDListW,
    },
};

use super::wide_null;

/// A folder the user picks, or `None` when they cancel. `owner` may be null.
pub fn choose_folder(owner: HWND, title: &str) -> io::Result<Option<PathBuf>> {
    let title = wide_null(title);
    let mut display = vec![0_u16; 260];
    let info = BROWSEINFOW {
        hwndOwner: owner,
        pidlRoot: ptr::null_mut(),
        pszDisplayName: display.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    // SAFETY: `info` and its buffers are live for the modal call.
    let chosen = unsafe { SHBrowseForFolderW(&info) };
    if chosen.is_null() {
        return Ok(None);
    }
    let mut path = vec![0_u16; 32_768];
    // SAFETY: the item id list came from the picker and the buffer is large enough for any path.
    let ok = unsafe { SHGetPathFromIDListW(chosen, path.as_mut_ptr()) };
    // SAFETY: the picker allocated the item id list with the COM allocator.
    unsafe { CoTaskMemFree(chosen.cast()) };
    if ok == 0 {
        return Err(io::Error::other("the chosen item is not a folder on disk"));
    }
    let length = path.iter().position(|unit| *unit == 0).unwrap_or(path.len());
    Ok(Some(PathBuf::from(String::from_utf16_lossy(&path[..length]))))
}
