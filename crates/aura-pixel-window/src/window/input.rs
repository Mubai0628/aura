//! What the pointer and keys do: which control is under the pointer, moving the focus, and
//! choosing a control.
use super::State;
use crate::{
    layout::Target,
    page::{Action, Block},
};

impl State {
    /// The control at `x`, `y` in the window's own pixels.
    pub(crate) fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (x.div_euclid(self.k), y.div_euclid(self.k));
        self.laid.controls.iter().position(|control| control.rect.contains(x, y))
    }

    /// Moves the focus to the next control that takes it, or the one before, wrapping round.
    pub(crate) fn move_focus(&mut self, forward: bool) {
        let stops: Vec<usize> = (0..self.laid.controls.len())
            .filter(|index| self.laid.controls[*index].tab_stop)
            .collect();
        if stops.is_empty() {
            return;
        }
        let at = self.look.focus.and_then(|focus| stops.iter().position(|stop| *stop == focus));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => stops.len() - 1,
            (Some(at), true) => (at + 1) % stops.len(),
            (Some(at), false) => (at + stops.len() - 1) % stops.len(),
        };
        self.look.focus = Some(stops[next]);
        self.look.cues = true;
        self.paint();
        self.a11y.focus_changed();
    }

    pub(crate) fn focus(&mut self, index: usize) {
        if self.laid.controls.get(index).is_some_and(|control| control.tab_stop)
            && self.look.focus != Some(index)
        {
            self.look.focus = Some(index);
            self.paint();
            self.a11y.focus_changed();
        }
    }

    /// Chooses control `index`: a button or link ends the page, a check box flips.
    pub(crate) fn activate(&mut self, index: usize) {
        let Some((target, enabled, node)) = self
            .laid
            .controls
            .get(index)
            .map(|control| (control.target, control.enabled, control.node))
        else {
            return;
        };
        if !enabled {
            return;
        }
        match target {
            Target::Button(id) => self.outcome = Some(Action::Button(id)),
            Target::Link(id) => self.outcome = Some(Action::Link(id)),
            Target::Close => self.close(),
            Target::Check(id) => {
                let checked = !self.page.checked(id);
                self.page.set_checked(id, checked);
                self.paint();
                self.a11y.toggled(node, checked);
                let notify = self.page.blocks.iter().any(
                    |block| matches!(block, Block::Check(check) if check.id == id && check.notify),
                );
                if notify {
                    self.outcome = Some(Action::Toggled(id));
                }
            }
        }
    }

    /// Enter: the focused button or link, otherwise the primary button.
    pub(crate) fn enter(&mut self) {
        let focused = self.look.focus.filter(|index| {
            matches!(
                self.laid.controls.get(*index).map(|control| control.target),
                Some(Target::Button(_) | Target::Link(_))
            )
        });
        let primary =
            || self.laid.controls.iter().position(|control| control.primary && control.enabled);
        if let Some(index) = focused.or_else(primary) {
            self.activate(index);
        }
    }

    /// Esc, Alt+F4 and the close button: only a page that can be closed ends.
    pub(crate) fn close(&mut self) {
        if self.page.closable {
            self.outcome = Some(Action::Close);
        }
    }
}
