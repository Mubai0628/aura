//! One part of the page for UI Automation: its properties, its place among its siblings, and
//! the patterns its role has (invoke, toggle, a read-only value or range).
// UI Automation's identifiers keep their Windows names where they are matched.
#![allow(non_upper_case_globals)]
use std::rc::Rc;

use windows_core::IUnknownImpl;

use windows::{
    Win32::{
        Foundation::E_ACCESSDENIED,
        System::{
            Com::SAFEARRAY,
            Ole::{SafeArrayCreateVector, SafeArrayPutElement},
            Variant::{VARIANT, VT_I4},
        },
        UI::Accessibility::{
            HeadingLevel1, IInvokeProvider, IInvokeProvider_Impl, IRangeValueProvider,
            IRangeValueProvider_Impl, IRawElementProviderFragment,
            IRawElementProviderFragment_Impl, IRawElementProviderFragmentRoot,
            IRawElementProviderSimple, IRawElementProviderSimple_Impl, IToggleProvider,
            IToggleProvider_Impl, IValueProvider, IValueProvider_Impl, NavigateDirection,
            NavigateDirection_NextSibling, NavigateDirection_Parent,
            NavigateDirection_PreviousSibling, Polite, ProviderOptions, ToggleState,
            ToggleState_Off, ToggleState_On, UIA_AutomationIdPropertyId, UIA_ButtonControlTypeId,
            UIA_CONTROLTYPE_ID, UIA_CheckBoxControlTypeId, UIA_ControlTypePropertyId,
            UIA_E_ELEMENTNOTAVAILABLE, UIA_EditControlTypeId, UIA_HasKeyboardFocusPropertyId,
            UIA_HeadingLevelPropertyId, UIA_HelpTextPropertyId, UIA_HyperlinkControlTypeId,
            UIA_InvokePatternId, UIA_IsContentElementPropertyId, UIA_IsControlElementPropertyId,
            UIA_IsEnabledPropertyId, UIA_IsKeyboardFocusablePropertyId, UIA_IsOffscreenPropertyId,
            UIA_ItemStatusPropertyId, UIA_LiveSettingPropertyId, UIA_NamePropertyId,
            UIA_PATTERN_ID, UIA_PROPERTY_ID, UIA_ProgressBarControlTypeId, UIA_RangeValuePatternId,
            UIA_TextControlTypeId, UIA_TogglePatternId, UIA_ValuePatternId, UiaAppendRuntimeId,
            UiaRect,
        },
    },
    core::{BOOL, BSTR, Error, HRESULT, IUnknown, Interface, PCWSTR, Result, implement},
};

use super::{Shared, options};
use crate::{
    layout::{Node, Role},
    window::{WM_A11Y_FOCUS, WM_A11Y_INVOKE},
};

#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IInvokeProvider,
    IToggleProvider,
    IValueProvider,
    IRangeValueProvider
)]
pub(crate) struct Element {
    shared: Rc<Shared>,
    node: usize,
    generation: u32,
}

fn gone() -> Error {
    Error::from_hresult(HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32))
}

impl Element {
    pub(crate) fn new(shared: Rc<Shared>, node: usize) -> Self {
        let generation = shared.generation();
        Self { shared, node, generation }
    }

    /// Reads this part, while the page it belongs to is still shown.
    fn with<T>(&self, read: impl FnOnce(&Node) -> T) -> Result<T> {
        if self.generation != self.shared.generation() {
            return Err(gone());
        }
        self.shared.nodes.borrow().get(self.node).map(read).ok_or_else(gone)
    }
}

fn control_type(role: Role) -> UIA_CONTROLTYPE_ID {
    match role {
        Role::Heading | Role::Text => UIA_TextControlTypeId,
        Role::Edit => UIA_EditControlTypeId,
        Role::Button => UIA_ButtonControlTypeId,
        Role::CheckBox => UIA_CheckBoxControlTypeId,
        Role::Hyperlink => UIA_HyperlinkControlTypeId,
        Role::ProgressBar => UIA_ProgressBarControlTypeId,
    }
}

fn text(value: &str) -> VARIANT {
    VARIANT::from(BSTR::from(value))
}

impl IRawElementProviderSimple_Impl for Element_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(options())
    }

    fn GetPatternProvider(&self, pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        let (role, range) = self.with(|node| (node.role, node.range.is_some()))?;
        match (pattern, role) {
            (UIA_InvokePatternId, Role::Button | Role::Hyperlink) => {
                self.to_interface::<IInvokeProvider>().cast()
            }
            (UIA_TogglePatternId, Role::CheckBox) => self.to_interface::<IToggleProvider>().cast(),
            (UIA_ValuePatternId, Role::Edit) => self.to_interface::<IValueProvider>().cast(),
            (UIA_RangeValuePatternId, Role::ProgressBar) if range => {
                self.to_interface::<IRangeValueProvider>().cast()
            }
            _ => Err(Error::empty()),
        }
    }

    fn GetPropertyValue(&self, property: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let focused = self.shared.focus.get() == Some(self.node);
        self.with(|node| {
            let focusable = node.enabled
                && matches!(node.role, Role::Button | Role::CheckBox | Role::Hyperlink)
                && node.automation_id != "close";
            match property {
                UIA_ControlTypePropertyId => VARIANT::from(control_type(node.role).0),
                UIA_NamePropertyId => text(&node.name),
                UIA_AutomationIdPropertyId => text(&node.automation_id),
                UIA_IsKeyboardFocusablePropertyId => VARIANT::from(focusable),
                UIA_HasKeyboardFocusPropertyId => VARIANT::from(focused),
                UIA_IsEnabledPropertyId => VARIANT::from(node.enabled),
                UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId => {
                    VARIANT::from(true)
                }
                UIA_IsOffscreenPropertyId => VARIANT::from(false),
                UIA_ItemStatusPropertyId => node.status.as_deref().map(text).unwrap_or_default(),
                UIA_HelpTextPropertyId => node.help.as_deref().map(text).unwrap_or_default(),
                UIA_HeadingLevelPropertyId if node.role == Role::Heading => {
                    VARIANT::from(HeadingLevel1.0)
                }
                UIA_LiveSettingPropertyId if node.role == Role::Heading => VARIANT::from(Polite.0),
                _ => VARIANT::default(),
            }
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        Err(Error::empty())
    }
}

impl IRawElementProviderFragment_Impl for Element_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        self.with(|_| ())?;
        let count = self.shared.nodes.borrow().len();
        match direction {
            NavigateDirection_Parent => self.shared.root().cast(),
            NavigateDirection_NextSibling if self.node + 1 < count => {
                Ok(Element::new(Rc::clone(&self.shared), self.node + 1).into())
            }
            NavigateDirection_PreviousSibling if self.node > 0 => {
                Ok(Element::new(Rc::clone(&self.shared), self.node - 1).into())
            }
            _ => Err(Error::empty()),
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        let parts = [UiaAppendRuntimeId as i32, self.generation as i32, self.node as i32];
        // SAFETY: a new array of three integers, filled in place and handed to the caller.
        unsafe {
            let array = SafeArrayCreateVector(VT_I4, 0, parts.len() as u32);
            if array.is_null() {
                return Err(Error::empty());
            }
            for (index, part) in parts.iter().enumerate() {
                SafeArrayPutElement(array, &(index as i32), (part as *const i32).cast())?;
            }
            Ok(array)
        }
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        let shared = Rc::clone(&self.shared);
        self.with(|node| shared.screen_rect(node))
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        self.with(|_| ())?;
        self.shared.post(WM_A11Y_FOCUS, self.node)
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        self.shared.root().cast()
    }
}

impl IInvokeProvider_Impl for Element_Impl {
    fn Invoke(&self) -> Result<()> {
        self.with(|_| ())?;
        self.shared.post(WM_A11Y_INVOKE, self.node)
    }
}

impl IToggleProvider_Impl for Element_Impl {
    fn Toggle(&self) -> Result<()> {
        self.with(|_| ())?;
        self.shared.post(WM_A11Y_INVOKE, self.node)
    }

    fn ToggleState(&self) -> Result<ToggleState> {
        self.with(|node| if node.toggled == Some(true) { ToggleState_On } else { ToggleState_Off })
    }
}

impl IValueProvider_Impl for Element_Impl {
    fn SetValue(&self, _value: &PCWSTR) -> Result<()> {
        Err(Error::from_hresult(E_ACCESSDENIED))
    }

    fn Value(&self) -> Result<BSTR> {
        self.with(|node| BSTR::from(node.value.as_deref().unwrap_or_default()))
    }

    fn IsReadOnly(&self) -> Result<BOOL> {
        Ok(true.into())
    }
}

impl IRangeValueProvider_Impl for Element_Impl {
    fn SetValue(&self, _value: f64) -> Result<()> {
        Err(Error::from_hresult(E_ACCESSDENIED))
    }

    fn Value(&self) -> Result<f64> {
        self.with(|node| node.range.unwrap_or(0.0))
    }

    fn IsReadOnly(&self) -> Result<BOOL> {
        Ok(true.into())
    }

    fn Maximum(&self) -> Result<f64> {
        Ok(100.0)
    }

    fn Minimum(&self) -> Result<f64> {
        Ok(0.0)
    }

    fn LargeChange(&self) -> Result<f64> {
        Ok(10.0)
    }

    fn SmallChange(&self) -> Result<f64> {
        Ok(1.0)
    }
}
