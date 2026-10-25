use super::*;
use diffy::{Line, Patch};
use std::borrow::Cow;

/// Applying a unified diff to decoded text, re-encoded in the target's own
/// convention.
pub(crate) fn prepare_patch_bytes(
    path: &WorkspacePath,
    state: &ExistingState,
    unified_diff: &str,
) -> Result<Vec<u8>, FsToolError> {
    let prepared = prepare_diff(unified_diff)?;
    let patch = parse_patch(&prepared.text)?;
    validate_patch_target(path, &patch, prepared.named.as_deref())?;
    patched_bytes(path, state, &patch, prepared.unplaced)
}

/// A diff as Aura reads it, with what the reading found.
pub(crate) struct PreparedDiff {
    pub text: String,
    /// Its hunks named no line numbers (`@@` alone, as in the apply_patch format): each is placed
    /// where its lines are.
    pub unplaced: bool,
    /// The file an apply_patch envelope names (`*** Update File: …`).
    pub named: Option<String>,
}

/// The
/// apply_patch envelope is read, and a diff whose hunks name no line numbers gets running numbers
/// so the parser keeps them; [`apply_text`] then places each by its lines.
pub(crate) fn prepare_diff(diff: &str) -> Result<PreparedDiff, FsToolError> {
    let mut named = None;
    let mut lines = Vec::new();
    for line in diff.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        if matches!(bare, "*** Begin Patch" | "*** End Patch" | "*** End of File") {
            continue;
        }
        if let Some(file) = bare.strip_prefix("*** Update File:") {
            named = Some(file.trim().to_owned());
            continue;
        }
        if bare.starts_with("*** Add File:") || bare.starts_with("*** Delete File:") {
            return Err(FsToolError::InvalidPatch(
                "a patch changes one existing file; create a file with fs.write and delete one \
                 with fs.delete"
                    .into(),
            ));
        }
        if bare.starts_with("*** Move to:") {
            return Err(FsToolError::InvalidPatch("a patch cannot move a file".into()));
        }
        lines.push(line.to_owned());
    }
    let unplaced = lines.iter().any(|line| line.starts_with("@@") && !numbered(line));
    if unplaced {
        running_headers(&mut lines);
    }
    let text = lines.concat();
    Ok(PreparedDiff { text: normalize_hunk_counts(&text).into_owned(), unplaced, named })
}

/// A hunk header with its line numbers: `@@ -12,4 +12,6 @@`.
fn numbered(line: &str) -> bool {
    let Some(header) = line.strip_prefix("@@ -") else { return false };
    let Some((old, rest)) = header.split_once(" +") else { return false };
    let Some((new, _)) = rest.split_once(" @@") else { return false };
    range(old).is_some() && range(new).is_some()
}

/// Every hunk header becomes a numbered one, its numbers running on from the hunk before; only the
/// counts mean anything, since each hunk is then placed by its lines.
fn running_headers(lines: &mut [String]) {
    let headers = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("@@"))
        .map(|(at, _)| at)
        .collect::<Vec<_>>();
    let (mut old_start, mut new_start) = (1, 1);
    for (k, &at) in headers.iter().enumerate() {
        let end = headers.get(k + 1).copied().unwrap_or(lines.len());
        let (mut old, mut new) = (0, 0);
        for line in &lines[at + 1..end] {
            match line.as_bytes().first() {
                Some(b'-') => old += 1,
                Some(b'+') => new += 1,
                Some(b'\\') => {}
                _ => {
                    old += 1;
                    new += 1;
                }
            }
        }
        lines[at] = format!("@@ -{old_start},{old} +{new_start},{new} @@\n");
        old_start += old + 1;
        new_start += new + 1;
    }
}

/// Repair only the declared line counts. Positions, line contents, file names
/// and newline markers remain byte-for-byte inputs to the strict diff parser.
/// A hunk is recounted only when its body is all diff lines and ends at the next
/// hunk or at the end of the diff. Any other hunk keeps its header, so the parser
/// still rejects it instead of reading it as an empty change.
///
/// A blank line inside a
/// hunk is an empty line of the file whose leading space was lost, so it becomes that context line
/// before the hunk is counted; blank lines after the last line of the diff belong to no hunk.
pub(crate) fn normalize_hunk_counts(diff: &str) -> Cow<'_, str> {
    let all = diff.split_inclusive('\n').collect::<Vec<_>>();
    let end = all.iter().rposition(|line| !blank(line)).map_or(0, |last| last + 1);
    let mut changed = end < all.len();
    let mut in_hunk = false;
    let mut lines = Vec::with_capacity(end);
    for line in &all[..end] {
        if line.starts_with("@@ ") {
            in_hunk = true;
        } else if in_hunk && blank(line) {
            lines.push(Cow::Owned(format!(" {line}")));
            changed = true;
            continue;
        } else if !matches!(line.as_bytes().first(), Some(b' ' | b'-' | b'+' | b'\\')) {
            in_hunk = false;
        }
        lines.push(Cow::Borrowed(*line));
    }
    let lines = lines.iter().map(AsRef::as_ref).collect::<Vec<&str>>();
    let mut replacements = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(header) = line.strip_prefix("@@ -") else { continue };
        let Some((old, rest)) = header.split_once(" +") else { continue };
        let Some((new, suffix)) = rest.split_once(" @@") else { continue };
        let Some((old_start, old_count)) = range(old) else { continue };
        let Some((new_start, new_count)) = range(new) else { continue };
        let Some((actual_old, actual_new)) = body_counts(&lines[index + 1..]) else { continue };
        if old_count != actual_old || new_count != actual_new {
            replacements.push((
                index,
                format!("@@ -{old_start},{actual_old} +{new_start},{actual_new} @@{suffix}"),
            ));
        }
    }
    if replacements.is_empty() && !changed {
        return Cow::Borrowed(diff);
    }
    let mut replacements = replacements.into_iter().peekable();
    let mut result = String::with_capacity(diff.len());
    for (index, line) in lines.iter().enumerate() {
        if replacements.peek().is_some_and(|(at, _)| *at == index) {
            result.push_str(&replacements.next().expect("replacement exists").1);
        } else {
            result.push_str(line);
        }
    }
    Cow::Owned(result)
}

/// A line with nothing on it but its line ending.
fn blank(line: &str) -> bool {
    line.trim_end_matches(['\r', '\n']).is_empty()
}

/// The line counts of a well-formed hunk body: only context, removed, added and
/// no-newline lines, up to the next hunk header or the end of the diff.
fn body_counts(body: &[&str]) -> Option<(usize, usize)> {
    let (mut old, mut new) = (0, 0);
    for line in body {
        match line.as_bytes().first() {
            Some(b' ') => {
                old += 1;
                new += 1;
            }
            Some(b'-') => old += 1,
            Some(b'+') => new += 1,
            Some(b'\\') if line.trim_end() == "\\ No newline at end of file" => {}
            _ if line.starts_with("@@ ") => break,
            _ => return None,
        }
    }
    (old + new > 0).then_some((old, new))
}

fn range(text: &str) -> Option<(&str, usize)> {
    let (start, count) = text.split_once(',').map_or((text, "1"), |parts| parts);
    if start.is_empty()
        || count.is_empty()
        || !start.bytes().all(|b| b.is_ascii_digit())
        || !count.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let _ = start.parse::<usize>().ok()?;
    Some((start, count.parse().ok()?))
}

pub(crate) fn parse_patch(unified_diff: &str) -> Result<Patch<'_, str>, FsToolError> {
    Patch::from_str(unified_diff).map_err(|error| FsToolError::InvalidPatch(error.to_string()))
}

/// The decode → apply → re-encode half of a patch, shared with external targets.
///
/// It is deliberately separate from the header cross-check: an external diff names
/// the absolute path the model was given, which is not a workspace path, and the
/// external target is already fixed by the verified handle and the grant, so a
/// header cannot redirect the patch anywhere. That is why the caller there refuses
/// `/dev/null` and otherwise relies on the hunks having to match the real bytes.
pub(crate) fn patched_bytes(
    path: &WorkspacePath,
    state: &ExistingState,
    patch: &Patch<'_, str>,
    unplaced: bool,
) -> Result<Vec<u8>, FsToolError> {
    let ExistingState::File { bytes, .. } = state else {
        return Err(FsToolError::InvalidRequest(format!("{} is not a file", path)));
    };
    let decoded = decode_text(bytes)?;
    let base = match decoded.line_ending {
        LineEnding::Lf | LineEnding::CrLf | LineEnding::Cr => {
            decoded.text.replace("\r\n", "\n").replace('\r', "\n")
        }
        LineEnding::Mixed | LineEnding::None => decoded.text,
    };
    let patched = apply_text(&base, patch, unplaced)?;
    encode_text(&patched, decoded.encoding, decoded.line_ending).map_err(Into::into)
}

/// The file's text with the diff applied. A hunk goes where its numbers say when its lines
/// are there; otherwise where its lines are, if that is one place (the nearest to its numbers when
/// they give one), trailing spaces aside if need be. A hunk that fits nowhere says which line
/// differs and what the file has there; a diff that changes nothing is refused, never reported done.
pub(crate) fn apply_text(
    base: &str,
    patch: &Patch<'_, str>,
    unplaced: bool,
) -> Result<String, FsToolError> {
    if patch.hunks().is_empty() {
        return Err(FsToolError::InvalidPatch(
            "Aura found no hunk in this diff; each change starts with a line such as \
             @@ -12,4 +12,6 @@ (or @@) followed by its lines"
                .into(),
        ));
    }
    let lines = base.split_inclusive('\n').collect::<Vec<_>>();
    let mut out = String::with_capacity(base.len());
    let mut cursor = 0;
    for (index, hunk) in patch.hunks().iter().enumerate() {
        let old = hunk
            .lines()
            .iter()
            .filter_map(|line| match line {
                Line::Context(text) | Line::Delete(text) => Some(*text),
                Line::Insert(_) => None,
            })
            .collect::<Vec<_>>();
        let declared = (!unplaced).then(|| hunk.old_range().start().saturating_sub(1));
        let at = place(&lines, cursor, &old, declared).map_err(|why| {
            FsToolError::InvalidPatch(format!("hunk #{} does not fit the file: {why}", index + 1))
        })?;
        lines[cursor..at].iter().for_each(|line| push_line(&mut out, line));
        let mut file_line = at;
        for line in hunk.lines() {
            match line {
                Line::Context(_) => {
                    push_line(&mut out, lines[file_line]);
                    file_line += 1;
                }
                Line::Delete(_) => file_line += 1,
                Line::Insert(text) => push_line(&mut out, text),
            }
        }
        cursor = at + old.len();
    }
    lines[cursor..].iter().for_each(|line| push_line(&mut out, line));
    if out == base {
        return Err(FsToolError::InvalidPatch(
            "the diff changes nothing in the file (no added or removed line took effect)".into(),
        ));
    }
    Ok(out)
}

/// A line after the text so far, on a line of its own.
fn push_line(out: &mut String, line: &str) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
}

/// The same line, its line ending aside, and with `loose` its trailing spaces too.
fn same(file: &str, diff: &str, loose: bool) -> bool {
    let file = file.trim_end_matches(['\r', '\n']);
    let diff = diff.trim_end_matches(['\r', '\n']);
    file == diff || loose && file.trim_end() == diff.trim_end()
}

/// Where a hunk's old lines start, at or after `cursor`.
fn place(
    lines: &[&str],
    cursor: usize,
    old: &[&str],
    declared: Option<usize>,
) -> Result<usize, String> {
    if old.is_empty() {
        return declared.filter(|at| (cursor..=lines.len()).contains(at)).ok_or_else(|| {
            "it has no unchanged or removed lines to place it by; add the lines around the change"
                .to_owned()
        });
    }
    let fits = |at: usize, loose: bool| {
        at + old.len() <= lines.len()
            && old.iter().enumerate().all(|(i, line)| same(lines[at + i], line, loose))
    };
    if let Some(at) = declared.filter(|at| *at >= cursor && fits(*at, false)) {
        return Ok(at);
    }
    for loose in [false, true] {
        let last = lines.len().saturating_sub(old.len());
        let found = (cursor..=last).filter(|at| fits(*at, loose)).collect::<Vec<_>>();
        match (found.as_slice(), declared) {
            ([], _) => {}
            ([one], _) => return Ok(*one),
            (many, Some(near)) => {
                return Ok(many.iter().copied().min_by_key(|at| at.abs_diff(near)).unwrap_or(near));
            }
            (many, None) => {
                return Err(format!(
                    "its lines appear {} times in the file; include more unchanged lines around \
                     the change",
                    many.len()
                ));
            }
        }
    }
    Err(mismatch(lines, old, declared))
}

/// Which of a hunk's lines the file does not have, and what it has there.
fn mismatch(lines: &[&str], old: &[&str], declared: Option<usize>) -> String {
    let show = |text: &str| {
        let text = text.trim_end_matches(['\r', '\n']);
        let shown = text.chars().take(160).collect::<String>();
        if shown.len() < text.len() { format!("«{shown}…»") } else { format!("«{shown}»") }
    };
    let Some(at) = declared else {
        return format!("its line {} is not in the file", show(old[0]));
    };
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

/// An external file's diff never creates or deletes the file (its target is fixed by the grant).
pub(crate) fn refuse_create_or_delete(patch: &Patch<'_, str>) -> Result<(), FsToolError> {
    if [patch.original(), patch.modified()].into_iter().flatten().any(|h| h == "/dev/null") {
        return Err(FsToolError::InvalidPatch(
            "creating or deleting a file with a patch is not supported for external paths".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_patch_target(
    path: &WorkspacePath,
    patch: &Patch<'_, str>,
    named: Option<&str>,
) -> Result<(), FsToolError> {
    for header in [patch.original(), patch.modified()].into_iter().flatten().chain(named) {
        if header == "/dev/null" {
            return Err(FsToolError::PatchTargetMismatch { path: path.clone() });
        }
        let header =
            header.strip_prefix("a/").or_else(|| header.strip_prefix("b/")).unwrap_or(header);
        let parsed = WorkspacePath::parse(header)
            .map_err(|error| FsToolError::InvalidPatch(format!("unsafe patch path: {error}")))?;
        if !names_the_file(path, &parsed) {
            return Err(FsToolError::PatchTargetMismatch { path: path.clone() });
        }
    }
    Ok(())
}

/// A header names the
/// file when its path is the end of the file's path, written from the file's own folder or one
/// above it.
fn names_the_file(path: &WorkspacePath, header: &WorkspacePath) -> bool {
    let target = path.as_str().split('/').collect::<Vec<_>>();
    let named = header.as_str().split('/').collect::<Vec<_>>();
    named.len() <= target.len()
        && target[target.len() - named.len()..]
            .iter()
            .zip(&named)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}
