//! Other builds on this machine. `cargo xtask` itself runs under a `cargo run`, often behind
//! rustup's `cargo.exe` proxy, so the cargo processes that started this one do not count.
use std::{
    collections::{HashMap, HashSet},
    process::Command,
};

use anyhow::{Context, Result, ensure};

/// One running process: its id, its parent's id, when it started (a Windows file time; 0 when
/// Windows does not say) and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Running {
    id: u32,
    parent: u32,
    started: u64,
    name: String,
}

/// `name (pid)` of every cargo or rustc process other than the ones that started xtask.
pub(super) fn other_builds() -> Result<Vec<String>> {
    let script = "Get-CimInstance Win32_Process | ForEach-Object { \
         $started = if ($_.CreationDate) { $_.CreationDate.ToFileTimeUtc() } else { 0 }; \
         '{0} {1} {2} {3}' -f $_.ProcessId, $_.ParentProcessId, $started, $_.Name }";
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .context("failed to start Windows PowerShell for the process list")?;
    ensure!(output.status.success(), "the process list failed with {}", output.status);
    let processes = parse(&String::from_utf8_lossy(&output.stdout));
    ensure!(!processes.is_empty(), "the process list was empty");
    Ok(others(&processes, std::process::id()))
}

/// The lines `id parent started name` the process list prints.
fn parse(text: &str) -> Vec<Running> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.trim().splitn(4, ' ');
            Some(Running {
                id: fields.next()?.parse().ok()?,
                parent: fields.next()?.parse().ok()?,
                started: fields.next()?.parse().ok()?,
                name: fields.next()?.to_owned(),
            })
        })
        .collect()
}

/// `name (id)` of every cargo or rustc in `processes` that is not an ancestor of `me`. A parent's
/// id can belong to a later process once the parent has ended; a real parent started before its
/// child.
fn others(processes: &[Running], me: u32) -> Vec<String> {
    let by_id: HashMap<u32, &Running> =
        processes.iter().map(|process| (process.id, process)).collect();
    let mut ancestors = HashSet::new();
    let mut current = by_id.get(&me).copied();
    while let Some(child) = current {
        current = by_id.get(&child.parent).copied().filter(|parent| {
            parent.id != child.id && parent.started <= child.started && ancestors.insert(parent.id)
        });
    }
    processes
        .iter()
        .filter(|process| {
            let name = process.name.to_ascii_lowercase();
            (name == "cargo.exe" || name == "rustc.exe") && !ancestors.contains(&process.id)
        })
        .map(|process| format!("{} ({})", process.name, process.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{others, parse};

    #[test]
    fn the_cargo_processes_that_started_xtask_do_not_count_and_every_other_build_does() {
        let processes = parse(
            "5 1 100 pwsh.exe\n\
             10 5 200 cargo.exe\n\
             11 10 210 cargo.exe\n\
             12 11 220 xtask.exe\n\
             20 5 300 cargo.exe\n\
             21 20 310 rustc.exe\n\
             0 0 0 System Idle Process\n\
             not a process line\n",
        );
        assert_eq!(processes.len(), 7);
        assert_eq!(processes[6].name, "System Idle Process");
        assert_eq!(others(&processes, 12), ["cargo.exe (20)", "rustc.exe (21)"]);
        assert_eq!(
            others(&processes, 5),
            ["cargo.exe (10)", "cargo.exe (11)", "cargo.exe (20)", "rustc.exe (21)"]
        );
    }

    #[test]
    fn a_later_process_with_an_ended_parents_id_is_not_taken_for_the_parent() {
        // xtask's parent 7 ended and its id went to a cargo that started after xtask.
        let processes = parse("7 5 900 cargo.exe\n12 7 220 xtask.exe\n");
        assert_eq!(others(&processes, 12), ["cargo.exe (7)"]);
    }
}
