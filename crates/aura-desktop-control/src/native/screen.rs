//! The screen: its monitors, whose desktop takes input now, and what it shows.
use aura_desktop_domain::{Monitor, Rect};
use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetMonitorInfoW, HDC,
    HMONITOR, MONITORINFO, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, GetUserObjectInformationW,
    OpenInputDesktop, UOI_NAME,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::core::BOOL;

/// Every monitor together, in physical pixels.
#[must_use]
pub fn virtual_screen() -> Rect {
    // SAFETY: plain queries of system metrics.
    unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

#[must_use]
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the Vec that `monitors` keeps alive during EnumDisplayMonitors.
        unsafe {
            let list = &mut *(list.0 as *mut Vec<Monitor>);
            let mut info =
                MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(monitor, &raw mut info).as_bool() {
                let (mut dpi, mut unused) = (96_u32, 96_u32);
                let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi, &raw mut unused);
                let area = info.rcMonitor;
                list.push(Monitor {
                    rect: Rect {
                        x: area.left,
                        y: area.top,
                        width: area.right - area.left,
                        height: area.bottom - area.top,
                    },
                    dpi,
                    primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
                });
            }
            BOOL(1)
        }
    }
    let mut list: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only pushes into `list` during this call.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&raw mut list as isize));
    }
    list
}

/// The name of the desktop that takes input now: `Default` while the user works; `Winlogon`
/// (or nothing this process may open) while the computer is locked or a UAC prompt shows.
#[must_use]
pub fn input_desktop() -> Option<String> {
    // SAFETY: the desktop handle is opened for reading its name and closed here.
    unsafe {
        let desktop =
            OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS).ok()?;
        let mut name = [0_u16; 128];
        let mut needed = 0_u32;
        let read = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            (name.len() * 2) as u32,
            Some(&raw mut needed),
        );
        let _ = CloseDesktop(desktop);
        read.ok()?;
        Some(super::text::from_buffer(&name))
    }
}

/// What the screen shows in `area`, as top-down rows of 32-bit BGRA pixels.
pub struct Pixels {
    pub area: Rect,
    pub bgra: Vec<u8>,
}

/// Copies `area` of the screen as the compositor shows it. Windows that asked not to be
/// captured (Aura's own notice among them) are not in it.
pub fn capture(area: Rect) -> anyhow::Result<Pixels> {
    anyhow::ensure!(!area.is_empty(), "截图的区域是空的");
    let (width, height) = (area.width, area.height);
    // SAFETY: every GDI object made here is selected out and deleted before returning; the
    // section's bits are copied while the bitmap is alive.
    unsafe {
        let screen = GetDC(None);
        anyhow::ensure!(!screen.is_invalid(), "拿不到屏幕");
        let memory = CreateCompatibleDC(Some(screen));
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let mut bits = std::ptr::null_mut();
        let section =
            CreateDIBSection(Some(memory), &raw const info, DIB_RGB_COLORS, &raw mut bits, None, 0);
        let result = match section {
            Ok(bitmap) => {
                let previous = SelectObject(memory, bitmap.into());
                let copied = BitBlt(
                    memory,
                    0,
                    0,
                    width,
                    height,
                    Some(screen),
                    area.x,
                    area.y,
                    SRCCOPY | CAPTUREBLT,
                );
                let pixels = copied.map(|()| {
                    let length = width as usize * height as usize * 4;
                    std::slice::from_raw_parts(bits.cast::<u8>(), length).to_vec()
                });
                SelectObject(memory, previous);
                let _ = DeleteObject(bitmap.into());
                pixels.map_err(|error| anyhow::anyhow!("截屏失败：{error}"))
            }
            Err(error) => Err(anyhow::anyhow!("截屏失败：{error}")),
        };
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        Ok(Pixels { area, bgra: result? })
    }
}
