//! UI Automation, the interface screen readers use: reading a window's elements and acting on
//! them. COM pointers stay inside [`Automation`] and [`UiElement`]; the rest of the crate passes
//! a `UiElement` back but cannot open it.
mod act;
mod node;
mod safety;

use crate::elements::{Node, UiError};
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::{InitVariantFromInt32Array, VariantClear};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation2, IUIAutomationCacheRequest, IUIAutomationCondition,
    IUIAutomationElement, TreeScope_Children, TreeScope_Element, TreeScope_Subtree,
    UIA_E_ELEMENTNOTAVAILABLE, UIA_E_ELEMENTNOTENABLED, UIA_E_INVALIDOPERATION, UIA_E_NOTSUPPORTED,
    UIA_E_TIMEOUT, UIA_IsOffscreenPropertyId, UIA_RuntimeIdPropertyId,
};

/// An element UI Automation found, with what a read needs cached on it.
pub struct UiElement(IUIAutomationElement);

pub struct Automation {
    uia: IUIAutomation2,
    /// Caches an element's own properties, in the raw view.
    own: IUIAutomationCacheRequest,
    /// Caches the properties of an element's children that are on the screen.
    children: IUIAutomationCacheRequest,
}

/// The failure of a call that gives no answer at all: no such element, no such pattern.
const EMPTY: i32 = 0;
const RPC_E_DISCONNECTED: u32 = 0x8001_0108;
const RPC_S_SERVER_UNAVAILABLE: u32 = 0x8007_06BA;
const CO_E_OBJNOTCONNECTED: u32 = 0x8004_01FD;
const ERROR_TIMEOUT: u32 = 0x8007_05B4;
const E_NOTIMPL: u32 = 0x8000_4001;

fn failed(error: &windows::core::Error) -> UiError {
    match error.code().0 as u32 {
        UIA_E_ELEMENTNOTAVAILABLE
        | RPC_E_DISCONNECTED
        | RPC_S_SERVER_UNAVAILABLE
        | CO_E_OBJNOTCONNECTED => UiError::Gone,
        UIA_E_TIMEOUT | ERROR_TIMEOUT => UiError::Timeout,
        UIA_E_ELEMENTNOTENABLED => UiError::NotEnabled,
        UIA_E_NOTSUPPORTED | UIA_E_INVALIDOPERATION | E_NOTIMPL => UiError::NotSupported,
        _ if error.code().0 == EMPTY => UiError::NotSupported,
        _ => UiError::Failed(format!("UI Automation：{error}")),
    }
}

impl Automation {
    /// UI Automation for the calling thread, which must have joined COM. Each call to a program
    /// gives up after `timeout`, and acting on an element never moves the keyboard focus to it
    /// unless the step is to focus it.
    pub fn new(timeout: Duration) -> Result<Self, UiError> {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX).max(1);
        // SAFETY: creates the in-process UI Automation object on a thread in COM, and sets it up
        // through its own methods.
        unsafe {
            let uia: IUIAutomation2 = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| failed(&error))?;
            uia.SetAutoSetFocus(false).map_err(|error| failed(&error))?;
            uia.SetConnectionTimeout(millis.min(2_000)).map_err(|error| failed(&error))?;
            uia.SetTransactionTimeout(millis).map_err(|error| failed(&error))?;
            let everything = uia.RawViewCondition().map_err(|error| failed(&error))?;
            let own = node::request(&uia, &everything, TreeScope_Element)
                .map_err(|error| failed(&error))?;
            let on_screen = on_screen(&uia).map_err(|error| failed(&error))?;
            let children = node::request(&uia, &on_screen, TreeScope_Children)
                .map_err(|error| failed(&error))?;
            Ok(Self { uia, own, children })
        }
    }

    /// The window's own element.
    pub fn window(&self, window: isize) -> Result<(UiElement, Node), UiError> {
        // SAFETY: a query of a window handle; a window that is gone fails the call.
        let element =
            unsafe { self.uia.ElementFromHandleBuildCache(HWND(window as *mut _), &self.own) }
                .map_err(|error| failed(&error))?;
        let node = self.node(&element);
        Ok((UiElement(element), node))
    }

    /// The element's children that are on the screen, in order.
    pub fn children(&self, of: &UiElement) -> Result<Vec<(UiElement, Node)>, UiError> {
        // SAFETY: calls on an element this object found; the array is read within its length.
        unsafe {
            let cached = of.0.BuildUpdatedCache(&self.children).map_err(|error| failed(&error))?;
            let array = match cached.GetCachedChildren() {
                Ok(array) => array,
                Err(error) if error.code().0 == EMPTY => return Ok(Vec::new()),
                Err(error) => return Err(failed(&error)),
            };
            let length = array.Length().map_err(|error| failed(&error))?;
            let mut children = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
            for index in 0..length {
                let element = array.GetElement(index).map_err(|error| failed(&error))?;
                let node = self.node(&element);
                children.push((UiElement(element), node));
            }
            Ok(children)
        }
    }

    /// The element under `root` (or `root` itself) with this runtime ID, if there is one.
    pub fn find(
        &self,
        root: &UiElement,
        runtime_id: &[i32],
    ) -> Result<Option<(UiElement, Node)>, UiError> {
        if runtime_id.is_empty() {
            return Ok(None);
        }
        // SAFETY: the condition's value is a VARIANT this function owns and clears once the
        // condition holds its own copy; the search runs on an element this object found.
        unsafe {
            let mut value =
                InitVariantFromInt32Array(runtime_id).map_err(|error| failed(&error))?;
            let condition = self.uia.CreatePropertyCondition(UIA_RuntimeIdPropertyId, &value);
            let _ = VariantClear(&raw mut value);
            let condition = condition.map_err(|error| failed(&error))?;
            match root.0.FindFirstBuildCache(TreeScope_Subtree, &condition, &self.own) {
                Ok(element) => {
                    let node = self.node(&element);
                    Ok(Some((UiElement(element), node)))
                }
                Err(error) if error.code().0 == EMPTY => Ok(None),
                Err(error) => Err(failed(&error)),
            }
        }
    }

    /// The element as it is now.
    pub fn refresh(&self, element: &UiElement) -> Result<Node, UiError> {
        // SAFETY: a call on an element this object found.
        let fresh =
            unsafe { element.0.BuildUpdatedCache(&self.own) }.map_err(|error| failed(&error))?;
        Ok(self.node(&fresh))
    }

    fn node(&self, element: &IUIAutomationElement) -> Node {
        // SAFETY: reads the properties cached on an element this object found; the value is
        // read only for an element with a value that is not a password field.
        unsafe {
            let mut node = node::cached(element);
            node.password = element.CurrentIsPassword().map(|flag| flag.as_bool()).unwrap_or(true);
            if node.patterns.value && !node.password {
                node.value = node::current_text(element, node::VALUE);
            }
            node
        }
    }
}

/// Elements of the control view that are on the screen.
unsafe fn on_screen(uia: &IUIAutomation2) -> windows::core::Result<IUIAutomationCondition> {
    // SAFETY: builds conditions through the UI Automation object; the VARIANT holds a plain
    // boolean and needs no clearing.
    unsafe {
        let shown =
            uia.CreatePropertyCondition(UIA_IsOffscreenPropertyId, &node::boolean(false))?;
        uia.CreateAndCondition(&uia.ControlViewCondition()?, &shown)
    }
}
