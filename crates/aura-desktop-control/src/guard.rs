//! Who has the desktop while Aura keeps it, independent of Windows hooks: a mouse button pressed,
//! the wheel turned, Escape or the stop keys of the user's take it over. Moving the pointer,
//! another key, or a key or button let go does not: a hand resting on the mouse or the keyboard
//! is no takeover. Neither is a click or the wheel on Aura's own window: the user is writing to
//! Aura or reading its conversation. Nothing gives the desktop back; the hold ends instead.
use aura_desktop_domain::Chord;
use aura_desktop_domain::input::{VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT};

/// Whether `key` (a virtual-key code) pressed while `held` says which modifiers are down is
/// `chord`: its key with exactly its modifiers among Control, Alt, Shift and Windows.
#[must_use]
pub fn is_chord(chord: &Chord, key: u32, held: impl Fn(u16) -> bool) -> bool {
    let wants = |code: u16| chord.modifiers.iter().any(|modifier| modifier.code == code);
    u32::from(chord.key.code) == key
        && [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN]
            .into_iter()
            .all(|code| held(code) == wants(code))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gate {
    Ready,
    TakenOver,
}

/// What took the desktop over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause {
    Button,
    Wheel,
    Escape,
    StopKeys,
    /// Stopped without an input: the watch on the user's input ended.
    Stopped,
}

/// The first input that took the desktop over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Takeover {
    pub cause: Cause,
    /// Windows marked the input as sent by a program rather than a mouse or a keyboard.
    pub injected: bool,
}

impl Takeover {
    /// The cause as the model and the conversation read it.
    #[must_use]
    pub fn words(self) -> String {
        let cause = match self.cause {
            Cause::Button => "点了鼠标",
            Cause::Wheel => "滚了滚轮",
            Cause::Escape => "按了 Esc",
            Cause::StopKeys => "按了停止快捷键",
            Cause::Stopped => "看护输入的线程已停止",
        };
        if self.injected { format!("{cause}，来自另一个程序") } else { cause.to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    /// Aura's own input.
    Aura,
    /// A mouse button pressed; `injected`: sent by a program.
    Button {
        injected: bool,
    },
    Wheel {
        injected: bool,
    },
    Escape {
        injected: bool,
    },
    /// The stop keys' last key pressed while their other keys are held.
    StopKeys {
        injected: bool,
    },
    /// The pointer moved, another key, or a key or button let go.
    Ignored,
}

/// A mouse event of the user's, as the watch on the pointer tells them apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pointer {
    /// A button pressed.
    Pressed,
    Wheel,
    /// The pointer moved, or a button let go.
    Other,
}

/// What a mouse event of the user's is to the kept desktop; `on_aura`: over a window of the Aura
/// that keeps it, which takes nothing over.
#[must_use]
pub fn pointer(event: Pointer, injected: bool, on_aura: bool) -> Input {
    match event {
        _ if on_aura => Input::Ignored,
        Pointer::Pressed => Input::Button { injected },
        Pointer::Wheel => Input::Wheel { injected },
        Pointer::Other => Input::Ignored,
    }
}

#[derive(Default)]
pub struct State {
    taken: Option<Takeover>,
}

impl State {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether this key must be kept from the program: Escape meant for Aura while the
    /// program's window is in front, or the stop keys, which are Aura's alone.
    pub fn observe(&mut self, input: Input, target_in_front: bool) -> bool {
        let (cause, injected, keep) = match input {
            Input::Aura | Input::Ignored => return false,
            Input::Button { injected } => (Cause::Button, injected, false),
            Input::Wheel { injected } => (Cause::Wheel, injected, false),
            Input::Escape { injected } => (Cause::Escape, injected, target_in_front),
            Input::StopKeys { injected } => (Cause::StopKeys, injected, true),
        };
        self.taken.get_or_insert(Takeover { cause, injected });
        keep
    }

    pub fn take_over(&mut self) {
        self.taken.get_or_insert(Takeover { cause: Cause::Stopped, injected: false });
    }

    #[must_use]
    pub fn gate(&self) -> Gate {
        if self.taken.is_some() { Gate::TakenOver } else { Gate::Ready }
    }

    /// What took the desktop over first, once it was.
    #[must_use]
    pub fn takeover(&self) -> Option<Takeover> {
        self.taken
    }
}
