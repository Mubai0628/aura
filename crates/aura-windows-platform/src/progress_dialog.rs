//! A small progress window for work that runs before any app window exists, such as the launcher
//! copying data before an update's first start. It appears only when the work outlasts a short
//! delay, shows how far it got, and closes itself when the work ends; it cannot cancel the work.
use std::{
    io,
    mem::size_of,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, S_FALSE, S_OK, WPARAM},
        System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        UI::{
            Controls::{
                TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDE_CONTENT, TDF_CALLBACK_TIMER,
                TDF_SHOW_PROGRESS_BAR, TDM_CLICK_BUTTON, TDM_ENABLE_BUTTON, TDM_SET_ELEMENT_TEXT,
                TDM_SET_MARQUEE_PROGRESS_BAR, TDM_SET_PROGRESS_BAR_MARQUEE,
                TDM_SET_PROGRESS_BAR_POS, TDM_SET_PROGRESS_BAR_RANGE, TDN_BUTTON_CLICKED,
                TDN_CREATED, TDN_TIMER,
            },
            WindowsAndMessaging::{IDCANCEL, SendMessageW},
        },
    },
    core::{BOOL, HRESULT},
};

use crate::dialog::wide_null;

/// Work shorter than this never shows a window, so a quick copy does not flash one.
const SHOW_AFTER: Duration = Duration::from_millis(500);
const RANGE: u64 = 1_000;

type TaskDialogIndirectFn =
    unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut BOOL) -> HRESULT;

/// Only version 6 of the common controls has TaskDialogIndirect, and a program gets that version
/// through its manifest. Looking it up here instead of importing it keeps a program without that
/// manifest (a test) loadable; there it simply shows no window.
fn task_dialog_indirect() -> Option<TaskDialogIndirectFn> {
    let name = wide_null("comctl32.dll");
    // SAFETY: `name` is NUL-terminated; only System32 is searched, so no planted copy loads.
    let module =
        unsafe { LoadLibraryExW(name.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return None;
    }
    // SAFETY: `module` is loaded and the export name is NUL-terminated.
    let address = unsafe { GetProcAddress(module, c"TaskDialogIndirect".as_ptr().cast()) }?;
    // SAFETY: every comctl32 that exports TaskDialogIndirect exports it with this signature.
    Some(unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, TaskDialogIndirectFn>(address)
    })
}

/// How far the work got, shared between the work and the window.
#[derive(Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
    finished: AtomicBool,
    /// Work that is not a copy names itself and shows a percentage instead of megabytes.
    stage: Mutex<Option<String>>,
    /// The work cannot say how far it got: the bar moves without a position.
    unmeasured: AtomicBool,
    /// What the window last showed of `unmeasured`.
    shown_unmeasured: AtomicBool,
}

impl Progress {
    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Release);
    }

    pub fn advance(&self, amount: u64) {
        self.done.fetch_add(amount, Ordering::AcqRel);
    }

    /// Sets how far the work got outright, for work that reports its own totals.
    pub fn set_done(&self, done: u64) {
        self.done.store(done, Ordering::Release);
    }

    /// Names the work's current stage; `unmeasured` when it cannot say how far it got.
    pub fn set_stage(&self, stage: &str, unmeasured: bool) {
        if let Ok(mut current) = self.stage.lock() {
            *current = Some(stage.to_owned());
        }
        self.unmeasured.store(unmeasured, Ordering::Release);
    }

    /// Done out of the total, in thousandths.
    fn permille(&self) -> u64 {
        let total = self.total.load(Ordering::Acquire);
        if total == 0 {
            return 0;
        }
        (u128::from(self.done.load(Ordering::Acquire)) * u128::from(RANGE) / u128::from(total))
            .min(u128::from(RANGE)) as u64
    }

    fn text(&self) -> String {
        const MIB: u64 = 1024 * 1024;
        let stage = self.stage.lock().ok().and_then(|stage| stage.clone());
        match stage {
            Some(stage) if self.unmeasured.load(Ordering::Acquire) => stage,
            Some(stage) => format!("{stage} {}%", self.permille() / 10),
            None => format!(
                "已复制 {} MB / {} MB",
                self.done.load(Ordering::Acquire) / MIB,
                self.total.load(Ordering::Acquire) / MIB
            ),
        }
    }
}

/// Runs `work` while a window titled `title` shows `instruction` and the progress `work` reports.
/// The window shows only if `work` is still running after a short delay, and closes when it ends.
pub fn with_progress_window<R>(
    title: &str,
    instruction: &str,
    work: impl FnOnce(&Progress) -> R,
) -> R {
    let progress = Arc::new(Progress::default());
    let shown = Arc::clone(&progress);
    let (title, instruction) = (title.to_owned(), instruction.to_owned());
    let window = thread::Builder::new().name("aura-progress-window".to_owned()).spawn(move || {
        let start = Instant::now();
        while start.elapsed() < SHOW_AFTER {
            if shown.finished.load(Ordering::Acquire) {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        // A window that cannot be shown leaves the work as it was.
        let _ = show(&title, &instruction, &shown);
    });
    let result = work(&progress);
    progress.finished.store(true, Ordering::Release);
    if let Ok(window) = window {
        let _ = window.join();
    }
    result
}

fn show(title: &str, instruction: &str, progress: &Arc<Progress>) -> io::Result<()> {
    let task_dialog_indirect = task_dialog_indirect()
        .ok_or_else(|| io::Error::other("TaskDialogIndirect needs common controls version 6"))?;
    let title = wide_null(title);
    let instruction = wide_null(instruction);
    let content = wide_null(&progress.text());
    let config = TASKDIALOGCONFIG {
        cbSize: u32::try_from(size_of::<TASKDIALOGCONFIG>())
            .map_err(|_| io::Error::other("TASKDIALOGCONFIG size overflow"))?,
        hwndParent: ptr::null_mut(),
        hInstance: ptr::null_mut(),
        dwFlags: TDF_SHOW_PROGRESS_BAR | TDF_CALLBACK_TIMER,
        dwCommonButtons: TDCBF_CANCEL_BUTTON,
        pszWindowTitle: title.as_ptr(),
        Anonymous1: Default::default(),
        pszMainInstruction: instruction.as_ptr(),
        pszContent: content.as_ptr(),
        cButtons: 0,
        pButtons: ptr::null(),
        nDefaultButton: 0,
        cRadioButtons: 0,
        pRadioButtons: ptr::null(),
        nDefaultRadioButton: 0,
        pszVerificationText: ptr::null(),
        pszExpandedInformation: ptr::null(),
        pszExpandedControlText: ptr::null(),
        pszCollapsedControlText: ptr::null(),
        Anonymous2: Default::default(),
        pszFooter: ptr::null(),
        pfCallback: Some(callback),
        lpCallbackData: Arc::as_ptr(progress) as isize,
        cxWidth: 0,
    };
    // SAFETY: every config pointer refers to a live NUL-terminated string, and the callback data
    // points at a `Progress` the caller's `Arc` keeps alive until this synchronous call returns.
    let result = unsafe {
        task_dialog_indirect(&raw const config, ptr::null_mut(), ptr::null_mut(), ptr::null_mut())
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    Ok(())
}

unsafe extern "system" fn callback(
    window: HWND,
    notification: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
    data: isize,
) -> i32 {
    // SAFETY: `data` is the `Progress` pointer `show` passed, alive for the dialog's lifetime.
    let progress = unsafe { &*(data as *const Progress) };
    let send = |message: i32, wparam: usize, lparam: isize| {
        // SAFETY: `window` is the live task dialog this callback belongs to.
        unsafe { SendMessageW(window, message as u32, wparam, lparam) };
    };
    match notification as i32 {
        TDN_CREATED => {
            send(TDM_SET_PROGRESS_BAR_RANGE, 0, (RANGE as isize) << 16);
            // Stopping halfway would leave a partial copy, so the button stays off.
            send(TDM_ENABLE_BUTTON, IDCANCEL as usize, 0);
            S_OK
        }
        TDN_TIMER => {
            let unmeasured = progress.unmeasured.load(Ordering::Acquire);
            if progress.shown_unmeasured.swap(unmeasured, Ordering::AcqRel) != unmeasured {
                send(TDM_SET_MARQUEE_PROGRESS_BAR, usize::from(unmeasured), 0);
                send(TDM_SET_PROGRESS_BAR_MARQUEE, usize::from(unmeasured), 30);
                if !unmeasured {
                    send(TDM_SET_PROGRESS_BAR_RANGE, 0, (RANGE as isize) << 16);
                }
            }
            if !unmeasured {
                send(TDM_SET_PROGRESS_BAR_POS, progress.permille() as usize, 0);
            }
            let text = wide_null(&progress.text());
            send(TDM_SET_ELEMENT_TEXT, TDE_CONTENT as usize, text.as_ptr() as isize);
            if progress.finished.load(Ordering::Acquire) {
                send(TDM_ENABLE_BUTTON, IDCANCEL as usize, 1);
                send(TDM_CLICK_BUTTON, IDCANCEL as usize, 0);
            }
            S_OK
        }
        // Only the work's end closes the window.
        TDN_BUTTON_CLICKED if !progress.finished.load(Ordering::Acquire) => S_FALSE,
        _ => S_OK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_work_returns_its_result_without_a_window() {
        let started = Instant::now();
        let result = with_progress_window("Aura", "复制数据", |progress| {
            progress.set_total(10);
            progress.advance(10);
            42
        });
        assert_eq!(result, 42);
        assert!(started.elapsed() < SHOW_AFTER * 4, "the window thread ended without showing");
    }

    #[test]
    fn slow_work_ends_on_its_own_even_where_no_window_can_show() {
        let started = Instant::now();
        let result = with_progress_window("Aura", "复制数据", |progress| {
            progress.set_total(10);
            thread::sleep(SHOW_AFTER * 2);
            progress.advance(10);
            "done"
        });
        assert_eq!(result, "done");
        assert!(started.elapsed() < SHOW_AFTER * 8, "the window closed when the work ended");
    }

    #[test]
    fn progress_is_counted_in_thousandths_and_capped() {
        let progress = Progress::default();
        assert_eq!(progress.permille(), 0);
        progress.set_total(4 * 1024 * 1024);
        progress.advance(1024 * 1024);
        assert_eq!(progress.permille(), 250);
        assert_eq!(progress.text(), "已复制 1 MB / 4 MB");
        progress.advance(10 * 1024 * 1024);
        assert_eq!(progress.permille(), RANGE);
    }

    #[test]
    fn a_named_stage_shows_its_percentage_or_only_its_name_while_unmeasured() {
        let progress = Progress::default();
        progress.set_total(200);
        progress.set_done(50);
        progress.set_stage("删除旧记录", false);
        assert_eq!(progress.text(), "删除旧记录 25%");
        progress.set_stage("整理数据库文件", true);
        assert_eq!(progress.text(), "整理数据库文件");
    }
}
