//! The properties a read caches on each element, and reading them back into a [`Node`].
use crate::elements::{Expand, Node, Patterns};
use aura_desktop_domain::{Checked, Rect};
use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Variant::{
    VARENUM, VARIANT, VT_ARRAY, VT_BOOL, VT_BSTR, VT_I4, VariantClear, VariantToBoolean,
    VariantToInt32, VariantToInt32ArrayAlloc, VariantToStringAlloc,
};
use windows::Win32::UI::Accessibility::{
    AutomationElementMode_Full, IUIAutomation2, IUIAutomationCacheRequest, IUIAutomationCondition,
    IUIAutomationElement, TreeScope, UIA_AutomationIdPropertyId, UIA_BoundingRectanglePropertyId,
    UIA_ClassNamePropertyId, UIA_ControlTypePropertyId,
    UIA_ExpandCollapseExpandCollapseStatePropertyId, UIA_HasKeyboardFocusPropertyId,
    UIA_IsEnabledPropertyId, UIA_IsExpandCollapsePatternAvailablePropertyId,
    UIA_IsInvokePatternAvailablePropertyId, UIA_IsKeyboardFocusablePropertyId,
    UIA_IsOffscreenPropertyId, UIA_IsPasswordPropertyId,
    UIA_IsScrollItemPatternAvailablePropertyId, UIA_IsScrollPatternAvailablePropertyId,
    UIA_IsSelectionItemPatternAvailablePropertyId, UIA_IsTogglePatternAvailablePropertyId,
    UIA_IsValuePatternAvailablePropertyId, UIA_NamePropertyId, UIA_PROPERTY_ID,
    UIA_ProcessIdPropertyId, UIA_RuntimeIdPropertyId, UIA_SelectionItemIsSelectedPropertyId,
    UIA_ToggleToggleStatePropertyId, UIA_ValueIsReadOnlyPropertyId, UIA_ValueValuePropertyId,
};

pub(super) const VALUE: UIA_PROPERTY_ID = UIA_ValueValuePropertyId;

/// What a read caches on every element. The value is not among them: it is read on its own,
/// and only for elements that are not password fields.
const PROPERTIES: [UIA_PROPERTY_ID; 23] = [
    UIA_RuntimeIdPropertyId,
    UIA_ControlTypePropertyId,
    UIA_NamePropertyId,
    UIA_AutomationIdPropertyId,
    UIA_ClassNamePropertyId,
    UIA_BoundingRectanglePropertyId,
    UIA_IsEnabledPropertyId,
    UIA_IsOffscreenPropertyId,
    UIA_IsPasswordPropertyId,
    UIA_HasKeyboardFocusPropertyId,
    UIA_IsKeyboardFocusablePropertyId,
    UIA_ProcessIdPropertyId,
    UIA_IsInvokePatternAvailablePropertyId,
    UIA_IsValuePatternAvailablePropertyId,
    UIA_IsTogglePatternAvailablePropertyId,
    UIA_IsExpandCollapsePatternAvailablePropertyId,
    UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsScrollPatternAvailablePropertyId,
    UIA_IsScrollItemPatternAvailablePropertyId,
    UIA_ValueIsReadOnlyPropertyId,
    UIA_ToggleToggleStatePropertyId,
    UIA_ExpandCollapseExpandCollapseStatePropertyId,
    UIA_SelectionItemIsSelectedPropertyId,
];

pub(super) unsafe fn request(
    uia: &IUIAutomation2,
    filter: &IUIAutomationCondition,
    scope: TreeScope,
) -> windows::core::Result<IUIAutomationCacheRequest> {
    // SAFETY: builds a cache request through the UI Automation object's own methods.
    unsafe {
        let request = uia.CreateCacheRequest()?;
        for property in PROPERTIES {
            request.AddProperty(property)?;
        }
        request.SetTreeScope(scope)?;
        request.SetTreeFilter(filter)?;
        request.SetAutomationElementMode(AutomationElementMode_Full)?;
        Ok(request)
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
    fn kind(&self) -> VARENUM {
        // SAFETY: every VARIANT starts with its type tag.
        unsafe { self.0.Anonymous.Anonymous.vt }
    }

    fn flag(&self) -> bool {
        // SAFETY: reads a VARIANT whose tag says it is a boolean.
        self.kind() == VT_BOOL
            && unsafe { VariantToBoolean(&raw const self.0) }.is_ok_and(|value| value.as_bool())
    }

    fn number(&self) -> Option<i32> {
        // SAFETY: reads a VARIANT whose tag says it is a 32-bit integer.
        (self.kind() == VT_I4).then(|| unsafe { VariantToInt32(&raw const self.0) }.ok()).flatten()
    }

    fn numbers(&self) -> Vec<i32> {
        if self.kind().0 != VT_ARRAY.0 | VT_I4.0 {
            return Vec::new();
        }
        let mut items: *mut i32 = std::ptr::null_mut();
        let mut count = 0_u32;
        // SAFETY: the array is copied out of the buffer the call allocated, within the count it
        // gave, and the buffer is freed with the allocator it came from.
        unsafe {
            if VariantToInt32ArrayAlloc(&raw const self.0, &raw mut items, &raw mut count).is_err()
                || items.is_null()
            {
                return Vec::new();
            }
            let found = std::slice::from_raw_parts(items, count as usize).to_vec();
            CoTaskMemFree(Some(items as *const _));
            found
        }
    }
}

/// A VARIANT holding a boolean, which needs no clearing.
pub(super) fn boolean(value: bool) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: writes the type tag of a zeroed VARIANT and the member the tag names.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_BOOL;
        inner.Anonymous.boolVal = if value { VARIANT_TRUE } else { VARIANT_FALSE };
    }
    variant
}

pub(super) fn checked(state: i32) -> Option<Checked> {
    match state {
        0 => Some(Checked::Off),
        1 => Some(Checked::On),
        2 => Some(Checked::Mixed),
        _ => None,
    }
}

fn expand(state: i32) -> Option<Expand> {
    match state {
        0 => Some(Expand::Collapsed),
        1 => Some(Expand::Expanded),
        2 => Some(Expand::PartiallyExpanded),
        3 => Some(Expand::Leaf),
        _ => None,
    }
}

/// The element's properties as the cache request read them; what an element does not report
/// reads as empty or false.
pub(super) unsafe fn cached(element: &IUIAutomationElement) -> Node {
    // SAFETY: reads properties cached on an element; each VARIANT is cleared when dropped.
    unsafe {
        let property = |id: UIA_PROPERTY_ID| element.GetCachedPropertyValue(id).ok().map(Owned);
        let flag = |id: UIA_PROPERTY_ID| property(id).is_some_and(|value| value.flag());
        let number = |id: UIA_PROPERTY_ID| property(id).and_then(|value| value.number());
        let patterns = Patterns {
            invoke: flag(UIA_IsInvokePatternAvailablePropertyId),
            value: flag(UIA_IsValuePatternAvailablePropertyId),
            toggle: flag(UIA_IsTogglePatternAvailablePropertyId),
            expand_collapse: flag(UIA_IsExpandCollapsePatternAvailablePropertyId),
            selection_item: flag(UIA_IsSelectionItemPatternAvailablePropertyId),
            scroll: flag(UIA_IsScrollPatternAvailablePropertyId),
            scroll_item: flag(UIA_IsScrollItemPatternAvailablePropertyId),
        };
        let text = |read: windows::core::Result<windows::core::BSTR>| {
            read.map(|text| text.to_string()).unwrap_or_default()
        };
        let rect = element.CachedBoundingRectangle().map_or(Rect::default(), |rect| Rect {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        });
        Node {
            runtime_id: property(UIA_RuntimeIdPropertyId)
                .map(|value| value.numbers())
                .unwrap_or_default(),
            control_type: element.CachedControlType().map_or(0, |kind| kind.0),
            name: text(element.CachedName()),
            automation_id: text(element.CachedAutomationId()),
            class_name: text(element.CachedClassName()),
            rect,
            enabled: flag(UIA_IsEnabledPropertyId),
            offscreen: flag(UIA_IsOffscreenPropertyId),
            password: flag(UIA_IsPasswordPropertyId),
            focused: flag(UIA_HasKeyboardFocusPropertyId),
            focusable: flag(UIA_IsKeyboardFocusablePropertyId),
            process_id: number(UIA_ProcessIdPropertyId)
                .and_then(|id| u32::try_from(id).ok())
                .unwrap_or(0),
            patterns,
            value: None,
            read_only: patterns.value && flag(UIA_ValueIsReadOnlyPropertyId),
            toggle: if patterns.toggle {
                number(UIA_ToggleToggleStatePropertyId).and_then(checked)
            } else {
                None
            },
            expand: if patterns.expand_collapse {
                number(UIA_ExpandCollapseExpandCollapseStatePropertyId).and_then(expand)
            } else {
                None
            },
            selected: patterns.selection_item.then(|| flag(UIA_SelectionItemIsSelectedPropertyId)),
        }
    }
}

/// A text property read from the program now, not from the cache.
pub(super) unsafe fn current_text(
    element: &IUIAutomationElement,
    id: UIA_PROPERTY_ID,
) -> Option<String> {
    // SAFETY: reads one property; the VARIANT is cleared when dropped and the copy of its text
    // is freed by `take_com_string`.
    unsafe {
        let value = Owned(element.GetCurrentPropertyValue(id).ok()?);
        if value.kind() != VT_BSTR {
            return None;
        }
        let text = VariantToStringAlloc(&raw const value.0).ok()?;
        Some(super::super::text::take_com_string(text))
    }
}
