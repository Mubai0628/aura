//! Reading a window's elements breadth first, so that a read its limits cut short still has the
//! outline of the whole window, then listing them in the order the window holds them.
use super::{Node, UiError, listing};
use aura_desktop_domain::{Element, Role};
use std::collections::VecDeque;
use std::time::Instant;

/// Where a read's nodes come from: UI Automation, or a made-up tree in the tests.
pub trait Tree {
    type Item;
    /// The node's children that are on the screen, in order.
    fn children(&mut self, of: &Self::Item) -> Result<Vec<(Self::Item, Node)>, UiError>;
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_elements: usize,
    /// Levels below the root.
    pub max_depth: usize,
    /// No node is opened after this; what was read by then is the answer.
    pub deadline: Instant,
}

#[derive(Debug, PartialEq)]
pub struct Walked {
    pub elements: Vec<Element>,
    pub complete: bool,
}

struct Visit<I> {
    item: I,
    node: Node,
    /// Child indexes from the root: in this order the elements are the window's order.
    path: Vec<u32>,
    /// The nearest listed element it is in.
    parent: Option<usize>,
    level: usize,
}

struct Listed {
    path: Vec<u32>,
    element: Element,
}

/// Reads the elements under `root`, whose name is `root_name`. Failing to open the root fails
/// the read. A node below it that fails to open is marked `more`; after a timeout, or once the
/// deadline passed, nodes already found are still listed but no more are opened.
pub fn walk<T: Tree>(
    tree: &mut T,
    root: &T::Item,
    root_name: &str,
    limits: &Limits,
) -> Result<Walked, UiError> {
    let mut listed: Vec<Listed> = Vec::new();
    let mut complete = true;
    let mut opening = true;
    let mut queue = VecDeque::new();
    push_children(&mut queue, tree.children(root)?, &[], None, 1);
    while let Some(visit) = queue.pop_front() {
        if left_out(&visit.node) {
            continue;
        }
        if listed.len() >= limits.max_elements {
            complete = false;
            mark(&mut listed, visit.parent);
            for rest in queue.drain(..) {
                if !left_out(&rest.node) {
                    mark(&mut listed, rest.parent);
                }
            }
            break;
        }
        let parent_name = visit.parent.map_or(root_name, |index| &listed[index].element.key.name);
        let at = if listing::listed(&visit.node, Some(parent_name)) {
            let depth = visit.parent.map_or(0, |index| listed[index].element.depth + 1);
            listed.push(Listed {
                path: visit.path.clone(),
                element: listing::element(&visit.node, depth),
            });
            Some(listed.len() - 1)
        } else {
            visit.parent
        };
        if !listing::expands(&visit.node) {
            continue;
        }
        if !opening || visit.level >= limits.max_depth || Instant::now() >= limits.deadline {
            complete = false;
            mark(&mut listed, at);
            continue;
        }
        match tree.children(&visit.item) {
            Ok(children) => push_children(&mut queue, children, &visit.path, at, visit.level + 1),
            Err(UiError::Gone) => {}
            Err(error) => {
                if error == UiError::Timeout {
                    opening = false;
                }
                complete = false;
                mark(&mut listed, at);
            }
        }
    }
    listed.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Walked { elements: listed.into_iter().map(|entry| entry.element).collect(), complete })
}

fn left_out(node: &Node) -> bool {
    node.offscreen || listing::skipped(Role::of_control_type(node.control_type))
}

fn mark(listed: &mut [Listed], at: Option<usize>) {
    if let Some(index) = at {
        listed[index].element.more = true;
    }
}

fn push_children<I>(
    queue: &mut VecDeque<Visit<I>>,
    children: Vec<(I, Node)>,
    path: &[u32],
    parent: Option<usize>,
    level: usize,
) {
    for (index, (item, node)) in children.into_iter().enumerate() {
        let mut child = path.to_vec();
        child.push(u32::try_from(index).unwrap_or(u32::MAX));
        queue.push_back(Visit { item, node, path: child, parent, level });
    }
}
