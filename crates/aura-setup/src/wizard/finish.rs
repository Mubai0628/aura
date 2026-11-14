//! The last page: what the user should know, the old data older versions and other installations
//! left, which goes only when the user removes it, and starting Aura.
use std::path::Path;

use anyhow::Result;
use aura_launcher::read_install_state;
use aura_old_data::{Installed, OldData, Removed, Scope};
use aura_pixel_window::{Action, PixelWindow};
use time::UtcOffset;

use super::{Outside, pages};
use crate::{host::Host, install::Kind};

/// Shows the finished page for the installation in `root` until the user is done with it. Old
/// data that cannot be looked for is not listed; the page itself always comes.
pub(super) fn finish(
    window: &mut PixelWindow,
    host: &(dyn Host + Sync),
    outside: &dyn Outside,
    (kind, version): (&Kind, &str),
    mut notes: Vec<String>,
    root: &Path,
) -> Result<()> {
    let roots = host.roots().ok();
    let namespace =
        read_install_state(root).ok().map(|state| state.release_descriptor.bundle_namespace);
    let installed = Installed::from_entries(host.installed_folders());
    let scope = roots.as_ref().zip(namespace.as_ref()).map(|(roots, current)| Scope {
        roots,
        current,
        installed: &installed,
    });
    let find = || scope.as_ref().map(aura_old_data::find).unwrap_or_default();
    let mut old = find();
    let mut ticked = vec![true; old.len()];
    let mut start = true;
    // Days as the clock on the taskbar shows them, the way 设置 → 存储 shows the same items.
    let offset = aura_windows_platform::local_utc_offset_seconds()
        .and_then(|seconds| UtcOffset::from_whole_seconds(seconds).ok())
        .unwrap_or(UtcOffset::UTC);
    loop {
        let mut page = pages::finished(kind, version, &notes, &old, &ticked, start, offset);
        let action = window.show(&mut page)?;
        start = page.checked("start");
        ticked = pages::ticked(&page, old.len());
        match action {
            Action::Toggled(_) => {}
            Action::Button("remove-old") => {
                let Some(scope) = &scope else { continue };
                let ids: Vec<String> = old
                    .iter()
                    .zip(&ticked)
                    .filter(|(_, ticked)| **ticked)
                    .map(|(item, _)| item.id.clone())
                    .collect();
                let removed = window.run_work(
                    &pages::removing_old(kind, version),
                    |_| {},
                    || aura_old_data::remove(scope, &ids),
                );
                notes.extend(removal_notes(&removed, &old));
                old = find();
                // What is still listed was left unticked or could not go: the user picks again.
                ticked = vec![false; old.len()];
            }
            Action::Button("done") => {
                if start {
                    outside.start(root);
                }
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
}

/// One line for what went, then one for each item that is in use or did not finish going.
pub(crate) fn removal_notes(removed: &Removed, old: &[OldData]) -> Vec<String> {
    let name = |id: &str| {
        old.iter().find(|item| item.id == id).map_or_else(|| id.to_owned(), pages::old_data_name)
    };
    let mut notes = Vec::new();
    if !removed.removed.is_empty() {
        let names: Vec<String> = removed.removed.iter().map(|id| name(id)).collect();
        notes.push(format!("已删除：{}", names.join("、")));
    }
    notes.extend(removed.in_use.iter().map(|id| format!("{} 正在使用，没有删", name(id))));
    notes
        .extend(removed.failed.iter().map(|(id, reason)| format!("{} 没删完：{reason}", name(id))));
    notes
}
