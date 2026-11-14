//! Whether a Shell
//! script deletes files. Every way PowerShell and CMD delete is permanent — nothing goes to the
//! Recycle Bin — so such a script is asked every time (`OperationRiskFlag::PermanentFileDeletion`),
//! and the model is told to delete with `fs.delete`, which recycles. A script is text, not a plan:
//! this reads the commands a model writes to delete, and errs toward asking. A word that only looks
//! like one (`git commit -m "fix rd"`) asks once more than needed; it never asks less.
use regex::Regex;
use std::sync::OnceLock;

/// A command that deletes, where a command starts: the script's start, a new line, `;`, `|`, `&`,
/// `(`, `{` or a quote (a script handed to `powershell -c`, `Invoke-Expression` or `cmd /c`), an
/// optional call operator, and the word followed by a space or the end.
const COMMAND: &str = r#"(?im)(?:^|[;|&({\r\n"'])\s*(?:&\s*)?(?:remove-item|ri|rm|rmdir|rd|del|erase|clear-recyclebin|remove-itemproperty\s+-path\s+\S*::filesystem)(?:\.exe)?(?:\s|$|[;|&)}])"#;
/// .NET and COM deletions a PowerShell script can make.
const DOTNET: &str = r"(?i)\[(?:system\.)?io\.(?:file|directory)\]::delete\s*\(|\]::delete(?:file|directory)\s*\(|\.delete\s*\(\s*(?:\$true|\$false)?\s*\)";
/// `git clean` with a force flag deletes untracked files; robocopy's /MIR and /PURGE delete what
/// the source lacks; `cmd /c del` from PowerShell is still a deletion; an encoded PowerShell command
/// cannot be read at all, so it is asked like one.
const TOOLS: &str = r"(?i)\bgit(?:\.exe)?\s+clean\b[^\r\n;|&]*\s-(?:-force|[a-z]*f)|\brobocopy(?:\.exe)?\b[^\r\n;|&]*\s/(?:mir|purge)\b|\bcmd(?:\.exe)?\s+/[ck]\s+[^\r\n]*\b(?:del|erase|rd|rmdir)\b|\b(?:powershell|pwsh)(?:\.exe)?\b[^\r\n]*\s-(?:encodedcommand|enc|ec)\b";

fn patterns() -> &'static [Regex; 3] {
    static PATTERNS: OnceLock<[Regex; 3]> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [COMMAND, DOTNET, TOOLS]
            .map(|pattern| Regex::new(pattern).expect("the deletion patterns are valid"))
    })
}

/// Whether the script deletes files, for either Shell. CMD's own commands are among PowerShell's
/// aliases, and a CMD script can start PowerShell, so both are read the same way.
pub(super) fn deletes_files(script: &str) -> bool {
    patterns().iter().any(|pattern| pattern.is_match(script))
}

/// VisualBasic's `DeleteFile` and `DeleteDirectory` with `SendToRecycleBin` move to the Recycle
/// Bin, as `fs.delete` does: not a deletion. Returns the script with each such call replaced by an
/// empty string, and the first argument of each call that names its place in quotes.
pub(super) fn recycling(script: &str) -> (String, Vec<String>) {
    static CALL: OnceLock<Regex> = OnceLock::new();
    let call = CALL.get_or_init(|| {
        Regex::new(r"(?i)\[[\w.]+\]::delete(?:file|directory)\s*\(")
            .expect("the call pattern is valid")
    });
    let mut kept = String::new();
    let mut places = Vec::new();
    let mut from = 0;
    for found in call.find_iter(script) {
        if found.start() < from {
            continue;
        }
        let Some(end) = closing(script, found.end()) else { break };
        let arguments = &script[found.end()..end];
        if !arguments.to_ascii_lowercase().contains("sendtorecyclebin") {
            continue;
        }
        if let Some(place) = quoted_first(arguments) {
            places.push(place);
        }
        kept.push_str(&script[from..found.start()]);
        kept.push_str("''");
        from = end + 1;
    }
    kept.push_str(&script[from..]);
    (kept, places)
}

/// The index of the `)` that closes the call whose arguments start at `start`, quotes skipped.
fn closing(script: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    for (offset, c) in script[start..].char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') if depth == 0 => return Some(start + offset),
            (None, ')') => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The first argument when it is text in quotes with nothing to expand.
fn quoted_first(arguments: &str) -> Option<String> {
    let first = arguments.trim_start();
    let quote = first.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let rest = &first[1..];
    let text = &rest[..rest.find(quote)?];
    (!(quote == '"' && text.contains(['$', '`']))).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletions_quoted_wrapped_or_encoded_are_seen() {
        for script in [
            "powershell -c \"Remove-Item x\"",
            "pwsh -Command 'Remove-Item x -Recurse'",
            "Invoke-Expression 'Remove-Item x'",
            "iex \"rm -r dist\"",
            "cmd /c \"del /q x\"",
            "powershell -EncodedCommand UgBlAG0AbwB2AGUALQBJAHQAZQBtACAAeAA=",
            "pwsh -enc UgBlAG0A",
            "powershell.exe -NoProfile -ec UgBlAG0A",
        ] {
            assert!(deletes_files(script), "{script}");
        }
    }

    /// What a pattern cannot read stays unseen; the user documentation says so.
    #[test]
    fn indirect_deletions_remain_a_known_limit() {
        for script in [
            "python -c \"import shutil; shutil.rmtree('dist')\"",
            "python cleanup.py",
            "node -e \"require('fs').rmSync('dist',{recursive:true})\"",
            "$c='Remove-Item'; & $c x",
            "Set-Content x ''",
            "Move-Item x $env:TEMP",
            "node --enable-source-maps build.mjs",
        ] {
            assert!(!deletes_files(script), "{script}");
        }
    }

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
            assert!(deletes_files(script), "{script}");
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
            assert!(!deletes_files(script), "{script}");
        }
    }
}
