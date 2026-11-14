//! What a Shell script does that the permission mode decides apart from running it: deleting
//! files (`deletion`, asked every time unless every target is where the conversation may delete),
//! downloading (`download`, asked once per session) and writing outside the conversation's own
//! places (`places`, asked every time) — all below 完全访问, and all read from the script's text.
mod deletion;
mod download;
mod places;
mod script;

use std::path::{Path, PathBuf};

pub(crate) struct ShellRisks {
    pub deletes: bool,
    /// Every deletion names its targets in the text, and each lies where the conversation may
    /// delete without asking.
    pub deletes_where_allowed: bool,
    /// Every deletion is a deletion command whose targets lie where the conversation may delete,
    /// or are left to a variable or the pipeline while the command stands there.
    pub deletes_here: bool,
    pub downloads: bool,
    /// Places outside the conversation's own that the script writes or deletes, as it names them.
    pub outside: Vec<String>,
}

/// Where a script acts as its conversation's own.
#[derive(Clone, Debug, Default)]
pub(crate) struct Area {
    /// Where it writes without asking: the workspace; a sub-agent's temporary folder; a main
    /// agent's sub-agents' worktrees.
    pub roots: Vec<PathBuf>,
    /// Where it deletes without asking: a sub-agent's worktree and temporary folder. Empty for a
    /// conversation whose deletions all ask.
    pub deletable: Vec<PathBuf>,
    /// The folder TEMP and TMP name in its commands, when it has one of its own.
    pub temp: Option<PathBuf>,
}

#[cfg(test)]
impl Area {
    /// A conversation's workspace and nothing more.
    pub(crate) fn workspace(root: PathBuf) -> Self {
        Self { roots: vec![root], ..Self::default() }
    }
}

/// `directory` is where the script starts, when it is known.
pub(crate) fn of_script(script: &str, directory: Option<&Path>, area: &Area) -> ShellRisks {
    let (script, recycled) = deletion::recycling(script);
    let mut found = places::of(&script, directory, area);
    // A file sent to the Recycle Bin is changed where it is.
    for place in recycled {
        let resolved = script::resolve(&place, directory, area.temp.as_deref());
        if resolved
            .is_some_and(|resolved| !area.roots.iter().any(|root| script::within(&resolved, root)))
            && !found.outside.contains(&place)
        {
            found.outside.push(place);
        }
    }
    let deletes = deletion::deletes_files(&script);
    ShellRisks {
        deletes,
        deletes_where_allowed: deletes && found.deletes && found.deletions_allowed,
        deletes_here: deletes && found.deletes && found.deletions_here,
        downloads: download::downloads(&script),
        outside: found.outside,
    }
}

/// Whether `fresh`, the call read again, differs from the `frozen` call only by risks a sub-agent's
/// allowance since then lets its commands skip: the user's answer to this very call, typically.
/// The user approved the frozen call with them, and it is the frozen call that runs.
pub(crate) fn allowed_since(
    frozen: &aura_execution_contract::OperationContent,
    fresh: &aura_execution_contract::OperationContent,
) -> bool {
    use aura_security_contract::OperationRiskFlag as Risk;
    let skippable = [Risk::PermanentFileDeletion, Risk::OutsideWorkspaceWrite];
    let dropped: Vec<&Risk> =
        frozen.risks.iter().filter(|risk| !fresh.risks.contains(risk)).collect();
    let mut adjusted = fresh.clone();
    adjusted.risks.clone_from(&frozen.risks);
    fresh.risks.iter().all(|risk| frozen.risks.contains(risk))
        && !dropped.is_empty()
        && dropped.iter().all(|risk| skippable.contains(risk))
        && adjusted == *frozen
}

impl ShellRisks {
    /// Whether its deletions ask: one the user let the sub-agent make stays in its own folders.
    pub(crate) fn deletion_asks(&self, allowed: &aura_agent_team::Allowances) -> bool {
        self.deletes && !self.deletes_where_allowed && !(allowed.deletions && self.deletes_here)
    }

    /// Whether its writes outside ask: always while a deletion asks, which may be one of them.
    pub(crate) fn outside_asks(&self, allowed: &aura_agent_team::Allowances) -> bool {
        !self.outside.is_empty() && (!allowed.outside_writes || self.deletion_asks(allowed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visual_basic_move_to_the_recycle_bin_is_not_a_deletion() {
        let area = Area::workspace(PathBuf::from(r"C:\Dev\project"));
        let script = "Add-Type -AssemblyName Microsoft.VisualBasic\n\
            foreach ($file in $files) {\n\
              [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile(\n\
                $file,\n\
                'OnlyErrorDialogs',\n\
                'SendToRecycleBin'\n\
              )\n\
            }\n\
            [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('C:\\Old\\x', \
            [Microsoft.VisualBasic.FileIO.UIOption]::OnlyErrorDialogs, \
            [Microsoft.VisualBasic.FileIO.RecycleOption]::SendToRecycleBin)";
        let found = of_script(script, Some(Path::new(r"C:\Dev\project")), &area);
        assert!(!found.deletes, "nothing is deleted for good");
        assert_eq!(found.outside, vec![r"C:\Old\x".to_owned()], "a recycled place is changed");
        // Without the option VisualBasic deletes for good, and other deletions still count.
        for script in [
            "[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p)",
            "[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p, 'OnlyErrorDialogs', 'DeletePermanently')",
            "[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p, 'OnlyErrorDialogs', 'SendToRecycleBin'); Remove-Item x",
        ] {
            assert!(of_script(script, None, &area).deletes, "{script}");
        }
    }

    #[test]
    fn deletions_are_allowed_only_where_the_conversation_may_delete() {
        let copy = PathBuf::from(r"C:\Copies\project\k1");
        let sub_agent =
            Area { roots: vec![copy.clone()], deletable: vec![copy.clone()], temp: None };
        let found = of_script("Remove-Item old.txt", Some(&copy), &sub_agent);
        assert!(found.deletes && found.deletes_where_allowed);
        let found = of_script("Remove-Item $x", Some(&copy), &sub_agent);
        assert!(found.deletes && !found.deletes_where_allowed);
        let main = Area::workspace(copy.clone());
        let found = of_script("Remove-Item old.txt", Some(&copy), &main);
        assert!(found.deletes && !found.deletes_where_allowed);
        assert!(!of_script("Get-ChildItem", Some(&copy), &sub_agent).deletes_where_allowed);
    }

    #[test]
    fn what_the_user_lets_a_sub_agent_do_asks_no_more() {
        use aura_agent_team::Allowances;
        let copy = PathBuf::from(r"C:\Copies\project\k1");
        let area = Area { roots: vec![copy.clone()], deletable: vec![copy.clone()], temp: None };
        let read = |script: &str| of_script(script, Some(&copy), &area);
        let none = Allowances::default();
        let deletions = Allowances { deletions: true, ..none };
        let outside = Allowances { outside_writes: true, ..none };
        let both = Allowances { deletions: true, outside_writes: true, ..none };
        // A deletion left to a variable asks until the user lets it delete in its folders.
        let found = read("Remove-Item $png");
        assert!(found.deletion_asks(&none) && !found.deletion_asks(&deletions));
        // One named outside asks whatever the user allowed, and so does its write outside.
        let found = read(r"Remove-Item C:\Dev\project\a.txt");
        assert!(found.deletion_asks(&both) && found.outside_asks(&both));
        // A write outside asks until the user lets its commands write outside.
        let found = read(r"Set-Content C:\Temp\a.txt x");
        assert!(!found.deletion_asks(&none) && found.outside_asks(&none));
        assert!(!found.outside_asks(&outside));
        // A deletion that still asks keeps the command's write outside asking.
        let found = read(r"cd $dir; Remove-Item $old; Set-Content C:\Temp\a.txt x");
        assert!(found.deletion_asks(&both) && found.outside_asks(&both));
        // A main agent is allowed nothing: its deletions ask as they did.
        let main = of_script("Remove-Item $png", Some(&copy), &Area::workspace(copy.clone()));
        assert!(main.deletion_asks(&none));
    }
}
