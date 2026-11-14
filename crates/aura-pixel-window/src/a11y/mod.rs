//! What screen readers see: the window is a UI Automation fragment root whose children are the
//! page's parts in reading order, each with its name, role and state. Buttons and links can be
//! invoked and check boxes toggled; that goes through the window's own messages, as a click does.
//! Providers use COM threading, so UI Automation calls them on the window's thread.
// UI Automation's identifiers keep their Windows names where they are matched.
#![allow(non_upper_case_globals)]
mod element;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use windows::Win32::System::Variant::VARIANT;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Com::SAFEARRAY,
        UI::{
            Accessibility::{
                IRawElementProviderFragment, IRawElementProviderFragment_Impl,
                IRawElementProviderFragmentRoot, IRawElementProviderFragmentRoot_Impl,
                IRawElementProviderSimple, IRawElementProviderSimple_Impl, NavigateDirection,
                NavigateDirection_FirstChild, NavigateDirection_LastChild, ProviderOptions,
                ProviderOptions_ServerSideProvider, ProviderOptions_UseComThreading,
                ToggleState_Off, ToggleState_On, UIA_AutomationFocusChangedEventId,
                UIA_LiveRegionChangedEventId, UIA_PATTERN_ID, UIA_PROPERTY_ID,
                UIA_ToggleToggleStatePropertyId, UiaClientsAreListening, UiaDisconnectProvider,
                UiaHostProviderFromHwnd, UiaRaiseAutomationEvent,
                UiaRaiseAutomationPropertyChangedEvent, UiaRect, UiaReturnRawElementProvider,
            },
            WindowsAndMessaging::PostMessageW,
        },
    },
    core::{Error, IUnknown, Result, implement},
};

use windows_core::IUnknownImpl;

use crate::layout::{Node, Role};
pub(crate) use element::Element;

pub(crate) struct Shared {
    hwnd: HWND,
    nodes: RefCell<Vec<Node>>,
    /// The node of the focused control.
    focus: Cell<Option<usize>>,
    /// Counts the pages shown, so a provider of an earlier page reports itself gone.
    generation: Cell<u32>,
    origin: Cell<(i32, i32)>,
    k: Cell<i32>,
    root: RefCell<Option<IRawElementProviderSimple>>,
}

impl Shared {
    pub(crate) fn new(hwnd: HWND) -> Rc<Self> {
        Rc::new(Self {
            hwnd,
            nodes: RefCell::default(),
            focus: Cell::new(None),
            generation: Cell::new(0),
            origin: Cell::new((0, 0)),
            k: Cell::new(1),
            root: RefCell::new(None),
        })
    }

    pub(crate) fn update(
        &self,
        nodes: Vec<Node>,
        focus: Option<usize>,
        origin: (i32, i32),
        k: i32,
    ) {
        *self.nodes.borrow_mut() = nodes;
        self.focus.set(focus);
        self.origin.set(origin);
        self.k.set(k);
    }

    pub(crate) fn moved(&self, origin: (i32, i32)) {
        self.origin.set(origin);
    }

    pub(crate) fn next_generation(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    pub(crate) fn generation(&self) -> u32 {
        self.generation.get()
    }

    pub(crate) fn root(self: &Rc<Self>) -> IRawElementProviderSimple {
        self.root
            .borrow_mut()
            .get_or_insert_with(|| Root { shared: Rc::clone(self) }.into())
            .clone()
    }

    fn element(self: &Rc<Self>, node: usize) -> IRawElementProviderSimple {
        Element::new(Rc::clone(self), node).into()
    }

    /// Asks the window to choose or focus the control of `node`, as the user would.
    fn post(&self, message: u32, node: usize) -> Result<()> {
        let control =
            self.nodes.borrow().get(node).and_then(|node| node.control).ok_or_else(Error::empty)?;
        // SAFETY: posts to this provider's own window.
        unsafe {
            PostMessageW(
                Some(self.hwnd),
                message,
                WPARAM(self.generation() as usize),
                LPARAM(control as isize),
            )
        }
    }

    fn listening() -> bool {
        // SAFETY: a plain query.
        unsafe { UiaClientsAreListening() }.as_bool()
    }

    pub(crate) fn focus_changed(self: &Rc<Self>) {
        if let (true, Some(node)) = (Self::listening(), self.focus.get()) {
            // SAFETY: raises the event for a provider of this window.
            unsafe {
                let _ =
                    UiaRaiseAutomationEvent(&self.element(node), UIA_AutomationFocusChangedEventId);
            }
        }
    }

    pub(crate) fn toggled(self: &Rc<Self>, node: usize, checked: bool) {
        if !Self::listening() {
            return;
        }
        let state = |on: bool| VARIANT::from(if on { ToggleState_On.0 } else { ToggleState_Off.0 });
        // SAFETY: raises the event for a provider of this window.
        unsafe {
            let _ = UiaRaiseAutomationPropertyChangedEvent(
                &self.element(node),
                UIA_ToggleToggleStatePropertyId,
                &state(!checked),
                &state(checked),
            );
        }
    }

    /// A new page: its heading is read out, and the focus moves to its first control.
    pub(crate) fn page_changed(self: &Rc<Self>) {
        if !Self::listening() {
            return;
        }
        let heading = self.nodes.borrow().iter().position(|node| node.role == Role::Heading);
        if let Some(heading) = heading {
            // SAFETY: raises the event for a provider of this window.
            unsafe {
                let _ =
                    UiaRaiseAutomationEvent(&self.element(heading), UIA_LiveRegionChangedEventId);
            }
        }
        self.focus_changed();
    }

    /// The window is going: UI Automation lets go of its providers.
    pub(crate) fn disconnect(&self) {
        if let Some(root) = self.root.borrow_mut().take() {
            // SAFETY: tells UI Automation this window's providers are gone.
            unsafe {
                UiaReturnRawElementProvider(
                    self.hwnd,
                    WPARAM(0),
                    LPARAM(0),
                    None::<&IRawElementProviderSimple>,
                );
                let _ = UiaDisconnectProvider(&root);
            }
        }
    }

    /// `node`'s rectangle on the screen.
    fn screen_rect(&self, node: &Node) -> UiaRect {
        let (x, y) = self.origin.get();
        let rect = node.rect.scaled(self.k.get());
        UiaRect {
            left: f64::from(x + rect.x),
            top: f64::from(y + rect.y),
            width: f64::from(rect.width),
            height: f64::from(rect.height),
        }
    }

    /// The node at a point on the screen, preferring controls.
    fn node_at(&self, x: f64, y: f64) -> Option<usize> {
        let (left, top) = self.origin.get();
        let k = self.k.get();
        let (x, y) = (((x as i32) - left).div_euclid(k), ((y as i32) - top).div_euclid(k));
        let nodes = self.nodes.borrow();
        let inside = |node: &&Node| node.rect.contains(x, y);
        let control = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.control.is_some())
            .find(|(_, node)| inside(node));
        control
            .or_else(|| nodes.iter().enumerate().find(|(_, node)| inside(node)))
            .map(|(index, _)| index)
    }
}

#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot
)]
struct Root {
    shared: Rc<Shared>,
}

fn options() -> ProviderOptions {
    ProviderOptions(ProviderOptions_ServerSideProvider.0 | ProviderOptions_UseComThreading.0)
}

impl IRawElementProviderSimple_Impl for Root_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(options())
    }

    fn GetPatternProvider(&self, _pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        Err(Error::empty())
    }

    /// The window's own provider gives its name and role.
    fn GetPropertyValue(&self, _property: UIA_PROPERTY_ID) -> Result<VARIANT> {
        Ok(VARIANT::default())
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        // SAFETY: the window this provider belongs to.
        unsafe { UiaHostProviderFromHwnd(self.shared.hwnd) }
    }
}

impl IRawElementProviderFragment_Impl for Root_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let count = self.shared.nodes.borrow().len();
        let node = match direction {
            NavigateDirection_FirstChild if count > 0 => 0,
            NavigateDirection_LastChild if count > 0 => count - 1,
            _ => return Err(Error::empty()),
        };
        Ok(Element::new(Rc::clone(&self.shared), node).into())
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        Ok(UiaRect::default())
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(self.to_interface())
    }
}

impl IRawElementProviderFragmentRoot_Impl for Root_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let node = self.shared.node_at(x, y).ok_or_else(Error::empty)?;
        Ok(Element::new(Rc::clone(&self.shared), node).into())
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        let node = self.shared.focus.get().ok_or_else(Error::empty)?;
        Ok(Element::new(Rc::clone(&self.shared), node).into())
    }
}
