//! What a program's window shows through UI Automation, the interface screen readers use: its
//! elements, and acting on one of them without the pointer or the keyboard.
use crate::Rect;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What an element is, after its UI Automation control type.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Button,
    Calendar,
    CheckBox,
    ComboBox,
    Edit,
    Hyperlink,
    Image,
    ListItem,
    List,
    Menu,
    MenuBar,
    MenuItem,
    ProgressBar,
    RadioButton,
    ScrollBar,
    Slider,
    Spinner,
    StatusBar,
    Tab,
    TabItem,
    Text,
    ToolBar,
    ToolTip,
    Tree,
    TreeItem,
    Custom,
    Group,
    Thumb,
    DataGrid,
    DataItem,
    Document,
    SplitButton,
    Window,
    Pane,
    Header,
    HeaderItem,
    Table,
    TitleBar,
    Separator,
    SemanticZoom,
    AppBar,
    /// A control type this version does not know.
    Other,
}

/// UI Automation numbers its control types from `UIA_ButtonControlTypeId` in this order.
const FIRST_CONTROL_TYPE: i32 = 50_000;
const ROLES: [Role; 41] = [
    Role::Button,
    Role::Calendar,
    Role::CheckBox,
    Role::ComboBox,
    Role::Edit,
    Role::Hyperlink,
    Role::Image,
    Role::ListItem,
    Role::List,
    Role::Menu,
    Role::MenuBar,
    Role::MenuItem,
    Role::ProgressBar,
    Role::RadioButton,
    Role::ScrollBar,
    Role::Slider,
    Role::Spinner,
    Role::StatusBar,
    Role::Tab,
    Role::TabItem,
    Role::Text,
    Role::ToolBar,
    Role::ToolTip,
    Role::Tree,
    Role::TreeItem,
    Role::Custom,
    Role::Group,
    Role::Thumb,
    Role::DataGrid,
    Role::DataItem,
    Role::Document,
    Role::SplitButton,
    Role::Window,
    Role::Pane,
    Role::Header,
    Role::HeaderItem,
    Role::Table,
    Role::TitleBar,
    Role::Separator,
    Role::SemanticZoom,
    Role::AppBar,
];

impl Role {
    #[must_use]
    pub fn of_control_type(id: i32) -> Self {
        id.checked_sub(FIRST_CONTROL_TYPE)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| ROLES.get(index))
            .copied()
            .unwrap_or(Self::Other)
    }
}

/// Names longer than this are cut, in a read and in the key that finds the element again.
pub const MAX_NAME_CHARS: usize = 1_000;
/// Values longer than this are cut in a read.
pub const MAX_VALUE_CHARS: usize = 4_000;
/// The most elements one read lists, and the most levels it goes down.
pub const MAX_ELEMENTS: u32 = 2_000;
pub const MAX_DEPTH: u32 = 64;
/// The most steps one act takes, and the most scroll steps one step makes either way.
pub const MAX_ACT_STEPS: usize = 50;
pub const MAX_SCROLL: i32 = 50;

/// `text` cut to at most `max` characters.
#[must_use]
pub fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => text[..end].to_owned(),
        None => text.to_owned(),
    }
}

/// Which element an earlier read listed. The host finds it again by its runtime ID (UI
/// Automation's identity for an element while it exists) and takes it for the same element only
/// while its control type, automation ID and name are what they were.
#[derive(Clone, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElementKey {
    pub runtime_id: Vec<i32>,
    pub control_type: i32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub automation_id: String,
    /// At most [`MAX_NAME_CHARS`] characters of it.
    pub name: String,
}

/// What can be done to an element without the pointer or the keyboard.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Ability {
    /// It can be invoked: a button, a link, a menu item.
    Click,
    SetValue,
    /// It can be checked and unchecked.
    Toggle,
    ExpandCollapse,
    Select,
    /// It scrolls what it holds.
    Scroll,
    ScrollIntoView,
    Focus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Checked {
    On,
    Off,
    /// Neither: some of what it stands for is checked.
    Mixed,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// One element of a read. A read lists them in the order the window holds them, each after the
/// element it is in.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Element {
    pub key: ElementKey,
    pub role: Role,
    /// How many listed elements it is inside: 0 for the top ones of the read.
    pub depth: u32,
    pub rect: Rect,
    /// What it holds, such as an edit's text; never read for a password field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub focused: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub password: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<Checked>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abilities: Vec<Ability>,
    /// Not all of what is inside it was read (the read's limits or time): read again with it as
    /// the root.
    #[serde(default, skip_serializing_if = "is_false")]
    pub more: bool,
}

/// One step of acting on a window's elements.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActStep {
    /// Invokes it; one that cannot be invoked is toggled, selected, or expanded or collapsed,
    /// whichever it can.
    Click {
        element: ElementKey,
    },
    SetValue {
        element: ElementKey,
        value: String,
    },
    Check {
        element: ElementKey,
    },
    Uncheck {
        element: ElementKey,
    },
    Expand {
        element: ElementKey,
    },
    Collapse {
        element: ElementKey,
    },
    Select {
        element: ElementKey,
    },
    /// Gives it the keyboard focus, which brings its window to the front.
    Focus {
        element: ElementKey,
    },
    ScrollIntoView {
        element: ElementKey,
    },
    /// Scrolls what it holds: positive `down` scrolls down, positive `right` right, one unit a
    /// small step such as a line.
    Scroll {
        element: ElementKey,
        #[serde(default)]
        down: i32,
        #[serde(default)]
        right: i32,
    },
}

impl ActStep {
    #[must_use]
    pub fn element(&self) -> &ElementKey {
        match self {
            Self::Click { element }
            | Self::SetValue { element, .. }
            | Self::Check { element }
            | Self::Uncheck { element }
            | Self::Expand { element }
            | Self::Collapse { element }
            | Self::Select { element }
            | Self::Focus { element }
            | Self::ScrollIntoView { element }
            | Self::Scroll { element, .. } => element,
        }
    }
}
