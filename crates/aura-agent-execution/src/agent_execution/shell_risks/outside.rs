//! Where a Shell script writes outside the workspace. Below 完全访问 a script whose text writes to
//! a place outside the workspace asks every time. The script is read as text: the commands that
//! write files, folders or registry keys, their target arguments, output redirections and .NET
//! file calls, with `cd` followed and environment variables expanded. A target that cannot be
//! worked out from the text (a variable, a wildcard, a path a program builds) is not seen, and the
//! script runs as any other.
use regex::Regex;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Commands that write to the path they are given first.
const WRITES_FIRST: [&str; 26] = [
    "set-content",
    "sc",
    "add-content",
    "ac",
    "out-file",
    "tee-object",
    "tee",
    "new-item",
    "ni",
    "mkdir",
    "md",
    "new-itemproperty",
    "set-itemproperty",
    "sp",
    "set-item",
    "si",
    "export-csv",
    "epcsv",
    "export-clixml",
    "start-transcript",
    "rename-item",
    "rni",
    "ren",
    "rename",
    "clear-content",
    "clc",
];
/// Commands that write to the destination they are given second.
const WRITES_SECOND: [&str; 11] = [
    "copy-item",
    "copy",
    "cp",
    "cpi",
    "move-item",
    "move",
    "mv",
    "mi",
    "xcopy",
    "robocopy",
    "mklink",
];
/// Named arguments that name the written place of a command that writes to its path.
const PATHS: [&str; 3] = ["-path", "-literalpath", "-filepath"];
/// Named arguments that name a written place whatever the command.
const DESTINATIONS: [&str; 3] = ["-destination", "-destinationpath", "-outfile"];
const DIRECTORY: [&str; 6] = ["cd", "chdir", "set-location", "sl", "pushd", "push-location"];
/// Commands that run the script text they are given.
const NESTED: [&str; 5] = ["powershell", "pwsh", "cmd", "invoke-expression", "iex"];

/// The places outside `roots` that the script writes, as the script names them; `directory` is
/// where it starts, when known.
pub(crate) fn targets(script: &str, directory: Option<&Path>, roots: &[PathBuf]) -> Vec<String> {
    let mut current = directory.map(Path::to_path_buf);
    let mut found = Vec::new();
    let outside = |found: &mut Vec<String>, named: &str, current: &Option<PathBuf>| {
        if let Some(place) = resolve(named, current.as_deref())
            && !roots.iter().any(|root| within(&place, root))
            && !found.contains(&named.to_owned())
        {
            found.push(named.to_owned());
        }
    };
    for statement in statements(script) {
        let quoted_words = words(&statement);
        // Redirections, wherever they stand; text that starts in quotes is not one.
        let mut index = 0;
        while index < quoted_words.len() {
            let (word, quoted) = &quoted_words[index];
            if let Some(rest) = redirection(word).filter(|_| !quoted) {
                if rest.is_empty() {
                    if let Some((target, _)) = quoted_words.get(index + 1) {
                        outside(&mut found, target, &current);
                    }
                    index += 1;
                } else {
                    outside(&mut found, rest, &current);
                }
            }
            index += 1;
        }
        let mut words: Vec<String> = quoted_words.into_iter().map(|(word, _)| word).collect();
        if words.first().is_some_and(|word| word == "&") {
            words.remove(0);
        }
        let Some(command) = words.first().map(|word| command_name(word)) else {
            continue;
        };
        if NESTED.contains(&command.as_str()) {
            // The script it runs: the text after -c/-Command or /c, or the expression.
            let at = words[1..].iter().position(|word| {
                matches!(word.to_ascii_lowercase().as_str(), "-c" | "-command" | "/c" | "/k")
            });
            let inner = match at {
                Some(at) => words[at + 2..].join(" "),
                None if command.starts_with('i') => words[1..].join(" "),
                None => String::new(),
            };
            for place in targets(&inner, current.as_deref(), roots) {
                if !found.contains(&place) {
                    found.push(place);
                }
            }
            continue;
        }
        let (named, positional) = arguments(&words[1..]);
        if DIRECTORY.contains(&command.as_str()) {
            if let Some(place) = named
                .first()
                .map(|(_, path)| path)
                .or(positional.first())
                .and_then(|path| resolve(path, current.as_deref()))
            {
                current = Some(place);
            }
            continue;
        }
        let value = |names: &[&str]| {
            named.iter().find(|(name, _)| names.contains(&name.as_str())).map(|(_, v)| v)
        };
        let target = if WRITES_FIRST.contains(&command.as_str()) {
            value(&PATHS).or(positional.first())
        } else if command == "mklink" {
            positional.first()
        } else if WRITES_SECOND.contains(&command.as_str()) {
            value(&["-destination"]).or(positional.get(1))
        } else {
            None
        };
        for target in target.into_iter().chain(value(&DESTINATIONS)) {
            outside(&mut found, target, &current);
        }
    }
    for call in dotnet().captures_iter(script) {
        let place = if matches!(call[1].to_ascii_lowercase().as_str(), "copy" | "move" | "replace")
        {
            call.get(3).map(|m| m.as_str())
        } else {
            call.get(2).map(|m| m.as_str())
        };
        if let Some(place) = place {
            outside(&mut found, place, &current);
        }
    }
    found
}

/// .NET calls that write the file they name: the method, its first and second text arguments.
fn dotnet() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r#"(?i)\[(?:system\.)?io\.(?:file|directory)\]::(write\w*|append\w*|create\w*|copy|move|replace|setattributes)\s*\(\s*['"]([^'"]+)['"](?:\s*,\s*['"]([^'"]+)['"])?"#,
        )
        .expect("the .NET pattern is valid")
    })
}

/// The script's statements: split at new lines, `;`, `|` and `&&` outside quotes.
fn statements(script: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = script.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), _) if c == q => {
                quote = None;
                current.push(c);
            }
            (Some(_), _) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                current.push(c);
            }
            (None, '\n' | '\r' | ';' | '|') => statements.push(std::mem::take(&mut current)),
            (None, '&') if chars.peek() == Some(&'&') => {
                chars.next();
                statements.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    statements.push(current);
    statements.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// A statement's words, quotes removed, each with whether it starts with a quote.
fn words(statement: &str) -> Vec<(String, bool)> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let (mut quoted, mut starts_quoted) = (false, false);
    for c in statement.chars() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                starts_quoted |= current.is_empty() && !quoted;
                quoted = true;
            }
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() || quoted {
                    words.push((std::mem::take(&mut current), starts_quoted));
                }
                (quoted, starts_quoted) = (false, false);
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() || quoted {
        words.push((current, starts_quoted));
    }
    words
}

fn command_name(word: &str) -> String {
    let name = word.rsplit(['\\', '/']).next().unwrap_or(word).to_ascii_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

/// Named place arguments with their values, and positional arguments; other named arguments and
/// their values are skipped, and CMD switches (`/s`) are not positional.
fn arguments(words: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let (mut named, mut positional) = (Vec::new(), Vec::new());
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        let lower = word.to_ascii_lowercase();
        if let Some((name, value)) = lower.split_once(':').filter(|(name, _)| name.starts_with('-'))
        {
            if PATHS.contains(&name) || DESTINATIONS.contains(&name) {
                named.push((name.to_owned(), word[name.len() + 1..].to_owned()));
            }
            let _ = value;
        } else if lower.starts_with('-') && lower.len() > 1 {
            if PATHS.contains(&lower.as_str()) || DESTINATIONS.contains(&lower.as_str()) {
                if let Some(value) = words.get(index + 1) {
                    named.push((lower.clone(), value.clone()));
                }
                index += 1;
            } else if words.get(index + 1).is_some_and(|next| !next.starts_with('-'))
                && takes_value(&lower)
            {
                index += 1;
            }
        } else if !(lower.starts_with('/') && lower.len() <= 4) && redirection(word).is_none() {
            positional.push(word.clone());
        }
        index += 1;
    }
    (named, positional)
}

/// Named arguments that take a value, which is not a positional argument.
fn takes_value(name: &str) -> bool {
    matches!(
        name,
        "-value"
            | "-itemtype"
            | "-type"
            | "-encoding"
            | "-name"
            | "-newname"
            | "-propertytype"
            | "-filter"
            | "-include"
            | "-exclude"
            | "-delimiter"
            | "-inputobject"
            | "-credential"
    )
}

/// The target of an output redirection in a word: `>`, `>>`, `2>`, `*>` alone (the target
/// follows), or with text before it or the target attached (`echo x>out.txt`).
fn redirection(word: &str) -> Option<&str> {
    let rest = &word[word.find('>')?..];
    let rest = rest.strip_prefix(">>").or_else(|| rest.strip_prefix('>'))?;
    (!rest.starts_with('&')).then_some(rest)
}

/// The place a written path names, absolute and normalized; `None` when the text does not say.
fn resolve(named: &str, current: Option<&Path>) -> Option<PathBuf> {
    let named = named.trim();
    let lower = named.to_ascii_lowercase();
    if lower.is_empty() || lower == "$null" || lower == "nul" || lower.starts_with("nul:") {
        return None;
    }
    // Places that are not files: the registry and certificates are outside any workspace; the
    // session's own environment, variables and functions are not written to disk.
    if ["hkcu:", "hklm:", "hkcr:", "hku:", "hkcc:", "registry::", "cert:"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return Some(PathBuf::from(r"\\registry").join(named));
    }
    if ["env:", "variable:", "function:", "alias:", "wsman:"].iter().any(|p| lower.starts_with(p)) {
        return None;
    }
    let expanded = expand(named)?;
    if expanded.contains(['*', '?', '$', '`']) {
        return None;
    }
    let path = Path::new(&expanded);
    let absolute = if path.is_absolute() || expanded.starts_with(r"\\") {
        path.to_path_buf()
    } else {
        current?.join(path)
    };
    Some(normalized(&absolute))
}

/// `$env:NAME`, `${env:NAME}`, `%NAME%`, `~` and `$HOME` expanded from Aura's own environment.
fn expand(named: &str) -> Option<String> {
    static ENV: OnceLock<Regex> = OnceLock::new();
    let pattern = ENV.get_or_init(|| {
        Regex::new(r"(?i)\$\{env:(\w+)\}|\$env:(\w+)|%(\w+)%|^\$home\b|^~").expect("valid")
    });
    let mut missing = false;
    let expanded = pattern.replace_all(named, |captures: &regex::Captures<'_>| {
        let name = captures.get(1).or(captures.get(2)).or(captures.get(3)).map(|m| m.as_str());
        let value = match name {
            Some(name) => std::env::var(name).ok(),
            None => std::env::var("USERPROFILE").ok(),
        };
        value.unwrap_or_else(|| {
            missing = true;
            String::new()
        })
    });
    (!missing).then(|| expanded.into_owned())
}

fn normalized(path: &Path) -> PathBuf {
    let mut parts = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            other => parts.push(other.as_os_str()),
        }
    }
    parts
}

fn within(place: &Path, root: &Path) -> bool {
    let key = |path: &Path| {
        let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
        let text = text.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(text);
        text.trim_end_matches('\\').to_owned()
    };
    let (place, root) = (key(place), key(root));
    place == root || place.starts_with(&format!("{root}\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(script: &str) -> Vec<String> {
        targets(script, Some(Path::new(r"C:\Dev\project\src")), &[PathBuf::from(r"C:\Dev\project")])
    }

    #[test]
    fn writes_outside_the_workspace_are_seen() {
        for (script, place) in [
            (r"Set-Content C:\Windows\x.txt 'a'", r"C:\Windows\x.txt"),
            (r"Add-Content -Path C:\Users\me\notes.txt -Value x", r"C:\Users\me\notes.txt"),
            (r"'x' | Out-File -FilePath D:\out.txt", r"D:\out.txt"),
            (r"Copy-Item .\a.txt C:\Temp\a.txt", r"C:\Temp\a.txt"),
            (
                r"Copy-Item -Path a.txt -Destination 'C:\Program Files (x86)\App'",
                r"C:\Program Files (x86)\App",
            ),
            (r"Move-Item a.txt ..\..\elsewhere", r"..\..\elsewhere"),
            (r"echo hi > C:\Temp\hi.txt", r"C:\Temp\hi.txt"),
            (r"Get-Date >> ..\..\log.txt", r"..\..\log.txt"),
            (r"dir 2>C:\err.txt", r"C:\err.txt"),
            (r"New-Item -ItemType Directory C:\NewFolder", r"C:\NewFolder"),
            (r"mkdir \\server\share\x", r"\\server\share\x"),
            (r"copy a.txt D:\backup\", r"D:\backup\"),
            (r"robocopy . E:\mirror /E", r"E:\mirror"),
            (r"Set-ItemProperty HKCU:\Software\X -Name Y -Value 1", r"HKCU:\Software\X"),
            (r"cd C:\Users; Set-Content notes.txt x", "notes.txt"),
            (r"Set-Location -Path D:\data; New-Item a.txt", "a.txt"),
            (r"[System.IO.File]::WriteAllText('C:\Temp\a.txt', 'x')", r"C:\Temp\a.txt"),
            (r"[IO.File]::Copy('a.txt', 'C:\Temp\b.txt')", r"C:\Temp\b.txt"),
            (r#"powershell -c "Set-Content C:\Temp\x.txt 1""#, r"C:\Temp\x.txt"),
            (r#"cmd /c "echo x > C:\Temp\y.txt""#, r"C:\Temp\y.txt"),
            (r"echo x>C:\Temp\z.txt", r"C:\Temp\z.txt"),
            (r#"echo x>"C:\Temp\q.txt""#, r"C:\Temp\q.txt"),
        ] {
            assert_eq!(found(script), vec![place.to_owned()], "{script}");
        }
    }

    #[test]
    fn writes_inside_and_what_the_text_does_not_say_run_unasked() {
        for script in [
            "Set-Content notes.txt 'a'",
            r"Set-Content C:\Dev\project\notes.txt x",
            r"Copy-Item C:\Users\me\template.txt .\template.txt",
            r"Copy-Item ..\README.md docs\README.md",
            "echo hi > out.txt",
            "npm run build 2>&1",
            "dir > $null",
            "git status > nul",
            "Set-Content $path 'x'",
            r"Out-File -FilePath $env:NO_SUCH_VARIABLE_HERE\x.txt",
            r"Set-Content C:\Temp\*.txt x",
            r"Get-Content C:\Windows\win.ini",
            "Set-Item Env:FOO bar",
            r"cd C:\Users; Get-ChildItem",
            r"python build.py --out C:\dist",
            r"cd ..; Set-Content notes.txt x",
            r"Write-Output 'see > C:\Temp\x.txt'",
        ] {
            assert_eq!(found(script), Vec::<String>::new(), "{script}");
        }
    }

    #[test]
    fn without_a_known_directory_only_absolute_places_are_seen() {
        let roots = [PathBuf::from(r"C:\Dev\project")];
        assert_eq!(targets(r"Set-Content C:\Temp\a.txt x", None, &roots), vec![r"C:\Temp\a.txt"]);
        assert_eq!(targets(r"Set-Content ..\a.txt x", None, &roots), Vec::<String>::new());
    }
}
