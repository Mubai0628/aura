//! A screenshot of a program's windows (or of a part of the screen) in which only the windows of
//! the programs the user let Aura operate can be seen.
use super::apps::{is_app_window, is_shell_window, owner, top_level};
use crate::marks;
use crate::native::{self, window_list::RawWindow};
use crate::redact::{self, Layer};
use aura_desktop_domain::{
    AppEntry, Delivery, HostError, HostErrorCode, HostResult, Mark, Rect, ScreenMap, WindowInfo,
    fitted, window_ref,
};
use base64::Engine as _;
use std::collections::BTreeSet;

/// The screen's pixels as captured, before they are encoded.
pub struct Shot {
    /// The part of the virtual screen, in screen pixels.
    pub area: Rect,
    /// Rows of blue, green, red pixels, top to bottom.
    pub bgr: Vec<u8>,
    /// The windows of the shown programs that are in the area, front to back.
    pub windows: Vec<WindowInfo>,
}

impl Shot {
    /// The pixel at a screen point, when the point is in the area.
    #[must_use]
    pub fn pixel(&self, x: i32, y: i32) -> Option<[u8; 3]> {
        if !self.area.contains(x, y) {
            return None;
        }
        let index = ((y - self.area.y) as usize * self.area.width as usize
            + (x - self.area.x) as usize)
            * 3;
        self.bgr.get(index..index + 3).map(|pixel| [pixel[0], pixel[1], pixel[2]])
    }
}

/// Captures `region`, or without one the area around the shown programs' windows (the whole
/// screen when none is on it), painting over every window not of `show` when `paint` is on.
pub fn shot(show: &[AppEntry], paint: bool, region: Option<Rect>) -> Result<Shot, HostError> {
    super::user_desktop()?;
    let screen = native::screen::virtual_screen();
    let windows = top_level();
    let shown = shown_windows(show, &windows);
    let area = match region {
        Some(region) => redact::intersect(region, screen).ok_or_else(|| {
            HostError::new(
                HostErrorCode::InvalidRequest,
                "要截的区域不在屏幕上",
                Delivery::NotDelivered,
            )
        })?,
        None => crate::area::around(
            windows
                .iter()
                .filter(|window| !window.minimized && shown.contains(&window.handle))
                .map(|window| window.rect),
            screen,
        )
        .unwrap_or(screen),
    };
    let layers: Vec<Layer> = windows
        .iter()
        .filter(|window| !window.minimized)
        .map(|window| Layer { rect: window.rect, shown: shown.contains(&window.handle) })
        .collect();
    let pixels =
        native::screen::capture(area).map_err(|error| super::internal(error.to_string()))?;
    let mut bgr = redact::without_alpha(&pixels.bgra);
    if paint {
        redact::paint(&mut bgr, 3, &redact::visible(area, &layers));
    }
    let listed = windows
        .iter()
        .filter(|window| is_app_window(window) && shown.contains(&window.handle))
        .filter(|window| redact::intersect(window.rect, area).is_some())
        .map(|window| WindowInfo {
            window: window_ref(window.handle),
            title: window.title.clone(),
            app_id: owner(show, window).map(|app| app.id.clone()),
            process_id: window.process_id,
            process_name: window.process_name.clone(),
            rect: window.rect,
            minimized: window.minimized,
            foreground: window.foreground,
        })
        .collect();
    Ok(Shot { area, bgr, windows: listed })
}

pub fn take(
    show: &[AppEntry],
    paint: bool,
    max_long_edge: u32,
    region: Option<Rect>,
    marks: &[Mark],
) -> Result<HostResult, HostError> {
    let Shot { area, mut bgr, windows } = shot(show, paint, region)?;
    let (width, height) = (area.width as u32, area.height as u32);
    let (out_width, out_height) = fitted(width, height, max_long_edge);
    let scale = f64::from(width) / f64::from(out_width.max(1));
    marks::draw(&mut marks::Canvas { bgr: &mut bgr, area }, marks, scale);
    let png = native::image::png(width, height, &bgr, out_width, out_height)
        .map_err(|error| super::internal(format!("截图编码失败：{error}")))?;
    Ok(HostResult::Screenshot {
        png: base64::engine::general_purpose::STANDARD.encode(png),
        map: ScreenMap {
            left: area.x,
            top: area.y,
            width: out_width,
            height: out_height,
            screen_width: width,
            screen_height: height,
        },
        monitors: native::screen::monitors(),
        windows,
    })
}

/// The windows that may be seen: those of a program in `show`, and every other top-level window
/// of the same processes (their menus, tooltips and dialogs). The desktop and the taskbars never
/// show, even when Explorer, which draws them, may.
fn shown_windows(show: &[AppEntry], windows: &[RawWindow]) -> BTreeSet<isize> {
    let processes: BTreeSet<u32> = windows
        .iter()
        .filter(|window| !is_shell_window(window) && owner(show, window).is_some())
        .map(|window| window.process_id)
        .collect();
    windows
        .iter()
        .filter(|window| !is_shell_window(window))
        .filter(|window| owner(show, window).is_some() || processes.contains(&window.process_id))
        .map(|window| window.handle)
        .collect()
}
