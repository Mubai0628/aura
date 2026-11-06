//! Opening a program the way the Start menu does.
use super::text::wide;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

/// Opens `target` (`shell:AppsFolder\<ID>` or a program's path) with the Shell. The Shell may
/// hand the work to extensions that need a single-threaded apartment, so it runs on a thread of
/// its own; Windows shows no error dialog.
pub fn shell_open(target: &str) -> anyhow::Result<()> {
    let target = target.to_owned();
    std::thread::spawn(move || {
        // SAFETY: this thread joins COM, opens the target with strings that outlive the call,
        // and leaves COM again before it ends.
        unsafe {
            let joined = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            let file = wide(&target);
            let verb = wide("open");
            let mut info = SHELLEXECUTEINFOW {
                cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
                lpVerb: PCWSTR(verb.as_ptr()),
                lpFile: PCWSTR(file.as_ptr()),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            let opened = ShellExecuteExW(&raw mut info);
            if joined.is_ok() {
                CoUninitialize();
            }
            opened.map_err(|error| anyhow::anyhow!("{error}"))
        }
    })
    .join()
    .map_err(|_| anyhow::anyhow!("打开程序的线程意外结束"))?
}
