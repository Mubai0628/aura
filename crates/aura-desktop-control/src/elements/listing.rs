//! Which elements a read lists and opens, and what it says of each.
use super::{Expand, Node};
use aura_desktop_domain::{
    Ability, Element, ElementKey, MAX_NAME_CHARS, MAX_VALUE_CHARS, Role, clip,
};

/// Kinds a read leaves out with all they hold: scroll bars, thumbs and separators are drawn
/// parts of other controls, tooltips come and go, and a title bar holds only the window's own
/// buttons.
#[must_use]
pub fn skipped(role: Role) -> bool {
    matches!(role, Role::ScrollBar | Role::Thumb | Role::Separator | Role::ToolTip | Role::TitleBar)
}

/// Kinds listed whatever their name: what the user works with, and the containers that order it.
const ALWAYS: [Role; 25] = [
    Role::Button,
    Role::CheckBox,
    Role::ComboBox,
    Role::Edit,
    Role::Hyperlink,
    Role::ListItem,
    Role::MenuItem,
    Role::RadioButton,
    Role::Slider,
    Role::Spinner,
    Role::SplitButton,
    Role::TabItem,
    Role::TreeItem,
    Role::DataItem,
    Role::HeaderItem,
    Role::Document,
    Role::List,
    Role::Tree,
    Role::Table,
    Role::DataGrid,
    Role::Tab,
    Role::Menu,
    Role::MenuBar,
    Role::ToolBar,
    Role::Calendar,
];

/// Whether a read lists the node: what can be used, and the named parts that tell what is on
/// the window (text, groups, panes, images); not what only lays out others. Text that repeats
/// the name of the element it is in (`parent_name`, such as a button's own caption) is left out.
#[must_use]
pub fn listed(node: &Node, parent_name: Option<&str>) -> bool {
    let role = Role::of_control_type(node.control_type);
    if skipped(role) {
        return false;
    }
    if ALWAYS.contains(&role) || node.patterns.any() {
        return true;
    }
    let name = node.name.trim();
    !name.is_empty()
        && !(role == Role::Text && parent_name.is_some_and(|parent| parent.trim() == name))
}

/// Whether a read looks inside the node. Controls whose parts are only their own drawing are not
/// opened; a combo box, menu item or tree item holds its choices only while it is expanded (one
/// that cannot expand holds none).
#[must_use]
pub fn expands(node: &Node) -> bool {
    match Role::of_control_type(node.control_type) {
        Role::Button
        | Role::CheckBox
        | Role::RadioButton
        | Role::Hyperlink
        | Role::Edit
        | Role::Text
        | Role::Image
        | Role::Slider
        | Role::Spinner
        | Role::ProgressBar
        | Role::SplitButton
        | Role::HeaderItem => false,
        Role::ComboBox | Role::MenuItem | Role::TreeItem => {
            matches!(node.expand, Some(Expand::Expanded | Expand::PartiallyExpanded))
        }
        role => !skipped(role),
    }
}

#[must_use]
pub fn abilities(node: &Node) -> Vec<Ability> {
    let patterns = node.patterns;
    [
        (patterns.invoke, Ability::Click),
        (patterns.value && !node.read_only && !node.password, Ability::SetValue),
        (patterns.toggle, Ability::Toggle),
        (
            patterns.expand_collapse && !matches!(node.expand, Some(Expand::Leaf)),
            Ability::ExpandCollapse,
        ),
        (patterns.selection_item, Ability::Select),
        (patterns.scroll, Ability::Scroll),
        (patterns.scroll_item, Ability::ScrollIntoView),
        (node.focusable, Ability::Focus),
    ]
    .into_iter()
    .filter_map(|(has, ability)| has.then_some(ability))
    .collect()
}

#[must_use]
pub fn key(node: &Node) -> ElementKey {
    ElementKey {
        runtime_id: node.runtime_id.clone(),
        control_type: node.control_type,
        automation_id: node.automation_id.clone(),
        name: clip(&node.name, MAX_NAME_CHARS),
    }
}

/// What a read says of a node it lists, `depth` listed elements down.
#[must_use]
pub fn element(node: &Node, depth: u32) -> Element {
    Element {
        key: key(node),
        role: Role::of_control_type(node.control_type),
        depth,
        rect: node.rect,
        value: node
            .value
            .as_deref()
            .filter(|_| !node.password)
            .map(|value| clip(value, MAX_VALUE_CHARS)),
        enabled: node.enabled,
        focused: node.focused,
        password: node.password,
        checked: node.toggle,
        expanded: match node.expand {
            Some(Expand::Expanded | Expand::PartiallyExpanded) => Some(true),
            Some(Expand::Collapsed) => Some(false),
            Some(Expand::Leaf) | None => None,
        },
        selected: node.selected,
        abilities: abilities(node),
        more: false,
    }
}
