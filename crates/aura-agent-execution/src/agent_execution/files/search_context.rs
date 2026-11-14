//! The lines around an `fs.search`'s matches in workspace files, when the call asks for them:
//! read from the file the search matched, and only while it still holds the text the search read.
use super::*;
use aura_workspace_domain::{FsSearchContext, FsSearchContextLine};
use std::collections::BTreeSet;

/// The most lines a call may ask for on each side of a match.
pub(super) const MAX_LINES: u32 = 10;
/// The most characters of one context line the result keeps.
const LINE_CHARS: usize = 400;

/// Adds to `result` up to `lines` lines before and after each match, for each file whose text is
/// still the one its matches came from. A file that changed since, or that cannot be read whole,
/// gets none; its matches stay as they are.
pub(super) fn add(
    service: &AgentExecution,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    result: &mut FsSearchTextResult,
    lines: u32,
) {
    let around = u64::from(lines.min(MAX_LINES));
    if around == 0 {
        return;
    }
    let mut files: Vec<(WorkspacePath, ContentHash, BTreeSet<u64>)> = Vec::new();
    for found in &result.matches {
        match files.iter_mut().find(|(path, _, _)| *path == found.path) {
            Some((_, _, matched)) => {
                matched.insert(found.line_index);
            }
            None => files.push((
                found.path.clone(),
                found.logical_hash.clone(),
                BTreeSet::from([found.line_index]),
            )),
        }
    }
    for (path, hash, matched) in files {
        let Ok(read) = service.fs.read_text(FsReadTextRequest {
            workspace_id,
            session_id,
            target: WorkspaceFileTarget { path: path.clone() },
            range: TextReadRange::Full,
            explicit_encoding: None,
        }) else {
            continue;
        };
        if read.logical_hash != hash {
            continue;
        }
        let body = read.text.strip_suffix('\n').unwrap_or(&read.text);
        let text: Vec<&str> =
            body.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
        let shown = context_lines(&text, &matched, around);
        if !shown.is_empty() {
            result.context.push(FsSearchContext { path, lines: shown });
        }
    }
}

/// The lines within `around` of a matched line that are not matched themselves, in order.
fn context_lines(text: &[&str], matched: &BTreeSet<u64>, around: u64) -> Vec<FsSearchContextLine> {
    let last = u64::try_from(text.len()).unwrap_or(u64::MAX).saturating_sub(1);
    let mut wanted = BTreeSet::new();
    for &line in matched {
        for index in line.saturating_sub(around)..=line.saturating_add(around).min(last) {
            if !matched.contains(&index) {
                wanted.insert(index);
            }
        }
    }
    wanted
        .into_iter()
        .filter_map(|index| {
            let line = *text.get(usize::try_from(index).ok()?)?;
            let truncated = line.chars().count() > LINE_CHARS;
            Some(FsSearchContextLine {
                line_index: index,
                text: if truncated {
                    line.chars().take(LINE_CHARS).collect()
                } else {
                    line.to_owned()
                },
                truncated,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_lines_are_the_neighbours_of_matches_that_are_not_matches_themselves() {
        let text = ["a", "b", "c", "d", "e", "f", "g"];
        let shown = context_lines(&text, &BTreeSet::from([1, 2, 6]), 1);
        let indexes: Vec<u64> = shown.iter().map(|line| line.line_index).collect();
        assert_eq!(indexes, vec![0, 3, 5], "around 1 and 2, then 6 at the end of the file");
        assert_eq!(shown[1].text, "d");
    }

    #[test]
    fn a_long_context_line_is_cut_and_says_so() {
        let long = "x".repeat(LINE_CHARS + 5);
        let text = ["match", long.as_str()];
        let shown = context_lines(&text, &BTreeSet::from([0]), 2);
        assert_eq!(shown.len(), 1);
        assert!(shown[0].truncated);
        assert_eq!(shown[0].text.chars().count(), LINE_CHARS);
    }
}
