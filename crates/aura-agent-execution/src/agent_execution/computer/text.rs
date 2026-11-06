//! The elements of a read and of an act, in the compact text the model reads: one element a line,
//! indented under the element it is in.
use aura_desktop_domain::{Checked, Element};

fn name_of<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

/// One element: `e12 button "保存" disabled [click]`.
#[must_use]
pub fn line(reference: &str, element: &Element) -> String {
    let mut line = format!("{reference} {} {}", name_of(element.role), quoted(&element.key.name));
    if let Some(value) = &element.value {
        line.push_str(&format!(" value={}", quoted(value)));
    }
    let mut states = Vec::new();
    if !element.enabled {
        states.push("disabled");
    }
    if element.focused {
        states.push("focused");
    }
    if element.password {
        states.push("password");
    }
    match element.checked {
        Some(Checked::On) => states.push("checked"),
        Some(Checked::Off) => states.push("unchecked"),
        Some(Checked::Mixed) => states.push("mixed"),
        None => {}
    }
    match element.expanded {
        Some(true) => states.push("expanded"),
        Some(false) => states.push("collapsed"),
        None => {}
    }
    if element.selected == Some(true) {
        states.push("selected");
    }
    if element.more {
        states.push("more");
    }
    for state in states {
        line.push(' ');
        line.push_str(state);
    }
    if !element.abilities.is_empty() {
        let abilities: Vec<String> =
            element.abilities.iter().map(|ability| name_of(*ability)).collect();
        line.push_str(&format!(" [{}]", abilities.join(", ")));
    }
    line
}

/// A read's elements, each indented two spaces a level under the element it is in.
#[must_use]
pub fn elements(elements: &[Element], refs: &[String]) -> String {
    elements
        .iter()
        .zip(refs)
        .map(|(element, reference)| {
            let depth = usize::try_from(element.depth).unwrap_or(0).min(64);
            format!("{}{}", "  ".repeat(depth), line(reference, element))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each step's element after an act: `1. e12 button "保存" [click]`, or that it is gone.
#[must_use]
pub fn after(refs: &[String], after: &[Option<Element>]) -> String {
    after
        .iter()
        .enumerate()
        .map(|(index, element)| {
            let reference = refs.get(index).map_or("?", String::as_str);
            match element {
                Some(element) => format!("{}. {}", index + 1, line(reference, element)),
                None => {
                    format!("{}. {reference} 已经不在了（例如这一步关掉了它的窗口）", index + 1)
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
