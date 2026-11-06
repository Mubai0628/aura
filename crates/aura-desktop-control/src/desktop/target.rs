//! The window a read or an act is for, and the errors both give.
use super::apps::windows_of;
use crate::elements::UiError;
use crate::native::focus;
use aura_desktop_domain::{
    AppEntry, Delivery, HostError, HostErrorCode, WindowInfo, window_handle,
};

/// The window of `app` that a request names, or the program's front one, when it can be read
/// and acted on: it can be reached and is not minimized (nothing of it is on the screen).
pub fn target_window(app: &AppEntry, window: Option<&str>) -> Result<WindowInfo, HostError> {
    let target = reachable_window(app, window)?;
    if target.minimized {
        return Err(HostError::new(
            HostErrorCode::Unsupported,
            "这个窗口最小化着，屏幕上没有它的元素：先把它还原",
            Delivery::NotDelivered,
        ));
    }
    Ok(target)
}

/// The window of `app` that a request names, or the program's front one, when Aura may reach
/// it: the desktop is the user's, the program is not above Aura and the window answers messages.
pub fn reachable_window(app: &AppEntry, window: Option<&str>) -> Result<WindowInfo, HostError> {
    super::user_desktop()?;
    let windows = windows_of(app);
    let found = match window {
        None => windows.into_iter().next(),
        Some(reference) => windows.into_iter().find(|candidate| candidate.window == reference),
    };
    let target = found.ok_or_else(|| {
        HostError::new(
            HostErrorCode::WindowNotFound,
            match window {
                None => format!("{} 没有打开的窗口", app.name),
                Some(reference) => format!("窗口 {reference} 已经关了，或不是 {} 的", app.name),
            },
            Delivery::NotDelivered,
        )
    })?;
    if focus::above_us(target.process_id) {
        return Err(HostError::new(
            HostErrorCode::Refused,
            format!("{} 以管理员身份运行，Windows 不让 Aura 读它的窗口、操作它", app.name),
            Delivery::NotDelivered,
        ));
    }
    if window_handle(&target.window).is_some_and(focus::hung) {
        return Err(not_responding(&app.name, None, Delivery::NotDelivered, None));
    }
    Ok(target)
}

pub const STALE: &str = "编号过期：这个元素已经不在了，或者变了，请重新读取";

pub fn stale() -> HostError {
    HostError::new(HostErrorCode::Stale, STALE, Delivery::NotDelivered)
}

pub fn not_responding(
    name: &str,
    waited_ms: Option<u32>,
    delivery: Delivery,
    steps_done: Option<u32>,
) -> HostError {
    let message = match waited_ms {
        Some(waited) => format!("{name} 没有响应（等了 {:.1} 秒）", f64::from(waited) / 1000.0),
        None => format!("{name} 没有响应"),
    };
    HostError { code: HostErrorCode::Timeout, message, delivery, steps_done }
}

/// A failure reading the window or the element a read starts from.
pub fn read_error(name: &str, error: &UiError) -> HostError {
    match error {
        UiError::Gone => stale(),
        UiError::Timeout => not_responding(name, None, Delivery::NotDelivered, None),
        other => HostError::new(HostErrorCode::Internal, other.to_string(), Delivery::NotDelivered),
    }
}

/// A failure getting the window's own element: a window that is gone is not a stale element.
pub fn window_error(name: &str, error: &UiError) -> HostError {
    match error {
        UiError::Gone => HostError::new(
            HostErrorCode::WindowNotFound,
            format!("{name} 的这个窗口已经关了"),
            Delivery::NotDelivered,
        ),
        other => read_error(name, other),
    }
}
