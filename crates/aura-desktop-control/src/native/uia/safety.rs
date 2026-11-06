use super::{Automation, UiElement, failed};
use crate::elements::{Node, UiError};
use aura_desktop_domain::{Role, safety::payment_label};
use windows::Win32::Foundation::POINT;

impl Automation {
    pub fn payment(&self, element: &UiElement, node: &Node) -> Result<bool, UiError> {
        if payment_label(&node.name, &node.automation_id) {
            return Ok(true);
        }
        // SAFETY: the walker reads names and IDs on UIA elements, never values.
        unsafe {
            let walker = self.uia.ControlViewWalker().map_err(|error| failed(&error))?;
            let mut current = element.0.clone();
            for _ in 0..16 {
                let parent = match walker.GetParentElementBuildCache(&current, &self.own) {
                    Ok(parent) => parent,
                    Err(error) if error.code().0 == super::EMPTY => return Ok(false),
                    Err(error) => return Err(failed(&error)),
                };
                let node = super::node::cached(&parent);
                if matches!(
                    Role::of_control_type(node.control_type),
                    Role::Group | Role::Pane | Role::Window
                ) && payment_label(&node.name, &node.automation_id)
                {
                    return Ok(true);
                }
                current = parent;
            }
            Err(UiError::Failed("目标容器过深，无法完成付款边界检查".into()))
        }
    }
    pub fn focused(&self) -> Result<(UiElement, Node), UiError> {
        // SAFETY: obtains the focused element with the same property cache used for reads.
        let element = unsafe { self.uia.GetFocusedElementBuildCache(&self.own) }
            .map_err(|error| failed(&error))?;
        let node = self.node(&element);
        Ok((UiElement(element), node))
    }

    pub fn at_point(&self, x: i32, y: i32) -> Result<(UiElement, Node), UiError> {
        // SAFETY: reads the element at physical screen coordinates, without moving input.
        let element = unsafe { self.uia.ElementFromPointBuildCache(POINT { x, y }, &self.own) }
            .map_err(|error| failed(&error))?;
        let node = self.node(&element);
        Ok((UiElement(element), node))
    }
}
