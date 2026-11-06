//! Bringing a program's window to the front, minimizing, maximizing, restoring or closing it.
//! Size changes and closing are posted to the window's own thread, so a busy program cannot hold
//! the host up; the host then waits a little for the window to show the change.
use super::apps::windows_of;
use super::target::reachable_window;
use crate::arrange::{self, Show, Step};
use crate::native::{focus, window_state};
use aura_desktop_domain::{
    AppEntry, Delivery, HostError, HostErrorCode, HostResult, WindowAction, window_handle,
};
use std::time::{Duration, Instant};

/// How long the host waits for the window to change.
const SETTLE: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);

pub fn window(
    app: &AppEntry,
    window: Option<&str>,
    action: WindowAction,
) -> Result<HostResult, HostError> {
    let target = reachable_window(app, window)?;
    let gone = || {
        HostError::new(
            HostErrorCode::WindowNotFound,
            format!("{} 的这个窗口已经关了", app.name),
            Delivery::NotDelivered,
        )
    };
    let handle = window_handle(&target.window).ok_or_else(gone)?;
    if let Some(why) = arrange::refusal(action, window_state::buttons(handle)) {
        return Err(HostError::new(HostErrorCode::Unsupported, why, Delivery::NotDelivered));
    }
    let now = window_state::shown(handle);
    let changed = match arrange::step(action, now, target.foreground) {
        Step::Nothing => false,
        Step::Front => {
            if !focus::bring_to_front(handle) {
                // A minimized window was restored on the way.
                let delivery =
                    if now.minimized { Delivery::Uncertain } else { Delivery::NotDelivered };
                return Err(HostError::new(
                    HostErrorCode::NotForeground,
                    format!("Windows 没让 {} 的窗口到前面：用户正在用别的程序时会这样", app.name),
                    delivery,
                ));
            }
            true
        }
        Step::Close => {
            if !window_state::close(handle) {
                return Err(gone());
            }
            // A program that asks whether to save keeps the window; the reply lists it.
            settle(|| !focus::exists(handle));
            true
        }
        Step::Show(how) => {
            let posted = match how {
                Show::Minimize => window_state::minimize(handle, false),
                Show::MinimizeBehind => window_state::minimize(handle, true),
                Show::Maximize => window_state::maximize(handle),
                Show::Normal => window_state::normal(handle),
            };
            if !posted {
                return Err(gone());
            }
            if !settle(|| arrange::shows(action, window_state::shown(handle))) {
                return Err(HostError::new(
                    HostErrorCode::Timeout,
                    format!(
                        "{} 的窗口 {} 秒内没有变：它可能稍后才变，先读取再决定",
                        app.name,
                        SETTLE.as_secs()
                    ),
                    Delivery::Uncertain,
                ));
            }
            // Windows lets a size change of a program in the background stay there; the reply's
            // list says when the window could not be brought to the front.
            if arrange::brings_to_front(action) {
                focus::bring_to_front(handle);
            }
            true
        }
    };
    Ok(HostResult::Window { window: target.window, changed, windows: windows_of(app) })
}

/// Waits up to [`SETTLE`] for `ready`; true when it came.
fn settle(mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + SETTLE;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}
