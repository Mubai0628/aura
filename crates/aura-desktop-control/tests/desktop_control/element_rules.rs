//! What a read says of one element, whether an element found again is the one that was read,
//! and which reads and acts are refused before anything starts.
use aura_desktop_control::elements::{
    Expand, MAX_SET_VALUE_CHARS, Node, Patterns, abilities, check_read, check_steps, element, same,
};
use aura_desktop_domain::*;
use windows::Win32::UI::Accessibility::{
    UIA_AppBarControlTypeId, UIA_ButtonControlTypeId, UIA_CONTROLTYPE_ID, UIA_EditControlTypeId,
    UIA_ListControlTypeId, UIA_ListItemControlTypeId, UIA_PaneControlTypeId, UIA_TextControlTypeId,
    UIA_WindowControlTypeId,
};

fn node(control_type: i32, name: &str) -> Node {
    Node {
        runtime_id: vec![42, 7],
        control_type,
        name: name.into(),
        enabled: true,
        ..Node::default()
    }
}

#[test]
fn roles_follow_the_systems_numbering_of_control_types() {
    let known: [(UIA_CONTROLTYPE_ID, Role); 8] = [
        (UIA_ButtonControlTypeId, Role::Button),
        (UIA_EditControlTypeId, Role::Edit),
        (UIA_ListItemControlTypeId, Role::ListItem),
        (UIA_ListControlTypeId, Role::List),
        (UIA_TextControlTypeId, Role::Text),
        (UIA_WindowControlTypeId, Role::Window),
        (UIA_PaneControlTypeId, Role::Pane),
        (UIA_AppBarControlTypeId, Role::AppBar),
    ];
    for (id, role) in known {
        assert_eq!(Role::of_control_type(id.0), role, "{}", id.0);
    }
}

#[test]
fn abilities_follow_the_patterns_an_element_has() {
    let mut field = node(UIA_EditControlTypeId.0, "名字");
    field.patterns = Patterns { value: true, ..Patterns::default() };
    field.focusable = true;
    assert_eq!(abilities(&field), [Ability::SetValue, Ability::Focus]);
    field.read_only = true;
    assert_eq!(abilities(&field), [Ability::Focus]);

    let mut item = node(UIA_ListItemControlTypeId.0, "苹果");
    item.patterns = Patterns {
        invoke: true,
        toggle: true,
        expand_collapse: true,
        selection_item: true,
        scroll: true,
        scroll_item: true,
        ..Patterns::default()
    };
    item.expand = Some(Expand::Leaf);
    assert_eq!(
        abilities(&item),
        [
            Ability::Click,
            Ability::Toggle,
            Ability::Select,
            Ability::Scroll,
            Ability::ScrollIntoView
        ]
    );
    item.expand = Some(Expand::Collapsed);
    assert!(abilities(&item).contains(&Ability::ExpandCollapse));
}

#[test]
fn a_read_says_what_state_an_element_is_in() {
    let mut combo = node(50_003, "大小");
    combo.patterns = Patterns { expand_collapse: true, value: true, ..Patterns::default() };
    combo.value = Some("中".repeat(MAX_VALUE_CHARS + 10));
    for (state, expanded) in [
        (Some(Expand::Collapsed), Some(false)),
        (Some(Expand::Expanded), Some(true)),
        (Some(Expand::PartiallyExpanded), Some(true)),
        (Some(Expand::Leaf), None),
        (None, None),
    ] {
        combo.expand = state;
        assert_eq!(element(&combo, 2).expanded, expanded, "{state:?}");
    }
    let read = element(&combo, 2);
    assert_eq!(read.depth, 2);
    assert_eq!(read.role, Role::ComboBox);
    assert_eq!(read.value.as_deref().map(|value| value.chars().count()), Some(MAX_VALUE_CHARS));

    let mut long = node(UIA_TextControlTypeId.0, &"长".repeat(MAX_NAME_CHARS + 5));
    long.automation_id = "status".into();
    let read = element(&long, 0);
    assert_eq!(read.key.name.chars().count(), MAX_NAME_CHARS);
    assert_eq!(read.key.automation_id, "status");
    assert_eq!(read.key.runtime_id, [42, 7]);
}

#[test]
fn an_element_found_again_is_the_one_read_only_while_nothing_that_names_it_changed() {
    let button = node(UIA_ButtonControlTypeId.0, "确定");
    let key = element(&button, 0).key;
    assert!(same(&key, &button));
    let mut moved = button.clone();
    moved.rect = Rect { x: 500, y: 500, width: 10, height: 10 };
    moved.enabled = false;
    assert!(same(&key, &moved), "where it is and its state do not name it");
    for changed in [
        Node { name: "取消".into(), ..button.clone() },
        Node { control_type: UIA_TextControlTypeId.0, ..button.clone() },
        Node { automation_id: "cancel".into(), ..button.clone() },
        Node { runtime_id: vec![42, 8], ..button.clone() },
    ] {
        assert!(!same(&key, &changed), "{changed:?}");
    }
    let long = node(UIA_TextControlTypeId.0, &"长".repeat(MAX_NAME_CHARS + 5));
    assert!(same(&element(&long, 0).key, &long), "a long name is compared as it was cut");
}

#[test]
fn reads_and_acts_outside_the_limits_are_refused_before_anything_starts() {
    assert!(check_read(300, 32, 8_000).is_ok());
    for (elements, depth, timeout) in [
        (0, 32, 8_000),
        (MAX_ELEMENTS + 1, 32, 8_000),
        (300, 0, 8_000),
        (300, MAX_DEPTH + 1, 8_000),
        (300, 32, 100),
        (300, 32, 60_001),
    ] {
        assert!(check_read(elements, depth, timeout).is_err(), "{elements} {depth} {timeout}");
    }

    let key = element(&node(UIA_ListControlTypeId.0, "水果"), 0).key;
    let scroll = |down, right| ActStep::Scroll { element: key.clone(), down, right };
    assert!(check_steps(&[scroll(3, 0), scroll(0, -MAX_SCROLL)], 8_000).is_ok());
    assert!(check_steps(&[], 8_000).is_err());
    assert!(check_steps(&vec![scroll(1, 0); MAX_ACT_STEPS + 1], 8_000).is_err());
    assert!(check_steps(&[scroll(0, 0)], 8_000).is_err());
    assert!(check_steps(&[scroll(MAX_SCROLL + 1, 0)], 8_000).is_err());
    assert!(check_steps(&[scroll(i32::MIN, 0)], 8_000).is_err());
    assert!(check_steps(&[scroll(1, 0)], 50).is_err());
    let nowhere = ElementKey { runtime_id: Vec::new(), ..key.clone() };
    assert!(check_steps(&[ActStep::Click { element: nowhere }], 8_000).is_err());
    let text =
        |length: usize| ActStep::SetValue { element: key.clone(), value: "字".repeat(length) };
    assert!(check_steps(&[text(MAX_SET_VALUE_CHARS)], 8_000).is_ok());
    assert!(check_steps(&[text(MAX_SET_VALUE_CHARS + 1)], 8_000).is_err());
}
