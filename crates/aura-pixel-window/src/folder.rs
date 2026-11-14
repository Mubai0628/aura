//! The system's folder picker, owned by one of these windows.
use std::path::PathBuf;

use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND},
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::Shell::{
            FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
            FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    },
    core::{HRESULT, HSTRING, Result},
};

/// A folder the user picks, or `None` when they cancel. `owner` is a window's handle, from
/// [`crate::PixelWindow::handle`], or `None`. Needs COM on this thread.
pub fn choose_folder(owner: Option<isize>, title: &str) -> Result<Option<PathBuf>> {
    // SAFETY: plain calls on a dialog this function owns; the chosen path is copied out before
    // its memory is freed.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let options = dialog.GetOptions()?;
        dialog.SetOptions(
            options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR,
        )?;
        dialog.SetTitle(&HSTRING::from(title))?;
        match dialog.Show(owner.map(|owner| HWND(owner as *mut _))) {
            Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                return Ok(None);
            }
            result => result?,
        }
        let item = dialog.GetResult()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let text = path.to_string();
        CoTaskMemFree(Some(path.0 as *const _));
        Ok(Some(PathBuf::from(text.map_err(|_| windows::core::Error::empty())?)))
    }
}
