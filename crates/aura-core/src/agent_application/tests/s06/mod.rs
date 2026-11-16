//! Permission scenarios driven through production Core routes and real stores.
use super::*;
mod mode_changes;
mod pending_file_approval;
mod pending_shell_approval;
mod restart_after_move;

pub(super) mod non_recursive_reads;
