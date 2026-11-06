//! The windows of the Aura that started the host belong to no program: the host never shows,
//! reads or acts on them, whatever the user allowed, so a model cannot answer Aura's own
//! approvals or change its settings through them.
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

/// One process of a snapshot of the running processes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Process {
    pub parent: u32,
    /// Its program file's name, such as aura-core.exe.
    pub name: String,
}

static AURA: OnceLock<BTreeSet<u32>> = OnceLock::new();

/// The processes that started `own`: its parent (Core) and the parent's parent (Aura's window),
/// unless that one is Explorer, which also draws the desktop and File Explorer's windows.
#[must_use]
pub fn starters(own: u32, processes: &HashMap<u32, Process>) -> BTreeSet<u32> {
    let parent =
        |pid: u32| processes.get(&pid).map(|process| process.parent).filter(|parent| *parent != 0);
    let mut found = BTreeSet::new();
    let Some(core) = parent(own) else {
        return found;
    };
    found.insert(core);
    if let Some(main) = parent(core).filter(|main| *main != own)
        && processes.get(&main).is_some_and(|main| !main.name.eq_ignore_ascii_case("explorer.exe"))
    {
        found.insert(main);
    }
    found
}

/// Kept for the host's life: the processes cannot end while it runs, since they keep it.
pub fn remember(processes: BTreeSet<u32>) {
    let _ = AURA.set(processes);
}

#[must_use]
pub fn is_aura(process: u32) -> bool {
    AURA.get().is_some_and(|aura| aura.contains(&process))
}
