use super::*;
use windows::Win32::System::Ole::OleUninitialize;
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, WS_OVERLAPPED};

#[test]
fn the_events_sink_implements_the_connection_points_requested_interface() {
    let sink: IMsTscAxEvents = Sink { events: Box::new(|_| {}) }.into();
    let mut pointer = std::ptr::null_mut();
    // SAFETY: the query writes one owned interface pointer, released below when successful.
    let result = unsafe { sink.query(&EVENTS, &raw mut pointer) };
    if result.is_ok() {
        // SAFETY: successful QueryInterface owns one reference.
        drop(unsafe { IUnknown::from_raw(pointer) });
    }
    assert!(result.is_ok(), "Remote Desktop cannot subscribe to the event sink: {result:?}");
}

#[test]
fn a_hidden_control_configures_without_connecting_or_reading_credentials() -> Result<()> {
    // SAFETY: this test owns the STA and a hidden parent; it never calls Connect or sends input.
    unsafe {
        crate::native::ole()?;
        let parent = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("Aura control fixture"),
            WS_OVERLAPPED,
            0,
            0,
            640,
            480,
            None,
            None,
            None,
            None,
        )?;
        let result = (|| {
            let rdp = Rdp::create(parent, |_| {})?;
            let advanced = first_object(&rdp.control, &ADVANCED)?;
            put(&advanced, "RedirectClipboard", boolean(true))?;
            put(&advanced, "AudioCaptureRedirectionMode", boolean(true))?;
            rdp.configure(1280, 800)?;
            for name in [
                "RedirectClipboard",
                "RedirectDrives",
                "RedirectPrinters",
                "RedirectPorts",
                "RedirectDevices",
                "RedirectSmartCards",
                "RedirectPOSDevices",
                "AudioCaptureRedirectionMode",
            ] {
                let value = get(&advanced, name)?;
                assert_eq!(value.0.Anonymous.Anonymous.vt, VT_BOOL, "{name}");
                assert_eq!(value.0.Anonymous.Anonymous.Anonymous.boolVal, VARIANT_FALSE, "{name}");
            }
            let secured = first_object(&rdp.control, &SECURED)?;
            assert_eq!(get(&secured, "AudioRedirectionMode")?.number(), Some(2));
            assert_eq!(get(&rdp.control, "DesktopWidth")?.number(), Some(1280));
            assert_eq!(get(&rdp.control, "DesktopHeight")?.number(), Some(800));
            assert_eq!(rdp.connected(), 0);
            Ok(())
        })();
        let _ = DestroyWindow(parent);
        OleUninitialize();
        result
    }
}
