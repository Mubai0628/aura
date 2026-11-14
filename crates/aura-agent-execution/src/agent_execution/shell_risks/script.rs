//! A Shell script read as text: its statements, their words and arguments, and the places the
//! words name, resolved the way the script would resolve them.
use regex::Regex;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Named arguments that name the written or deleted place of a command that acts on its path.
pub(super) const PATHS: [&str; 3] = ["-path", "-literalpath", "-filepath"];
/// Named arguments that name a written place whatever the command.
pub(super) const DESTINATIONS: [&str; 3] = ["-destination", "-destinationpath", "-outfile"];

/// The script's statements: split at new lines, `;`, `|`, `&&`, `{` and `}` outside quotes. A
/// here-string is data, not commands: it stays one empty quoted word of its statement. `${...}`
/// names a variable and is not a block.
pub(super) fn statements(script: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let chars: Vec<char> = script.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        match (quote, c) {
            (Some(q), _) if c == q => {
                quote = None;
                current.push(c);
            }
            (Some(_), _) => current.push(c),
            (None, '@') if here_string_opens(&chars, index) => {
                index = here_string_end(&chars, index);
                current.push_str("''");
                continue;
            }
            (None, '\'' | '"') => {
                quote = Some(c);
                current.push(c);
            }
            (None, '$') if chars.get(index + 1) == Some(&'{') => {
                // `${env:TEMP}` up to its closing brace.
                let end = chars[index..].iter().position(|c| *c == '}').map(|at| index + at);
                let end = end.unwrap_or(chars.len() - 1);
                current.extend(&chars[index..=end]);
                index = end + 1;
                continue;
            }
            (None, '\n' | '\r' | ';' | '|' | '{' | '}') => {
                statements.push(std::mem::take(&mut current));
            }
            (None, '&') if chars.get(index + 1) == Some(&'&') => {
                index += 1;
                statements.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
        index += 1;
    }
    statements.push(current);
    statements.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// `@'` or `@"` followed by the end of its line opens a PowerShell here-string.
fn here_string_opens(chars: &[char], at: usize) -> bool {
    matches!(chars.get(at + 1), Some('\'' | '"'))
        && chars[at + 2..].iter().take_while(|c| **c != '\n').all(|c| c.is_whitespace())
}

/// Just past the here-string opened at `at`: its closing quote and `@` at the start of a line, or
/// the end of the script when it is never closed.
fn here_string_end(chars: &[char], at: usize) -> usize {
    let quote = chars[at + 1];
    let mut index = at + 2;
    while index < chars.len() {
        if chars[index] == '\n' {
            let mut line = index + 1;
            while chars.get(line).is_some_and(|c| *c == ' ' || *c == '\t') {
                line += 1;
            }
            if chars.get(line) == Some(&quote) && chars.get(line + 1) == Some(&'@') {
                return line + 2;
            }
        }
        index += 1;
    }
    chars.len()
}

/// A statement's words, quotes removed, each with whether it starts with a quote.
pub(super) fn words(statement: &str) -> Vec<(String, bool)> {
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

pub(super) fn command_name(word: &str) -> String {
    let name = word.rsplit(['\\', '/']).next().unwrap_or(word).to_ascii_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

/// Named place arguments with their values, and positional arguments; other named arguments and
/// their values are skipped, and CMD switches (`/s`) are not positional.
pub(super) fn arguments(words: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let (mut named, mut positional) = (Vec::new(), Vec::new());
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        let lower = word.to_ascii_lowercase();
        if let Some((name, _)) = lower.split_once(':').filter(|(name, _)| name.starts_with('-')) {
            if PATHS.contains(&name) || DESTINATIONS.contains(&name) {
                named.push((name.to_owned(), word[name.len() + 1..].to_owned()));
            }
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
pub(super) fn redirection(word: &str) -> Option<&str> {
    let rest = &word[word.find('>')?..];
    let rest = rest.strip_prefix(">>").or_else(|| rest.strip_prefix('>'))?;
    (!rest.starts_with('&')).then_some(rest)
}

/// The place a path names, absolute and normalized; `None` when the text does not say. `temp` is
/// what TEMP and TMP name for the script, when it has a folder of its own.
pub(super) fn resolve(named: &str, current: Option<&Path>, temp: Option<&Path>) -> Option<PathBuf> {
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
    let expanded = expand(named, temp)?;
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

/// `$env:NAME`, `${env:NAME}`, `%NAME%`, `~` and `$HOME` expanded from Aura's own environment,
/// with TEMP and TMP as `temp` when it is given.
fn expand(named: &str, temp: Option<&Path>) -> Option<String> {
    static ENV: OnceLock<Regex> = OnceLock::new();
    let pattern = ENV.get_or_init(|| {
        Regex::new(r"(?i)\$\{env:(\w+)\}|\$env:(\w+)|%(\w+)%|^\$home\b|^~").expect("valid")
    });
    let mut missing = false;
    let expanded = pattern.replace_all(named, |captures: &regex::Captures<'_>| {
        let name = captures.get(1).or(captures.get(2)).or(captures.get(3)).map(|m| m.as_str());
        let value = match name {
            Some(name) if name.eq_ignore_ascii_case("temp") || name.eq_ignore_ascii_case("tmp") => {
                temp.map(|folder| folder.display().to_string()).or_else(|| std::env::var(name).ok())
            }
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

pub(super) fn within(place: &Path, root: &Path) -> bool {
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

    #[test]
    fn blocks_and_here_strings_split_as_powershell_reads_them() {
        assert_eq!(
            statements("if (Test-Path $png) { Remove-Item $png -Force }"),
            vec!["if (Test-Path $png) ", " Remove-Item $png -Force "]
        );
        assert_eq!(
            statements("Set-Content ${env:TEMP}\\a.txt x"),
            vec!["Set-Content ${env:TEMP}\\a.txt x"]
        );
        let script = "@'\nconst a = require('fs'); rm -rf /\n{ x }\n'@ | Set-Content \"$env:TEMP\\a.js\"\nnode a.js";
        assert_eq!(
            statements(script),
            vec!["'' ", " Set-Content \"$env:TEMP\\a.js\"", "node a.js"]
        );
    }

    #[test]
    fn temp_is_the_conversations_own_when_it_has_one() {
        let own = Path::new(r"C:\Users\me\AppData\Local\Temp\aura-agents\ses_1");
        assert_eq!(resolve(r"$env:TEMP\a.txt", None, Some(own)), Some(own.join("a.txt")));
        assert_eq!(resolve(r"${env:TMP}\b", None, Some(own)), Some(own.join("b")));
        assert_eq!(resolve(r"%TEMP%\c", None, Some(own)), Some(own.join("c")));
    }
}
