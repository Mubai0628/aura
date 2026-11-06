//! Which window of a program is in front. Windows puts a program's menu, dropdown or tooltip in
//! front as a top-level window of its own, without a title. The program is still the one in
//! front then, through the window that opened it; bringing that window to the front again would
//! close the menu.

/// One top-level window, in the list's front-to-back order.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub process_id: u32,
    /// One the lists name: titled, not a tool window or the desktop's own.
    pub listed: bool,
    pub foreground: bool,
}

/// The listed window that is in front: the window in front itself, or, when that one is not
/// listed (a menu), the frontmost listed window of its process.
#[must_use]
pub fn in_front(windows: &[Window]) -> Option<usize> {
    let front = windows.iter().position(|window| window.foreground)?;
    if windows[front].listed {
        return Some(front);
    }
    let process = windows[front].process_id;
    if process == 0 {
        return None;
    }
    windows.iter().position(|window| window.listed && window.process_id == process)
}
