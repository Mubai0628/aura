//! Acting on an element through its control patterns, in the background: the pointer does not
//! move and no key is pressed.
use super::{Automation, UiElement, failed, node};
use crate::elements::UiError;
use aura_desktop_domain::Checked;
use windows::Win32::UI::Accessibility::{
    IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern, IUIAutomationScrollItemPattern,
    IUIAutomationScrollPattern, IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern,
    IUIAutomationValuePattern, ScrollAmount, ScrollAmount_NoAmount, ScrollAmount_SmallDecrement,
    ScrollAmount_SmallIncrement, UIA_ExpandCollapsePatternId, UIA_InvokePatternId, UIA_PATTERN_ID,
    UIA_ScrollItemPatternId, UIA_ScrollPatternId, UIA_SelectionItemPatternId, UIA_TogglePatternId,
    UIA_ValuePatternId,
};
use windows::core::{BSTR, Interface};

/// The element's pattern; an element without it answers `NotSupported`.
fn pattern<T: Interface>(element: &UiElement, id: UIA_PATTERN_ID) -> Result<T, UiError> {
    // SAFETY: asks an element that an `Automation` found for one of its patterns.
    unsafe { element.0.GetCurrentPatternAs::<T>(id) }.map_err(|error| failed(&error))
}

fn done(result: windows::core::Result<()>) -> Result<(), UiError> {
    result.map_err(|error| failed(&error))
}

fn amount(units: i32) -> ScrollAmount {
    if units > 0 { ScrollAmount_SmallIncrement } else { ScrollAmount_SmallDecrement }
}

impl Automation {
    pub fn invoke(&self, element: &UiElement) -> Result<(), UiError> {
        let invoke = pattern::<IUIAutomationInvokePattern>(element, UIA_InvokePatternId)?;
        // SAFETY: a call on the pattern just obtained.
        done(unsafe { invoke.Invoke() })
    }

    pub fn set_value(&self, element: &UiElement, value: &str) -> Result<(), UiError> {
        let node = self.refresh(element)?;
        if node.password {
            return Err(UiError::Failed("密码要用户自己输入，这一步没有执行".into()));
        }
        let pattern = pattern::<IUIAutomationValuePattern>(element, UIA_ValuePatternId)?;
        // SAFETY: a call on the pattern just obtained, with a string that outlives it.
        done(unsafe { pattern.SetValue(&BSTR::from(value)) })
    }

    pub fn toggle(&self, element: &UiElement) -> Result<(), UiError> {
        let toggle = pattern::<IUIAutomationTogglePattern>(element, UIA_TogglePatternId)?;
        // SAFETY: a call on the pattern just obtained.
        done(unsafe { toggle.Toggle() })
    }

    pub fn checked(&self, element: &UiElement) -> Result<Option<Checked>, UiError> {
        let toggle = pattern::<IUIAutomationTogglePattern>(element, UIA_TogglePatternId)?;
        // SAFETY: a query of the pattern just obtained.
        let state = unsafe { toggle.CurrentToggleState() }.map_err(|error| failed(&error))?;
        Ok(node::checked(state.0))
    }

    pub fn expand(&self, element: &UiElement) -> Result<(), UiError> {
        let pattern =
            pattern::<IUIAutomationExpandCollapsePattern>(element, UIA_ExpandCollapsePatternId)?;
        // SAFETY: a call on the pattern just obtained.
        done(unsafe { pattern.Expand() })
    }

    pub fn collapse(&self, element: &UiElement) -> Result<(), UiError> {
        let pattern =
            pattern::<IUIAutomationExpandCollapsePattern>(element, UIA_ExpandCollapsePatternId)?;
        // SAFETY: a call on the pattern just obtained.
        done(unsafe { pattern.Collapse() })
    }

    pub fn select(&self, element: &UiElement) -> Result<(), UiError> {
        let pattern =
            pattern::<IUIAutomationSelectionItemPattern>(element, UIA_SelectionItemPatternId)?;
        // SAFETY: a call on the pattern just obtained.
        done(unsafe { pattern.Select() })
    }

    /// Gives the element the keyboard focus, which brings its window to the front.
    pub fn focus(&self, element: &UiElement) -> Result<(), UiError> {
        // SAFETY: a call on an element that an `Automation` found.
        done(unsafe { element.0.SetFocus() })
    }

    pub fn scroll_into_view(&self, element: &UiElement) -> Result<(), UiError> {
        let pattern = pattern::<IUIAutomationScrollItemPattern>(element, UIA_ScrollItemPatternId)?;
        // SAFETY: a call on the pattern just obtained.
        done(unsafe { pattern.ScrollIntoView() })
    }

    /// Scrolls `down` small steps down (up when negative) and `right` small steps right.
    pub fn scroll(&self, element: &UiElement, down: i32, right: i32) -> Result<(), UiError> {
        let scroll = pattern::<IUIAutomationScrollPattern>(element, UIA_ScrollPatternId)?;
        for _ in 0..down.unsigned_abs() {
            // SAFETY: a call on the pattern just obtained.
            done(unsafe { scroll.Scroll(ScrollAmount_NoAmount, amount(down)) })?;
        }
        for _ in 0..right.unsigned_abs() {
            // SAFETY: a call on the pattern just obtained.
            done(unsafe { scroll.Scroll(amount(right), ScrollAmount_NoAmount) })?;
        }
        Ok(())
    }
}
