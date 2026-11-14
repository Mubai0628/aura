//! Where a Shell script writes, and where it deletes. The script is read as text: the commands
//! that write files, folders or registry keys and those that delete, their target arguments,
//! output redirections and .NET file calls, with `cd` followed and environment variables expanded.
//! A written place that cannot be worked out from the text (a variable, a wildcard, a path a
//! program builds) is not seen. A deletion whose targets cannot all be worked out is not known.
use super::Area;
use super::deletion;
use super::script::{
    DESTINATIONS, PATHS, arguments, command_name, redirection, resolve, statements, within, words,
};
use regex::Regex;
use std::path::{Path, PathBuf};
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
/// Commands that delete the paths they are given.
const DELETES: [&str; 7] = ["remove-item", "ri", "rm", "rmdir", "rd", "del", "erase"];
const DIRECTORY: [&str; 6] = ["cd", "chdir", "set-location", "sl", "pushd", "push-location"];
/// Commands that run the script text they are given.
const NESTED: [&str; 5] = ["powershell", "pwsh", "cmd", "invoke-expression", "iex"];

pub(super) struct Places {
    /// Places outside the area's roots that the script writes or deletes, as it names them.
    pub outside: Vec<String>,
    /// Whether a statement deletes.
    pub deletes: bool,
    /// Whether every deleting statement names all its targets and each lies where the area may
    /// delete.
    pub deletions_allowed: bool,
    /// Whether every deleting statement is a deletion command whose targets lie where the area
    /// may delete, or are left to a variable or the pipeline while the command stands there.
    pub deletions_here: bool,
}

/// The places of `script`; `directory` is where it starts, when known.
pub(super) fn of(script: &str, directory: Option<&Path>, area: &Area) -> Places {
    let mut current = directory.map(Path::to_path_buf);
    let mut places = Places {
        outside: Vec::new(),
        deletes: false,
        deletions_allowed: true,
        deletions_here: true,
    };
    let temp = area.temp.as_deref();
    let deletable = |place: &Path| area.deletable.iter().any(|root| within(place, root));
    let outside = |places: &mut Places, named: &str, current: &Option<PathBuf>| {
        if let Some(place) = resolve(named, current.as_deref(), temp)
            && !area.roots.iter().any(|root| within(&place, root))
            && !places.outside.contains(&named.to_owned())
        {
            places.outside.push(named.to_owned());
        }
    };
    for statement in statements(script) {
        let deleting = deletion::deletes_files(&statement);
        let quoted_words = words(&statement);
        // Redirections, wherever they stand; text that starts in quotes is not one.
        let mut index = 0;
        while index < quoted_words.len() {
            let (word, quoted) = &quoted_words[index];
            if let Some(rest) = redirection(word).filter(|_| !quoted) {
                if rest.is_empty() {
                    if let Some((target, _)) = quoted_words.get(index + 1) {
                        outside(&mut places, target, &current);
                    }
                    index += 1;
                } else {
                    outside(&mut places, rest, &current);
                }
            }
            index += 1;
        }
        let mut words: Vec<String> = quoted_words.into_iter().map(|(word, _)| word).collect();
        if words.first().is_some_and(|word| word == "&") {
            words.remove(0);
        }
        let Some(command) = words.first().map(|word| command_name(word)) else {
            places.deletes |= deleting;
            places.deletions_allowed &= !deleting;
            places.deletions_here &= !deleting;
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
            let nested = of(&inner, current.as_deref(), area);
            for place in nested.outside {
                if !places.outside.contains(&place) {
                    places.outside.push(place);
                }
            }
            places.deletes |= deleting || nested.deletes;
            // A deletion the inner text does not show (an encoded command) is not known.
            let shown = !deleting || nested.deletes;
            places.deletions_allowed &= nested.deletions_allowed && shown;
            places.deletions_here &= nested.deletions_here && shown;
            continue;
        }
        let (named, positional) = arguments(&words[1..]);
        if DIRECTORY.contains(&command.as_str()) {
            // A folder the text does not name leaves where later statements stand unknown.
            current = named
                .first()
                .map(|(_, path)| path)
                .or(positional.first())
                .and_then(|path| resolve(path, current.as_deref(), temp));
            places.deletes |= deleting;
            places.deletions_allowed &= !deleting;
            places.deletions_here &= !deleting;
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
            outside(&mut places, target, &current);
        }
        if !deleting {
            continue;
        }
        places.deletes = true;
        // Only a deletion command's targets are read; another deletion is not placed at all.
        let command_deletes = DELETES.contains(&command.as_str());
        let stands_here = current.as_deref().is_some_and(deletable);
        let targets = deleted(&command, &words, &named, &positional);
        if targets.is_empty() {
            places.deletions_allowed = false;
            places.deletions_here &= command_deletes && stands_here;
        }
        for target in targets {
            match deleted_place(&target, current.as_deref(), temp) {
                Some(place) => {
                    if !area.roots.iter().any(|root| within(&place, root))
                        && !places.outside.contains(&target)
                    {
                        places.outside.push(target.clone());
                    }
                    places.deletions_allowed &= deletable(place.as_path());
                    places.deletions_here &= deletable(place.as_path());
                }
                None => {
                    places.deletions_allowed = false;
                    places.deletions_here &= command_deletes && stands_here && unplaced(&target);
                }
            }
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
            outside(&mut places, place, &current);
        }
    }
    places
}

/// The targets a deleting command names: the paths of a deletion command, the folder `git clean`
/// cleans, the destination robocopy mirrors to. Empty when the text does not say.
fn deleted(
    command: &str,
    words: &[String],
    named: &[(String, String)],
    positional: &[String],
) -> Vec<String> {
    if DELETES.contains(&command) {
        return named
            .iter()
            .filter(|(name, _)| PATHS.contains(&name.as_str()))
            .map(|(_, value)| value)
            .chain(positional)
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
    }
    if command == "git" && words.iter().any(|word| word.eq_ignore_ascii_case("clean")) {
        let folder = words
            .iter()
            .position(|word| word == "-C")
            .and_then(|at| words.get(at + 1))
            .cloned()
            .unwrap_or_else(|| ".".to_owned());
        return vec![folder];
    }
    if command == "robocopy" {
        return positional.get(1).cloned().into_iter().collect();
    }
    Vec::new()
}

/// A target the text leaves to where the command stands: a variable, or a relative path a
/// wildcard leaves open, that neither starts from a root nor climbs out of the folder.
fn unplaced(target: &str) -> bool {
    let target = target.trim().trim_matches(['"', '\'']);
    let lower = target.to_ascii_lowercase();
    let rooted = target.starts_with(['\\', '/', '~', '%'])
        || target.as_bytes().get(1) == Some(&b':')
        || ["$env:", "${env:", "$home"].iter().any(|start| lower.starts_with(start));
    !rooted && !target.split(['\\', '/']).any(|part| part == "..")
}

/// The place a deletion target names: a wildcard in its last part deletes in the folder before
/// it, and nowhere else.
fn deleted_place(target: &str, current: Option<&Path>, temp: Option<&Path>) -> Option<PathBuf> {
    let (folder, last) = match target.rfind(['\\', '/']) {
        Some(at) => (&target[..at], &target[at + 1..]),
        None => ("", target),
    };
    if !last.contains(['*', '?']) {
        return resolve(target, current, temp);
    }
    if folder.is_empty() {
        return current.map(Path::to_path_buf);
    }
    resolve(folder, current, temp)
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

#[cfg(test)]
#[path = "places_tests.rs"]
mod tests;
