//! The window on the screen is the picture drawn offscreen, at the scale of the monitor under
//! the pointer. Ignored by default: it shows a window and takes the focus for a moment, so it
//! runs by hand and on its own (the process turns per-monitor DPI awareness on first, as the
//! programs' manifests do), and only after a minute without keyboard or mouse input.
use std::{ffi::c_void, ptr, thread, time::Duration};

use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC,
        CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY,
        SelectObject,
    },
    System::SystemInformation::GetTickCount,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::GetWindowRect,
    },
};

use super::install_page;
use crate::{
    draw,
    testing::{Key, press, wait_for_window},
    window::PixelWindow,
};

const IDLE: Duration = Duration::from_secs(60);

fn idle() -> Duration {
    let mut input = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: plain queries into a local.
    unsafe {
        if !GetLastInputInfo(&mut input).as_bool() {
            return Duration::ZERO;
        }
        Duration::from_millis(u64::from(GetTickCount().wrapping_sub(input.dwTime)))
    }
}

/// The screen's pixels in `rect`, as 0x00RRGGBB.
fn capture(rect: RECT) -> Vec<u32> {
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    let mut pixels = vec![0_u32; (width * height) as usize];
    // SAFETY: a memory DC and a top-down 32-bit DIB of the rectangle's size, copied out and freed.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = ptr::null_mut();
        let bitmap = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .expect("a bitmap");
        let old = SelectObject(memory, bitmap.into());
        BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            rect.left,
            rect.top,
            SRCCOPY | CAPTUREBLT,
        )
        .expect("a copy");
        ptr::copy_nonoverlapping(bits.cast::<u32>(), pixels.as_mut_ptr(), pixels.len());
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
    pixels.iter().map(|pixel| pixel & 0x00ff_ffff).collect()
}

#[test]
#[ignore = "shows a window on the screen; run by hand, alone, while nobody uses the computer"]
fn the_window_on_the_screen_is_the_picture_drawn_offscreen() {
    if idle() < IDLE {
        eprintln!("someone used the computer in the last minute; not showing a window");
        return;
    }
    // SAFETY: sets this process's DPI awareness before it has any window.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        .expect("per-monitor awareness: run this test on its own");
    let page = install_page();
    let shown = page.clone();
    let showing = thread::spawn(move || {
        let mut page = shown;
        let mut window = PixelWindow::open(&page).expect("the window opens");
        window.show(&mut page).expect("the page runs")
    });
    let window = wait_for_window(Duration::from_secs(10)).expect("the window appears");
    thread::sleep(Duration::from_millis(800));
    let mut rect = RECT::default();
    // SAFETY: a query into a local about this process's window.
    unsafe { GetWindowRect(HWND(window as *mut _), &mut rect) }.expect("its place");
    let on_screen = capture(rect);
    press(window, Key::Escape);
    showing.join().expect("the window's thread ends");

    let width = (rect.right - rect.left) as u32;
    let (laid, image) = (1..=4)
        .map(|k| (k, draw(&page, k).expect("draws")))
        .find(|(_, (_, image))| image.width == width)
        .map(|(_, drawn)| drawn)
        .expect("a whole scale whose picture is the window's size");
    assert_eq!(image.height, (rect.bottom - rect.top) as u32);
    let k = image.width as i32 / (laid.scene.width + crate::theme::SHADOW_OFFSET);
    let (body_width, body_height) = ((laid.scene.width * k) as u32, (laid.scene.height * k) as u32);
    let mut differing = 0;
    for y in 0..body_height {
        for x in 0..body_width {
            let drawn = image.pixel(x, y) & 0x00ff_ffff;
            if on_screen[(y * image.width + x) as usize] != drawn {
                differing += 1;
            }
        }
    }
    eprintln!("window {}x{} at {k}x, {differing} differing pixels", image.width, image.height);
    assert_eq!(differing, 0, "the screen shows something else than the picture at {k}x");
}
