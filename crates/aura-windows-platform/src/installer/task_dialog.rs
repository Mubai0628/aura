//! Installer windows built on the system task dialog: a question with buttons, links, a check box
//! and replaceable content, or a progress bar that follows work running on another thread.
use std::{
    io,
    mem::size_of,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
};

use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, S_FALSE, S_OK, WPARAM},
        UI::{
            Controls::{
                TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDE_CONTENT,
                TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER, TDF_ENABLE_HYPERLINKS,
                TDF_SHOW_PROGRESS_BAR, TDF_SIZE_TO_CONTENT, TDF_VERIFICATION_FLAG_CHECKED,
                TDM_CLICK_BUTTON, TDM_SET_ELEMENT_TEXT, TDM_SET_PROGRESS_BAR_POS,
                TDN_BUTTON_CLICKED, TDN_CREATED, TDN_HYPERLINK_CLICKED, TDN_TIMER,
                TaskDialogIndirect,
            },
            WindowsAndMessaging::{IDCANCEL, IDOK, SendMessageW},
        },
    },
    core::{HRESULT, PCWSTR},
};

use super::wide_null;

/// What a question window shows.
pub struct Question<'a> {
    pub title: &'a str,
    pub instruction: &'a str,
    /// May contain `<a href="...">` links when `on_link` is given.
    pub content: &'a str,
    pub footer: Option<&'a str>,
    /// Custom buttons as (id, label); ids from 100 up.
    pub buttons: &'a [(i32, &'a str)],
    pub cancel: bool,
    pub check_box: Option<(&'a str, bool)>,
}

/// The button pressed (`None` for cancel) and the check box state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Answer {
    pub button: Option<i32>,
    pub checked: bool,
}

/// A link clicked: its `href`, and the window to parent further windows on. The returned text, if
/// any, replaces the content.
pub type LinkHandler<'a> = dyn FnMut(&str, HWND) -> Option<String> + 'a;

struct QuestionState<'a> {
    on_link: Option<&'a mut LinkHandler<'a>>,
    replaced: Vec<u16>,
}

unsafe extern "system" fn question_callback(
    hwnd: HWND,
    message: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
    data: isize,
) -> HRESULT {
    if message as i32 == TDN_HYPERLINK_CLICKED && data != 0 {
        // SAFETY: `data` points to the QuestionState owned by `ask` for the dialog's lifetime.
        let state = unsafe { &mut *(data as *mut QuestionState<'_>) };
        // SAFETY: for this notification lparam is a NUL-terminated href.
        let href = unsafe { pcwstr_to_string(lparam as PCWSTR) };
        if let Some(handler) = state.on_link.as_deref_mut()
            && let Some(content) = handler(&href, hwnd)
        {
            state.replaced = wide_null(&content);
            // SAFETY: hwnd is the live dialog and the text buffer outlives the dialog.
            unsafe {
                SendMessageW(
                    hwnd,
                    TDM_SET_ELEMENT_TEXT as u32,
                    TDE_CONTENT as usize,
                    state.replaced.as_ptr() as isize,
                )
            };
        }
    }
    S_OK
}

/// Shows a question and waits for the answer.
pub fn ask<'h>(
    question: &Question<'_>,
    on_link: Option<&'h mut LinkHandler<'h>>,
) -> io::Result<Answer> {
    let title = wide_null(question.title);
    let instruction = wide_null(question.instruction);
    let content = wide_null(question.content);
    let footer = question.footer.map(wide_null);
    let labels: Vec<Vec<u16>> =
        question.buttons.iter().map(|(_, label)| wide_null(label)).collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = question
        .buttons
        .iter()
        .zip(&labels)
        .map(|((id, _), label)| TASKDIALOG_BUTTON { nButtonID: *id, pszButtonText: label.as_ptr() })
        .collect();
    let check_label = question.check_box.map(|(label, _)| wide_null(label));
    let mut state = QuestionState { on_link, replaced: Vec::new() };
    let mut flags = TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT | TDF_ENABLE_HYPERLINKS;
    if question.check_box.is_some_and(|(_, checked)| checked) {
        flags |= TDF_VERIFICATION_FLAG_CHECKED;
    }
    let mut config = base_config(&title, &instruction, &content, flags)?;
    config.dwCommonButtons = if question.cancel { TDCBF_CANCEL_BUTTON } else { 0 };
    config.cButtons = u32::try_from(buttons.len()).unwrap_or(0);
    config.pButtons = buttons.as_ptr();
    config.nDefaultButton = question.buttons.first().map_or(IDCANCEL, |(id, _)| *id);
    config.pszFooter = footer.as_ref().map_or(ptr::null(), |text| text.as_ptr());
    config.pszVerificationText = check_label.as_ref().map_or(ptr::null(), |text| text.as_ptr());
    config.pfCallback = Some(question_callback);
    config.lpCallbackData = (&raw mut state) as isize;
    let mut pressed = IDCANCEL;
    let mut checked = 0;
    // SAFETY: every pointer in `config` refers to buffers that outlive this modal call.
    let result =
        unsafe { TaskDialogIndirect(&config, &mut pressed, ptr::null_mut(), &mut checked) };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    Ok(Answer { button: (pressed != IDCANCEL).then_some(pressed), checked: checked != 0 })
}

/// Progress reported by work running under a progress window: a share in thousandths.
#[derive(Clone)]
pub struct Progress {
    done: Arc<AtomicU32>,
    finished: Arc<AtomicBool>,
}

impl Progress {
    pub fn set(&self, done: u64, total: u64) {
        let share = done.saturating_mul(1_000).checked_div(total).unwrap_or(0).min(1_000);
        self.done.store(u32::try_from(share).unwrap_or(1_000), Ordering::Relaxed);
    }
}

struct ProgressState {
    progress: Progress,
}

unsafe extern "system" fn progress_callback(
    hwnd: HWND,
    message: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
    data: isize,
) -> HRESULT {
    // SAFETY: `data` points to the ProgressState owned by `run_with_progress` for the dialog.
    let state = unsafe { &*(data as *const ProgressState) };
    let message = message as i32;
    if message == TDN_CREATED || message == TDN_TIMER {
        let done = state.progress.done.load(Ordering::Relaxed);
        // SAFETY: hwnd is the live dialog.
        unsafe { SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_POS as u32, (done / 10) as usize, 0) };
        if state.progress.finished.load(Ordering::Acquire) {
            // SAFETY: as above; the dialog closes as if OK were pressed.
            unsafe { SendMessageW(hwnd, TDM_CLICK_BUTTON as u32, IDOK as usize, 0) };
        }
    }
    if message == TDN_BUTTON_CLICKED && !state.progress.finished.load(Ordering::Acquire) {
        // The window stays until the work has finished.
        return S_FALSE;
    }
    S_OK
}

/// Runs `work` on another thread while a progress window follows it, and returns its result once it
/// has finished; the window cannot be closed before that.
pub fn run_with_progress<T: Send + 'static>(
    title: &str,
    instruction: &str,
    content: &str,
    work: impl FnOnce(&Progress) -> T + Send + 'static,
) -> io::Result<T> {
    let progress =
        Progress { done: Arc::new(AtomicU32::new(0)), finished: Arc::new(AtomicBool::new(false)) };
    let worker_progress = progress.clone();
    let worker = thread::spawn(move || {
        let result = work(&worker_progress);
        worker_progress.finished.store(true, Ordering::Release);
        result
    });
    let title = wide_null(title);
    let instruction = wide_null(instruction);
    let content = wide_null(content);
    let state = ProgressState { progress };
    let mut config =
        base_config(&title, &instruction, &content, TDF_SHOW_PROGRESS_BAR | TDF_CALLBACK_TIMER)?;
    config.dwCommonButtons = 0;
    config.pfCallback = Some(progress_callback);
    config.lpCallbackData = (&raw const state) as isize;
    // SAFETY: every pointer in `config` refers to buffers that outlive this modal call.
    let result =
        unsafe { TaskDialogIndirect(&config, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) };
    let value = worker.join().map_err(|_| io::Error::other("installer work panicked"))?;
    if result < 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    Ok(value)
}

fn base_config(
    title: &[u16],
    instruction: &[u16],
    content: &[u16],
    flags: i32,
) -> io::Result<TASKDIALOGCONFIG> {
    Ok(TASKDIALOGCONFIG {
        cbSize: u32::try_from(size_of::<TASKDIALOGCONFIG>())
            .map_err(|_| io::Error::other("TASKDIALOGCONFIG size overflow"))?,
        hwndParent: ptr::null_mut(),
        hInstance: ptr::null_mut(),
        dwFlags: flags,
        dwCommonButtons: 0,
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
        pfCallback: None,
        lpCallbackData: 0,
        cxWidth: 0,
    })
}

/// SAFETY: `text` is null or a NUL-terminated UTF-16 string.
unsafe fn pcwstr_to_string(text: PCWSTR) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: guaranteed NUL-terminated by the caller.
    let length = (0..).take_while(|&index| unsafe { *text.add(index) } != 0).count();
    // SAFETY: `length` units precede the terminator.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) })
}
