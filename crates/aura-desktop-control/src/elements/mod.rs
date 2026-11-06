//! The rules of reading a window's elements and acting on them, apart from Windows: which
//! elements a read lists and how far it goes, and whether an element found again is still the
//! one an earlier read listed.
mod listing;
mod same;
mod steps;
mod walk;

pub use listing::{abilities, element, expands, key, listed, skipped};
pub use same::same;
pub use steps::{MAX_SET_VALUE_CHARS, MAX_TIMEOUT_MS, MIN_TIMEOUT_MS, check_read, check_steps};
pub use walk::{Limits, Tree, Walked, walk};

use aura_desktop_domain::{Checked, Rect};

/// Which of UI Automation's control patterns an element has.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Patterns {
    pub invoke: bool,
    pub value: bool,
    pub toggle: bool,
    pub expand_collapse: bool,
    pub selection_item: bool,
    pub scroll: bool,
    pub scroll_item: bool,
}

impl Patterns {
    #[must_use]
    pub fn any(&self) -> bool {
        self.invoke
            || self.value
            || self.toggle
            || self.expand_collapse
            || self.selection_item
            || self.scroll
            || self.scroll_item
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Expand {
    Collapsed,
    Expanded,
    PartiallyExpanded,
    /// Nothing to expand.
    Leaf,
}

/// An element as UI Automation reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub runtime_id: Vec<i32>,
    pub control_type: i32,
    pub name: String,
    pub automation_id: String,
    pub class_name: String,
    pub rect: Rect,
    pub enabled: bool,
    pub offscreen: bool,
    pub password: bool,
    pub focused: bool,
    pub focusable: bool,
    pub process_id: u32,
    pub patterns: Patterns,
    /// Read only for elements with a value that are not password fields.
    pub value: Option<String>,
    pub read_only: bool,
    pub toggle: Option<Checked>,
    pub expand: Option<Expand>,
    pub selected: Option<bool>,
}

/// Why a UI Automation call did not give an answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiError {
    /// The element is no longer there.
    Gone,
    /// The program did not answer in time.
    Timeout,
    NotEnabled,
    NotSupported,
    Failed(String),
}

impl std::fmt::Display for UiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gone => formatter.write_str("元素已经不在了"),
            Self::Timeout => formatter.write_str("程序无响应"),
            Self::NotEnabled => formatter.write_str("元素现在不可用"),
            Self::NotSupported => formatter.write_str("元素不支持这个操作"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}
