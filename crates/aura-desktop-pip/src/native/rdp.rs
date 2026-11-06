//! The Remote Desktop control in a window of ours, hosted with Windows' own ATL (`atl.dll` from
//! System32, never from the program's folder): its settings through its scriptable interface,
//! `ConnectToChildSession` through its extended settings, and its events through a sink of ours.
//! Nothing here reads, fills or stores a credential: Windows' own prompt asks the user.
use std::ffi::c_void;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{E_NOTIMPL, HWND, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{
    DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT, DISPPARAMS,
    EXCEPINFO, IConnectionPoint, IConnectionPointContainer, IDispatch, IDispatch_Impl,
    IDispatch_Vtbl, ITypeInfo,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::System::Ole::DISPID_PROPERTYPUT;
use windows::Win32::System::Variant::{
    VARIANT, VT_BOOL, VT_BSTR, VT_DISPATCH, VT_I4, VariantClear,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, WINDOW_EX_STYLE, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
};
use windows::core::{
    BOOL, BSTR, Error, GUID, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, Interface, PCWSTR, Result,
    implement, interface, s, w,
};

#[cfg(test)]
#[path = "rdp_tests.rs"]
mod tests;

/// The Remote Desktop client control that may be scripted with any setting.
const CONTROL: PCWSTR = w!("{A0C63C30-F08D-4AB4-907C-34905D770C7D}");
/// The control's events.
const EVENTS: GUID = GUID::from_u128(0x336d_5562_efa8_482e_8cb3_c5c0_fc7a_7db6);
// Advise queries the outgoing interface's IID, not just IDispatch.
#[interface("336D5562-EFA8-482E-8CB3-C5C0FC7A7DB6")]
unsafe trait IMsTscAxEvents: IDispatch {}
/// Settings 9 to 2: the newest the control has is used.
const ADVANCED: [&str; 4] =
    ["AdvancedSettings9", "AdvancedSettings8", "AdvancedSettings7", "AdvancedSettings2"];
const SECURED: [&str; 3] = ["SecuredSettings3", "SecuredSettings2", "SecuredSettings"];
/// Nothing of this computer goes to the child session.
const NOT_REDIRECTED: [&str; 8] = [
    "RedirectClipboard",
    "RedirectDrives",
    "RedirectPrinters",
    "RedirectPorts",
    "RedirectSmartCards",
    "RedirectDevices",
    "RedirectPOSDevices",
    "AudioCaptureRedirectionMode",
];

// A COM interface is called by the order of its methods; their names here are ours.
#[interface("302D8188-0052-4807-806A-362B628F9AC5")]
unsafe trait IMsRdpExtendedSettings: IUnknown {
    fn put_property(&self, name: *const u16, value: *const VARIANT) -> HRESULT;
    fn get_property(&self, name: *const u16, value: *mut VARIANT) -> HRESULT;
}

type AtlAxWinInit = unsafe extern "system" fn() -> BOOL;
type AtlAxGetControl = unsafe extern "system" fn(HWND, *mut *mut c_void) -> HRESULT;

/// What the control says happened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RdpEvent {
    Connecting,
    Connected,
    LoginComplete,
    Disconnected(i32),
    FatalError(i32),
    /// Windows shows its sign-in prompt.
    SigningIn,
}

/// The control's window and the control.
pub(crate) struct Rdp {
    pub(crate) window: HWND,
    control: IDispatch,
    advised: Option<(IConnectionPoint, u32)>,
}

impl Rdp {
    /// Hosts a new control in a child window of `parent`; `events` hears what it says.
    pub(crate) fn create(parent: HWND, events: impl Fn(RdpEvent) + 'static) -> Result<Self> {
        // SAFETY: the library is loaded from System32 only and stays loaded for the process; the
        // two functions are ATL's exported entry points with these signatures.
        let (init, get) = unsafe {
            let atl = LoadLibraryExW(w!("atl.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)?;
            let init = GetProcAddress(atl, s!("AtlAxWinInit")).ok_or_else(Error::from_thread)?;
            let get = GetProcAddress(atl, s!("AtlAxGetControl")).ok_or_else(Error::from_thread)?;
            (
                std::mem::transmute::<unsafe extern "system" fn() -> isize, AtlAxWinInit>(init),
                std::mem::transmute::<unsafe extern "system" fn() -> isize, AtlAxGetControl>(get),
            )
        };
        // SAFETY: registers ATL's host window class on this thread, which runs a message loop.
        if !unsafe { init() }.as_bool() {
            return Err(Error::from_thread());
        }
        // SAFETY: the host window class was registered above; the window is a child of `parent`.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("AtlAxWin"),
                CONTROL,
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                320,
                200,
                Some(parent),
                None,
                None,
                None,
            )
        }?;
        let mut unknown = std::ptr::null_mut();
        // SAFETY: `window` hosts the control; the pointer comes back with a reference we own.
        unsafe { get(window, &raw mut unknown) }.ok()?;
        // SAFETY: a valid interface pointer with one reference, which `from_raw` takes over.
        let unknown = unsafe { IUnknown::from_raw(unknown) };
        let control: IDispatch = unknown.cast()?;
        let mut rdp = Self { window, control, advised: None };
        rdp.advise(events)?;
        Ok(rdp)
    }

    fn advise(&mut self, events: impl Fn(RdpEvent) + 'static) -> Result<()> {
        let container: IConnectionPointContainer = self.control.cast()?;
        // SAFETY: `EVENTS` is the control's event interface, which it fires.
        let point = unsafe { container.FindConnectionPoint(&EVENTS) }?;
        let sink: IMsTscAxEvents = Sink { events: Box::new(events) }.into();
        let unknown: IUnknown = sink.cast()?;
        // SAFETY: the sink stays alive as long as the connection holds a reference to it.
        let cookie = unsafe { point.Advise(&unknown) }?;
        self.advised = Some((point, cookie));
        Ok(())
    }

    /// Settings for a connection to this user's child session of `width` by `height` pixels:
    /// nothing redirected, Windows key chords kept on this computer, no sound, the picture scaled
    /// to the window.
    pub(crate) fn configure(&self, width: u32, height: u32) -> Result<()> {
        put(&self.control, "Server", text("localhost"))?;
        put(&self.control, "DesktopWidth", number(i32::try_from(width).unwrap_or(1280)))?;
        put(&self.control, "DesktopHeight", number(i32::try_from(height).unwrap_or(800)))?;
        let advanced = first_object(&self.control, &ADVANCED)?;
        for name in NOT_REDIRECTED {
            put(&advanced, name, boolean(false))?;
        }
        put(&advanced, "GrabFocusOnConnect", boolean(false))?;
        put(&advanced, "EnableAutoReconnect", boolean(false))?;
        put(&advanced, "SmartSizing", boolean(true))?;
        put(&advanced, "EnableCredSspSupport", boolean(true))?;
        // The connection is to this computer's own listener; a certificate warning would only
        // stand in the way.
        put(&advanced, "AuthenticationLevel", number(0))?;
        let secured = first_object(&self.control, &SECURED)?;
        put(&secured, "KeyboardHookMode", number(0))?;
        put(&secured, "AudioRedirectionMode", number(2))?;
        let extended: IMsRdpExtendedSettings = self.control.cast()?;
        let name = BSTR::from("ConnectToChildSession");
        let on = boolean(true);
        // SAFETY: both arguments outlive the call; the control copies what it keeps.
        unsafe { extended.put_property(name.as_ptr(), &raw const on) }.ok()
    }

    pub(crate) fn connect(&self) -> Result<()> {
        call(&self.control, "Connect")
    }

    pub(crate) fn disconnect(&self) {
        if self.connected() != 0 {
            let _ = call(&self.control, "Disconnect");
        }
    }

    /// 0 when not connected, 1 when connected, 2 while connecting.
    pub(crate) fn connected(&self) -> i32 {
        get(&self.control, "Connected").ok().and_then(|value| value.number()).unwrap_or(0)
    }
}

impl Drop for Rdp {
    fn drop(&mut self) {
        if let Some((point, cookie)) = self.advised.take() {
            // SAFETY: the cookie is the one `Advise` returned for this connection point.
            let _ = unsafe { point.Unadvise(cookie) };
        }
    }
}

#[implement(IMsTscAxEvents)]
struct Sink {
    events: Box<dyn Fn(RdpEvent)>,
}

impl IMsTscAxEvents_Impl for Sink_Impl {}

impl IDispatch_Impl for Sink_Impl {
    fn GetTypeInfoCount(&self) -> Result<u32> {
        Ok(0)
    }

    fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> {
        Err(E_NOTIMPL.into())
    }

    fn GetIDsOfNames(
        &self,
        _: *const GUID,
        _: *const PCWSTR,
        _: u32,
        _: u32,
        _: *mut i32,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Invoke(
        &self,
        member: i32,
        _: *const GUID,
        _: u32,
        _: DISPATCH_FLAGS,
        parameters: *const DISPPARAMS,
        _: *mut VARIANT,
        _: *mut EXCEPINFO,
        _: *mut u32,
    ) -> Result<()> {
        // SAFETY: the control passes valid parameters for the event it fires, or none.
        let first = || unsafe {
            let parameters = parameters.as_ref()?;
            if parameters.cArgs == 0 || parameters.rgvarg.is_null() {
                return None;
            }
            // The arguments come last first; the events read here have one.
            let first = &*parameters.rgvarg.add(parameters.cArgs as usize - 1);
            (first.Anonymous.Anonymous.vt == VT_I4)
                .then(|| first.Anonymous.Anonymous.Anonymous.lVal)
        };
        // The event ids of the control's event interface (IMsTscAxEvents).
        let event = match member {
            1 => RdpEvent::Connecting,
            2 => RdpEvent::Connected,
            3 => RdpEvent::LoginComplete,
            4 => RdpEvent::Disconnected(first().unwrap_or(0)),
            10 => RdpEvent::FatalError(first().unwrap_or(0)),
            18 => RdpEvent::SigningIn,
            _ => return Ok(()),
        };
        (self.events)(event);
        Ok(())
    }
}

/// A VARIANT this value owns and clears.
struct Owned(VARIANT);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: clears a VARIANT this value owns, once.
        unsafe {
            let _ = VariantClear(&raw mut self.0);
        }
    }
}

impl Owned {
    fn number(&self) -> Option<i32> {
        // SAFETY: reads the member the type tag names.
        unsafe {
            let inner = &self.0.Anonymous.Anonymous;
            (inner.vt == VT_I4).then(|| inner.Anonymous.lVal)
        }
    }

    fn object(&self) -> Option<IDispatch> {
        // SAFETY: reads the member the type tag names, and adds a reference of our own.
        unsafe {
            let inner = &self.0.Anonymous.Anonymous;
            if inner.vt != VT_DISPATCH {
                return None;
            }
            (*inner.Anonymous.pdispVal).clone()
        }
    }
}

fn boolean(value: bool) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: writes the type tag of a zeroed VARIANT and the member the tag names.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_BOOL;
        inner.Anonymous.boolVal = if value { VARIANT_TRUE } else { VARIANT_FALSE };
    }
    variant
}

fn number(value: i32) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: as in `boolean`.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = value;
    }
    variant
}

/// A VARIANT holding a string; its owner clears it.
fn text(value: &str) -> Owned {
    let mut variant = VARIANT::default();
    // SAFETY: as in `boolean`; the string is freed when the VARIANT is cleared.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_BSTR;
        inner.Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(value));
    }
    Owned(variant)
}

trait Argument {
    fn raw(&self) -> VARIANT;
}

// A bitwise copy for the argument array: the callee does not free its arguments, and the owner
// still clears the original once.
impl Argument for VARIANT {
    fn raw(&self) -> VARIANT {
        VARIANT { Anonymous: self.Anonymous.clone() }
    }
}

impl Argument for Owned {
    fn raw(&self) -> VARIANT {
        VARIANT { Anonymous: self.0.Anonymous.clone() }
    }
}

fn member(object: &IDispatch, name: &str) -> Result<i32> {
    let name = HSTRING::from(name);
    let names = [PCWSTR(name.as_ptr())];
    let mut id = 0;
    // SAFETY: one name, valid for the call, and one writable id.
    unsafe { object.GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, 0, &raw mut id) }
        .map_err(|error| Error::new(error.code(), format!("{name}: {error}")))?;
    Ok(id)
}

fn invoke(
    object: &IDispatch,
    name: &str,
    flags: DISPATCH_FLAGS,
    argument: Option<VARIANT>,
) -> Result<Owned> {
    let id = member(object, name)?;
    let mut arguments: Vec<VARIANT> = argument.into_iter().collect();
    let mut named = DISPID_PROPERTYPUT;
    let parameters = DISPPARAMS {
        rgvarg: if arguments.is_empty() { std::ptr::null_mut() } else { arguments.as_mut_ptr() },
        rgdispidNamedArgs: if flags == DISPATCH_PROPERTYPUT {
            &raw mut named
        } else {
            std::ptr::null_mut()
        },
        cArgs: u32::try_from(arguments.len()).unwrap_or(0),
        cNamedArgs: u32::from(flags == DISPATCH_PROPERTYPUT),
    };
    let mut result = Owned(VARIANT::default());
    // SAFETY: the parameters and the result outlive the call.
    unsafe {
        object.Invoke(
            id,
            &GUID::zeroed(),
            0,
            flags,
            &raw const parameters,
            Some(&raw mut result.0),
            None,
            None,
        )
    }?;
    Ok(result)
}

fn put(object: &IDispatch, name: &str, value: impl Argument) -> Result<()> {
    invoke(object, name, DISPATCH_PROPERTYPUT, Some(value.raw())).map(|_| ())
}

fn get(object: &IDispatch, name: &str) -> Result<Owned> {
    invoke(object, name, DISPATCH_PROPERTYGET, None)
}

fn call(object: &IDispatch, name: &str) -> Result<()> {
    invoke(object, name, DISPATCH_METHOD, None).map(|_| ())
}

/// The first of `names` the control has, as an object.
fn first_object(object: &IDispatch, names: &[&str]) -> Result<IDispatch> {
    for name in names {
        if let Some(found) = get(object, name).ok().and_then(|value| value.object()) {
            return Ok(found);
        }
    }
    Err(Error::new(E_NOTIMPL, format!("没有 {}", names.join("、"))))
}
