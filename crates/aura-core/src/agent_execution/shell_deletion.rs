//! Whether a Shell
//! script deletes files. Every way PowerShell and CMD delete is permanent — nothing goes to the
//! Recycle Bin — so such a script is asked every time (`OperationRiskFlag::PermanentFileDeletion`),
//! and the model is told to delete with `fs.delete`, which recycles. A script is text, not a plan:
//! this reads the commands a model writes to delete, and errs toward asking. A word that only looks
//! like one (`git commit -m "fix rd"`) asks once more than needed; it never asks less.
use aura_windows_user_contract::ManagedShell;
use regex::Regex;
use std::sync::OnceLock;

/// A command that deletes, where a command starts: the script's start, a new line, `;`, `|`, `&`,
/// `(` or `{`, an optional call operator, and the word followed by a space or the end.
const COMMAND: &str = r"(?im)(?:^|[;|&({\r\n])\s*(?:&\s*)?(?:remove-item|ri|rm|rmdir|rd|del|erase|clear-recyclebin|remove-itemproperty\s+-path\s+\S*::filesystem)(?:\.exe)?(?:\s|$|[;|&)}])";
/// .NET and COM deletions a PowerShell script can make.
const DOTNET: &str = r"(?i)\[(?:system\.)?io\.(?:file|directory)\]::delete\s*\(|\]::delete(?:file|directory)\s*\(|\.delete\s*\(\s*(?:\$true|\$false)?\s*\)";
/// `git clean` with a force flag deletes untracked files; robocopy's /MIR and /PURGE delete what
/// the source lacks; `cmd /c del` from PowerShell is still a deletion.
const TOOLS: &str = r"(?i)\bgit(?:\.exe)?\s+clean\b[^\r\n;|&]*\s-(?:-force|[a-z]*f)|\brobocopy(?:\.exe)?\b[^\r\n;|&]*\s/(?:mir|purge)\b|\bcmd(?:\.exe)?\s+/[ck]\s+[^\r\n]*\b(?:del|erase|rd|rmdir)\b";

fn patterns() -> &'static [Regex; 3] {
    static PATTERNS: OnceLock<[Regex; 3]> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [COMMAND, DOTNET, TOOLS]
            .map(|pattern| Regex::new(pattern).expect("the deletion patterns are valid"))
    })
}

/// Whether the script deletes files, for either Shell. CMD's own commands are among PowerShell's
/// aliases, and a CMD script can start PowerShell, so both are read the same way.
pub(crate) fn deletes_files(_shell: ManagedShell, script: &str) -> bool {
    patterns().iter().any(|pattern| pattern.is_match(script))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELLS: [ManagedShell; 2] = [ManagedShell::PowerShell, ManagedShell::Cmd];

    #[test]
    fn the_ways_a_model_deletes_files_are_seen() {
        for script in [
            "Remove-Item C:\\Users\\me\\Desktop\\old.txt",
            "Get-ChildItem *.tmp | Remove-Item -Force",
            "Get-ChildItem *.log | ri",
            "rm -r -fo .\\dist",
            "cd build; rmdir /s /q out",
            "rd /s /q node_modules",
            "del /q *.bak",
            "erase notes.txt",
            "if (Test-Path x) { Remove-Item x -Recurse }",
            "& rm x",
            "Clear-RecycleBin -Force",
            "[System.IO.File]::Delete('C:\\a.txt')",
            "[IO.Directory]::Delete($dir, $true)",
            "(Get-Item a.txt).Delete()",
            "[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p)",
            "git clean -fdx",
            "git clean --force -d",
            "robocopy C:\\src C:\\dst /MIR",
            "cmd /c del /q C:\\temp\\*",
            "REMOVE-ITEM x",
        ] {
            for shell in SHELLS {
                assert!(deletes_files(shell, script), "{script}");
            }
        }
    }

    #[test]
    fn ordinary_scripts_are_not_asked_again() {
        for script in [
            "Get-ChildItem -Recurse | Select-Object Name",
            "npm run build",
            "cargo test -p aura-core",
            "git status; git diff --stat",
            "git clean -n",
            "Write-Output 'remove the old lines by hand'",
            "Copy-Item a.txt b.txt",
            "Move-Item a.txt archive\\a.txt",
            "robocopy C:\\src C:\\dst /E",
            "dir /b",
            "python -m http.server 8000",
            "Get-Content .\\rd-notes.md",
            "Set-Location C:\\Dev\\Mini-Game\\rm-tools",
        ] {
            for shell in SHELLS {
                assert!(!deletes_files(shell, script), "{script}");
            }
        }
    }
}
