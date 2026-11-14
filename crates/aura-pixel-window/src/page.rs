//! What a window shows: a frame (the setup program's or the launcher's), a heading, blocks, and a
//! footer of buttons. The window keeps the user's changes (checked boxes) in the page it was given.

/// Names a control, so the caller knows which one the user chose.
pub type Id = &'static str;

#[derive(Clone, Debug)]
pub struct Page {
    /// The title bar's words, and the window's name for Windows and screen readers.
    pub title: String,
    pub frame: Frame,
    /// The title bar has a close button; Esc and Alt+F4 then close the page.
    pub closable: bool,
    pub heading: String,
    pub blocks: Vec<Block>,
    pub footer: Footer,
}

#[derive(Clone, Debug)]
pub enum Frame {
    /// The setup program: 760 px wide, its column holding the large mark, the version and the
    /// steps of the whole setup above the sea.
    Setup { version: String, marks: Vec<Mark> },
    /// The launcher: 480 px wide, at least, with a narrow column.
    Launcher { icon: Icon, fish: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    Logo,
    Warning,
}

/// One of the setup's steps in the column.
#[derive(Clone, Debug)]
pub struct Mark {
    pub label: String,
    pub state: State,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Done,
    Now,
    Todo,
}

#[derive(Clone, Debug)]
pub enum Block {
    Field(Field),
    Check(Check),
    Meter(Meter),
    Steps(Vec<Step>),
    Pairs(Vec<Pair>),
    Text(Line),
}

/// A location the window shows and does not edit, with an optional button beside it.
#[derive(Clone, Debug)]
pub struct Field {
    /// Shown above the field; screen readers read it as the field's name.
    pub label: String,
    pub show_label: bool,
    pub value: String,
    /// The user changed it: drawn with a dark edge.
    pub changed: bool,
    pub button: Option<Button>,
    /// A line below the field, each part in its face.
    pub detail: Vec<(String, Face)>,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: Id,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
    /// Ticking it ends the page with [`Action::Toggled`], so the caller can change the page.
    pub notify: bool,
}

/// A progress bar with a line of words, above or below it.
#[derive(Clone, Debug)]
pub struct Meter {
    /// Done out of one; `None` when the work cannot say, and the bar moves without a position.
    pub fraction: Option<f64>,
    pub left: String,
    pub right: String,
    pub words_above: bool,
}

#[derive(Clone, Debug)]
pub struct Step {
    pub label: String,
    pub state: State,
    pub value: Option<String>,
}

/// A row of a table of locations: a short key, a path and a size.
#[derive(Clone, Debug)]
pub struct Pair {
    pub key: String,
    pub value: String,
    pub extra: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub text: String,
    pub tone: Tone,
    pub face: Face,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tone {
    Ink,
    Muted,
    Danger,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Face {
    /// Fusion Pixel 12, at 12 or 24 px.
    Pixel,
    /// The system's reading face, for names and paths.
    Reading,
}

#[derive(Clone, Debug, Default)]
pub struct Footer {
    pub link: Option<Link>,
    /// Buttons at the left edge, before the space.
    pub start: Vec<Button>,
    /// Buttons at the right edge; the primary one is usually last.
    pub end: Vec<Button>,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub id: Id,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Button {
    pub id: Id,
    pub label: String,
    pub kind: Kind,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Normal,
    Primary,
    Danger,
}

/// What ended a page: a button, a link, a check box that reports its changes, or closing the
/// window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Button(Id),
    Link(Id),
    Toggled(Id),
    Close,
}

impl Check {
    pub fn new(id: Id, label: impl Into<String>, checked: bool) -> Self {
        Self { id, label: label.into(), checked, enabled: true, notify: false }
    }
}

impl Button {
    pub fn new(id: Id, label: impl Into<String>, kind: Kind) -> Self {
        Self { id, label: label.into(), kind, enabled: true }
    }
}

impl Line {
    pub fn new(text: impl Into<String>, tone: Tone, face: Face) -> Self {
        Self { text: text.into(), tone, face }
    }
}

impl Page {
    pub fn checked(&self, id: Id) -> bool {
        self.blocks
            .iter()
            .any(|block| matches!(block, Block::Check(check) if check.id == id && check.checked))
    }

    pub fn set_checked(&mut self, id: Id, checked: bool) {
        for block in &mut self.blocks {
            if let Block::Check(check) = block
                && check.id == id
            {
                check.checked = checked;
            }
        }
    }

    pub fn meter_mut(&mut self) -> Option<&mut Meter> {
        self.blocks.iter_mut().find_map(|block| match block {
            Block::Meter(meter) => Some(meter),
            _ => None,
        })
    }

    pub fn steps_mut(&mut self) -> Option<&mut Vec<Step>> {
        self.blocks.iter_mut().find_map(|block| match block {
            Block::Steps(steps) => Some(steps),
            _ => None,
        })
    }

    pub fn field_mut(&mut self, label: &str) -> Option<&mut Field> {
        self.blocks.iter_mut().find_map(|block| match block {
            Block::Field(field) if field.label == label => Some(field),
            _ => None,
        })
    }

    /// The buttons in the footer, in the order they are drawn.
    pub fn buttons(&self) -> impl Iterator<Item = &Button> {
        self.footer.start.iter().chain(&self.footer.end)
    }

    pub fn button_mut(&mut self, id: Id) -> Option<&mut Button> {
        let fields = self.blocks.iter_mut().filter_map(|block| match block {
            Block::Field(field) => field.button.as_mut(),
            _ => None,
        });
        fields
            .chain(self.footer.start.iter_mut().chain(&mut self.footer.end))
            .find(|button| button.id == id)
    }

    /// Whether anything on the page moves by itself: a spinner or a bar without a position.
    pub fn animated(&self) -> bool {
        self.blocks.iter().any(|block| match block {
            Block::Meter(meter) => meter.fraction.is_none(),
            Block::Steps(steps) => steps.iter().any(|step| step.state == State::Now),
            _ => false,
        })
    }
}
