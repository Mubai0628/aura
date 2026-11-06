//! Whether an element found again by its runtime ID is still the one a read listed.
use super::{Node, listing};
use aura_desktop_domain::ElementKey;

/// The same element: the same runtime ID, control type, automation ID and name. An element that
/// was replaced, or whose name changed, is not taken for the listed one: guessing could act on
/// the wrong thing.
#[must_use]
pub fn same(listed: &ElementKey, found: &Node) -> bool {
    listing::key(found) == *listed
}
