//! Programs and their windows: listing them and opening a program.
use crate::catalog::{self, WindowFacts};
use crate::native::{self, window_list::RawWindow};
use aura_desktop_domain::{
    AppEntry, AppKind, Delivery, HostError, HostErrorCode, HostResult, WindowInfo, window_handle,
    window_ref,
};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// Windows the desktop itself draws: the wallpaper, the taskbars.
const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];
const LAUNCH_POLL: Duration = Duration::from_millis(200);

/// The windows a user works in, front to back: titled, not tool windows, not the desktop's own.
#[must_use]
pub fn app_windows(apps: &[AppEntry]) -> Vec<WindowInfo> {
    top_level().into_iter().filter(is_app_window).map(|window| info(apps, &window)).collect()
}

/// The desktop's top-level windows, front to back. While a program's menu or dropdown is in
/// front, the window it belongs to counts as in front instead (see [`crate::front`]).
pub(super) fn top_level() -> Vec<RawWindow> {
    let mut windows = native::window_list::top_level();
    let layers: Vec<crate::front::Window> = windows
        .iter()
        .map(|window| crate::front::Window {
            process_id: window.process_id,
            listed: is_app_window(window),
            foreground: window.foreground,
        })
        .collect();
    if let Some(front) = crate::front::in_front(&layers) {
        for (index, window) in windows.iter_mut().enumerate() {
            window.foreground = index == front;
        }
    }
    windows
}

pub(super) fn is_app_window(window: &RawWindow) -> bool {
    !window.tool_window
        && !window.title.trim().is_empty()
        && !window.rect.is_empty()
        && !is_shell_window(window)
}

pub(super) fn is_shell_window(window: &RawWindow) -> bool {
    SHELL_CLASSES.contains(&window.class.as_str())
}

/// The program a window belongs to. Aura's own windows belong to none.
pub(super) fn owner<'a>(apps: &'a [AppEntry], window: &RawWindow) -> Option<&'a AppEntry> {
    if crate::own::is_aura(window.process_id) {
        return None;
    }
    catalog::owner(apps, &facts(window))
}

fn facts(window: &RawWindow) -> WindowFacts<'_> {
    WindowFacts {
        aumid: window.aumid.as_deref(),
        process_path: window.process_path.as_deref(),
        process_name: &window.process_name,
    }
}

fn info(apps: &[AppEntry], window: &RawWindow) -> WindowInfo {
    WindowInfo {
        window: window_ref(window.handle),
        title: window.title.clone(),
        app_id: owner(apps, window).map(|app| app.id.clone()),
        process_id: window.process_id,
        process_name: window.process_name.clone(),
        rect: window.rect,
        minimized: window.minimized,
        foreground: window.foreground,
    }
}

/// The program's own windows, front to back.
#[must_use]
pub fn windows_of(app: &AppEntry) -> Vec<WindowInfo> {
    let apps = std::slice::from_ref(app);
    app_windows(apps)
        .into_iter()
        .filter(|window| window.app_id.as_deref() == Some(&app.id))
        .collect()
}

/// Opens the program unless it has a window already, then waits for a window of it that was not
/// there before: some programs start a launcher that hands over to another process, so the
/// process the Shell started says nothing. Either way its front window comes to the front: a
/// program is opened to be worked in.
pub fn launch(app: &AppEntry, timeout: Duration) -> Result<HostResult, HostError> {
    let before = windows_of(app);
    if !before.is_empty() {
        return Ok(HostResult::Launched { windows: to_front(app, before), already_running: true });
    }
    // A program the user added has its path as its ID.
    let target = match app.kind {
        AppKind::Manual => app.id.clone(),
        AppKind::Packaged | AppKind::Desktop => format!("shell:AppsFolder\\{}", app.id),
    };
    native::launch::shell_open(&target).map_err(|error| {
        HostError::new(
            HostErrorCode::LaunchFailed,
            format!("打不开 {}：{error}", app.name),
            Delivery::NotDelivered,
        )
    })?;
    let known: BTreeSet<String> = before.into_iter().map(|window| window.window).collect();
    let deadline = Instant::now() + timeout;
    loop {
        let new: Vec<WindowInfo> =
            windows_of(app).into_iter().filter(|window| !known.contains(&window.window)).collect();
        if !new.is_empty() {
            return Ok(HostResult::Launched {
                windows: to_front(app, new),
                already_running: false,
            });
        }
        if Instant::now() >= deadline {
            let message = if running_elsewhere(app) {
                format!(
                    "{0} 正在另一个 Windows 会话里运行（小窗和用户自己的桌面各是一个会话）。这类程序同一时间通常只能运行一份，所以这里没有出现它的窗口；可以请用户关掉那边的 {0} 后再打开。",
                    app.name
                )
            } else {
                format!("{} 已经打开，但 {} 秒内没有出现它的窗口", app.name, timeout.as_secs())
            };
            return Err(HostError::new(HostErrorCode::LaunchFailed, message, Delivery::Uncertain));
        }
        std::thread::sleep(LAUNCH_POLL);
    }
}

/// `shown`, after the first of them was brought to the front, as they are then. Windows may
/// refuse; the list then says it is not in front.
fn to_front(app: &AppEntry, shown: Vec<WindowInfo>) -> Vec<WindowInfo> {
    let Some(handle) = shown.first().and_then(|window| window_handle(&window.window)) else {
        return shown;
    };
    native::focus::bring_to_front(handle);
    let wanted: BTreeSet<&str> = shown.iter().map(|window| window.window.as_str()).collect();
    let now: Vec<WindowInfo> = windows_of(app)
        .into_iter()
        .filter(|window| wanted.contains(window.window.as_str()))
        .collect();
    if now.is_empty() { shown } else { now }
}

/// Whether the program runs in another Windows session than this host's.
fn running_elsewhere(app: &AppEntry) -> bool {
    let Some(name) = app.path.as_deref().and_then(catalog::file_name) else { return false };
    let Some(here) = native::process::session_of(std::process::id()) else { return false };
    let named = native::process::processes()
        .into_iter()
        .filter(|(_, process)| process.name.eq_ignore_ascii_case(name))
        .filter_map(|(id, _)| native::process::session_of(id))
        .collect::<Vec<_>>();
    elsewhere(here, &named)
}

/// Whether any of the sessions a program's processes run in is not `here`.
fn elsewhere(here: u32, sessions: &[u32]) -> bool {
    sessions.iter().any(|session| *session != here)
}

#[cfg(test)]
mod tests {
    use super::elsewhere;

    #[test]
    fn a_program_running_only_in_another_session_is_there_and_not_here() {
        assert!(elsewhere(3, &[1]), "only on the user's own desktop");
        assert!(elsewhere(1, &[1, 3]), "in both");
        assert!(!elsewhere(3, &[3]), "only here");
        assert!(!elsewhere(3, &[]), "not running");
    }

    #[test]
    fn this_process_has_a_session() {
        assert!(crate::native::process::session_of(std::process::id()).is_some());
    }
}
