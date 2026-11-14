//! Tests with a time budget. Beside the parallel load of the other tests they measure that load
//! as much as the code: Core's PowerShell return time, 1.2 s alone on the development machine,
//! reached 1.5 s beside the rest of the Core tests. They run after every other test of the run has
//! ended, one at a time, with their budgets unchanged.
use super::artifacts::{Kind, Program};

pub(super) struct Alone {
    pub(super) package: &'static str,
    pub(super) kind: Kind,
    /// The test's full name, as `--exact` takes it.
    pub(super) test: &'static str,
}

pub(super) const ALONE: &[Alone] = &[
    Alone {
        package: "aura-core",
        kind: Kind::Lib,
        test: "agent_execution::tests::s08::performance::\
               s08_real_core_shell_return_times_include_authority_and_durable_results",
    },
    // A Git read stopped on its timeout or cancel ends within 5 s; beside the real terminals
    // of the same program it took longer.
    Alone {
        package: "aura-backend-windows-user",
        kind: Kind::Test,
        test: "git_detection::\
               single_process_read_timeout_and_cancel_are_confirmed_without_starting_children",
    },
];

/// The budgeted tests among the selected programs, with the program each one is in.
pub(super) fn in_selection<'a>(selected: &[&'a Program]) -> Vec<(&'a Program, &'static str)> {
    ALONE
        .iter()
        .filter_map(|alone| {
            selected
                .iter()
                .find(|program| program.package == alone.package && program.kind == alone.kind)
                .map(|program| (*program, alone.test))
        })
        .collect()
}

/// Leaves the budgeted tests out of the parallel run. A full name matches no other test.
pub(super) fn skip_arguments(alone: &[(&Program, &'static str)]) -> Vec<String> {
    alone.iter().flat_map(|(_, test)| ["--skip".to_owned(), (*test).to_owned()]).collect()
}

/// Runs one budgeted test by itself.
pub(super) fn run_arguments(test: &str) -> Vec<String> {
    [test, "--exact", "--test-threads=1"].map(str::to_owned).to_vec()
}
