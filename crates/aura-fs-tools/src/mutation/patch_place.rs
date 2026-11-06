//! Where a hunk's lines are in the file, and, for a hunk that fits nowhere, what the file has
//! instead.

/// A placed hunk's old lines: where they start in the file and how many there are.
#[derive(Clone, Copy, Debug)]
pub(super) struct Region {
    pub at: usize,
    pub len: usize,
    pub hunk: usize,
}

impl Region {
    /// The lines `at..at + len` share a line with these, or one of the two is an insertion point
    /// strictly inside the other.
    fn overlaps(&self, at: usize, len: usize) -> bool {
        match (self.len, len) {
            (0, 0) => false,
            (0, _) => at < self.at && self.at < at + len,
            (_, 0) => self.at < at && at < self.at + self.len,
            _ => self.at < at + len && at < self.at + self.len,
        }
    }
}

/// The same line, its line ending aside, and with `loose` its trailing spaces too.
pub(super) fn same(file: &str, diff: &str, loose: bool) -> bool {
    let file = file.trim_end_matches(['\r', '\n']);
    let diff = diff.trim_end_matches(['\r', '\n']);
    file == diff || loose && file.trim_end() == diff.trim_end()
}

/// The lines are in the file somewhere, trailing spaces aside.
pub(super) fn anywhere(lines: &[&str], old: &[&str]) -> bool {
    (0..=lines.len().saturating_sub(old.len())).any(|at| fits(lines, at, old, true))
}

fn fits(lines: &[&str], at: usize, old: &[&str], loose: bool) -> bool {
    at + old.len() <= lines.len()
        && old.iter().enumerate().all(|(i, line)| same(lines[at + i], line, loose))
}

/// Where a hunk's old lines start: first at or after `cursor`, the end of the hunks before it, as
/// for a diff in the file's order; then above it, outside the lines earlier hunks change, since a
/// diff's hunks may come in any order. Where its numbers say when its lines are there; otherwise
/// where its lines are, if that is one place (the nearest to its numbers when they give one),
/// trailing spaces aside if need be.
pub(super) fn place(
    lines: &[&str],
    cursor: usize,
    old: &[&str],
    declared: Option<usize>,
    placed: &[Region],
) -> Result<usize, String> {
    if old.is_empty() {
        return declared
            .filter(|at| {
                (cursor..=lines.len()).contains(at)
                    && !placed.iter().any(|region| region.overlaps(*at, 0))
            })
            .ok_or_else(|| {
                "it has no unchanged or removed lines to place it by; add the lines around the \
                 change"
                    .to_owned()
            });
    }
    if let Some(at) = declared.filter(|at| *at >= cursor && fits(lines, *at, old, false)) {
        return Ok(at);
    }
    let free = |at: usize| !placed.iter().any(|region| region.overlaps(at, old.len()));
    let end = lines.len().saturating_sub(old.len()) + 1;
    for range in [cursor.min(end)..end, 0..cursor.min(end)] {
        for loose in [false, true] {
            let found = range
                .clone()
                .filter(|at| fits(lines, *at, old, loose) && free(*at))
                .collect::<Vec<_>>();
            match (found.as_slice(), declared) {
                ([], _) => {}
                ([one], _) => return Ok(*one),
                (many, Some(near)) => {
                    return Ok(many
                        .iter()
                        .copied()
                        .min_by_key(|at| at.abs_diff(near))
                        .unwrap_or(near));
                }
                (many, None) => {
                    return Err(format!(
                        "its lines appear {} times in the file; include more unchanged lines \
                         around the change",
                        many.len()
                    ));
                }
            }
        }
    }
    Err(explain(lines, old, declared, placed))
}

/// Why a hunk fits nowhere: its lines are lines another hunk already changes, or the place where
/// the most of its first lines are, with the line that differs there and what the file has.
fn explain(lines: &[&str], old: &[&str], declared: Option<usize>, placed: &[Region]) -> String {
    let taken = (0..=lines.len().saturating_sub(old.len()))
        .filter(|at| fits(lines, *at, old, true))
        .find_map(|at| placed.iter().find(|region| region.overlaps(at, old.len())));
    if let Some(region) = taken {
        return format!(
            "its lines are lines hunk #{} also changes (from line {}); put both changes into one \
             hunk",
            region.hunk + 1,
            region.at + 1
        );
    }
    let matched = |at: usize| {
        old.iter().zip(&lines[at..]).take_while(|(diff, file)| same(file, diff, true)).count()
    };
    // The most leading lines matched; on a tie, the place nearest to the hunk's numbers, or the
    // first.
    let mut best: Option<(usize, usize)> = None;
    for at in 0..lines.len() {
        let count = matched(at);
        let better = count > 0
            && best.is_none_or(|(most, place)| {
                count > most
                    || count == most && declared.is_some_and(|d| at.abs_diff(d) < place.abs_diff(d))
            });
        if better {
            best = Some((count, at));
        }
    }
    if let Some((count, at)) = best
        && let Some(expected) = old.get(count)
    {
        let number = at + count + 1;
        let shown = match lines.get(at + count) {
            None => format!(
                "line {number} should be {} but the file has only {} lines",
                show(expected),
                lines.len()
            ),
            Some(actual) => format!(
                "line {number} should be {} but the file has {}{}",
                show(expected),
                show(actual),
                if actual.trim() == expected.trim() {
                    " (only the indentation differs)"
                } else {
                    ""
                }
            ),
        };
        let from = if count == 1 {
            format!("line {}", at + 1)
        } else {
            format!("lines {}–{}", at + 1, at + count)
        };
        return format!("{shown}; its lines before that are {from}");
    }
    let first = old[0];
    if !first.trim().is_empty()
        && let Some(at) = lines.iter().position(|line| line.trim() == first.trim())
    {
        return format!(
            "its first line {} is in the file only with other indentation, at line {}: {}",
            show(first),
            at + 1,
            show(lines[at])
        );
    }
    match declared {
        Some(at) => at_declared(lines, old, at),
        None => format!("its first line {} is not in the file", show(first)),
    }
}

/// The first of a hunk's lines that differs where its numbers say it is.
fn at_declared(lines: &[&str], old: &[&str], at: usize) -> String {
    for (i, expected) in old.iter().enumerate() {
        let number = at + i + 1;
        match lines.get(at + i) {
            None => {
                return format!(
                    "line {number} should be {} but the file has only {} lines",
                    show(expected),
                    lines.len()
                );
            }
            Some(actual) if !same(actual, expected, true) => {
                return format!(
                    "line {number} should be {} but the file has {}",
                    show(expected),
                    show(actual)
                );
            }
            Some(_) => {}
        }
    }
    "its lines are not in the file in this order".to_owned()
}

fn show(text: &str) -> String {
    let text = text.trim_end_matches(['\r', '\n']);
    let shown = text.chars().take(160).collect::<String>();
    if shown.len() < text.len() { format!("«{shown}…»") } else { format!("«{shown}»") }
}
