//! How many tests of a program run at once. The tests in `aura-desktop-control`'s test program
//! move the pointer, raise and close windows and take screenshots of the one desktop, so a test
//! running beside another would change what that one sees; they run one at a time. Every other
//! program runs `TEST_THREADS` tests at once.
use super::artifacts::{Kind, Program};
use crate::TEST_THREADS;

const ONE_AT_A_TIME: &[(&str, Kind)] = &[("aura-desktop-control", Kind::Test)];

/// `RUST_TEST_THREADS` for `program`; a `--test-threads` argument still overrides it.
pub(super) fn for_program(program: &Program) -> &'static str {
    let alone = ONE_AT_A_TIME
        .iter()
        .any(|(package, kind)| program.package == *package && program.kind == *kind);
    if alone { "1" } else { TEST_THREADS }
}
