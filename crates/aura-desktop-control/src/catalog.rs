//! Which programs there are, and which program a window belongs to.
use aura_desktop_domain::{AppEntry, AppKind, ManualApp};
use std::collections::BTreeSet;

/// One item of the Start menu's All apps, as Windows describes it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StartMenuItem {
    /// Its parsing name: an AUMID, or a path that starts with a known folder's GUID.
    pub parsing_name: String,
    pub name: String,
    /// The file it opens, when Windows knows it: the path with its known folder filled in, or
    /// the target of its shortcut.
    pub target: Option<String>,
}

/// The catalog: the Start menu's programs, then the ones the user added. Documents, help files
/// and web addresses on the Start menu are not programs and are left out; each ID and each
/// program file appears once, the Start menu's first.
#[must_use]
pub fn catalog(items: Vec<StartMenuItem>, manual: &[ManualApp]) -> Vec<AppEntry> {
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut apps = Vec::new();
    for item in items {
        let id = item.parsing_name.trim().to_owned();
        let name = item.name.trim().to_owned();
        let Some(kind) = program_kind(&id, item.target.as_deref()) else { continue };
        if name.is_empty() || !ids.insert(id.to_lowercase()) {
            continue;
        }
        let path = item.target.filter(|target| is_program_file(target));
        if let Some(path) = &path {
            paths.insert(path.to_lowercase());
        }
        apps.push(AppEntry { id, name, kind, path });
    }
    for added in manual {
        let path = added.path.trim();
        if !is_program_file(path)
            || !std::path::Path::new(path).is_absolute()
            || !paths.insert(path.to_lowercase())
            || !ids.insert(path.to_lowercase())
        {
            continue;
        }
        let name = added.name.trim();
        let name = if name.is_empty() { file_name(path).unwrap_or(path) } else { name };
        apps.push(AppEntry {
            id: path.to_owned(),
            name: name.to_owned(),
            kind: AppKind::Manual,
            path: Some(path.to_owned()),
        });
    }
    apps
}

/// A packaged app is an AUMID with `!` and opens no file; any other item is a program when it
/// opens a program file, or when it is an AUMID with nothing Windows can show as a file.
fn program_kind(id: &str, target: Option<&str>) -> Option<AppKind> {
    if id.is_empty() || id.contains("://") {
        return None;
    }
    match target.map(str::trim).filter(|target| !target.is_empty()) {
        Some(target) => is_program_file(target).then_some(AppKind::Desktop),
        None if id.contains('!') && !looks_like_path(id) => Some(AppKind::Packaged),
        None => (!looks_like_path(id)).then_some(AppKind::Desktop),
    }
}

fn looks_like_path(id: &str) -> bool {
    id.contains('\\') || id.contains('/') || id.contains(':')
}

fn is_program_file(path: &str) -> bool {
    path.trim().to_ascii_lowercase().ends_with(".exe") && !path.contains("://")
}

/// The last part of a path.
#[must_use]
pub fn file_name(path: &str) -> Option<&str> {
    path.rsplit(['\\', '/']).next().filter(|name| !name.is_empty())
}

/// What a window says about the program that made it.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowFacts<'a> {
    /// The window's (or its process's) AUMID, when it has one.
    pub aumid: Option<&'a str>,
    pub process_path: Option<&'a str>,
    pub process_name: &'a str,
}

/// The program a window belongs to: the one with the window's AUMID, else the one whose program
/// file the window's process runs, else the only one whose program file has the process's name.
#[must_use]
pub fn owner<'a>(apps: &'a [AppEntry], facts: &WindowFacts<'_>) -> Option<&'a AppEntry> {
    if let Some(aumid) = facts.aumid.map(str::trim).filter(|aumid| !aumid.is_empty())
        && let Some(app) = apps.iter().find(|app| app.id.eq_ignore_ascii_case(aumid))
    {
        return Some(app);
    }
    if let Some(process) = facts.process_path
        && let Some(app) = apps
            .iter()
            .find(|app| app.path.as_deref().is_some_and(|path| path.eq_ignore_ascii_case(process)))
    {
        return Some(app);
    }
    let named = |app: &&AppEntry| {
        app.path.as_deref().and_then(file_name).is_some_and(|name| {
            !facts.process_name.is_empty() && name.eq_ignore_ascii_case(facts.process_name)
        })
    };
    let mut found = apps.iter().filter(named);
    match (found.next(), found.next()) {
        (Some(app), None) => Some(app),
        _ => None,
    }
}
