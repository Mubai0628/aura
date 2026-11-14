//! A text file's lines, whatever line endings a checkout gave them. With `core.autocrlf` Git
//! checks a worktree's files out with CRLF while a file written in the main checkout may keep LF,
//! so bringing the worktree's work back compares and merges the lines and writes the result with
//! the checkout's own line endings.

/// The line endings a text file uses throughout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Endings {
    Lf,
    CrLf,
}

/// What a file's bytes are, as far as their line endings go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Not text: it has a NUL byte.
    Binary,
    /// Text with both kinds of line endings.
    Mixed,
    /// Text without a line break.
    NoBreak,
    Uniform(Endings),
}

fn kind(bytes: &[u8]) -> Kind {
    if bytes.contains(&0) {
        return Kind::Binary;
    }
    let breaks = bytes.iter().filter(|byte| **byte == b'\n').count();
    let crlf = bytes.windows(2).filter(|pair| *pair == b"\r\n").count();
    match (breaks, crlf) {
        (0, _) => Kind::NoBreak,
        (_, 0) => Kind::Uniform(Endings::Lf),
        (breaks, crlf) if breaks == crlf => Kind::Uniform(Endings::CrLf),
        _ => Kind::Mixed,
    }
}

/// The line endings to write the file with when its sides are compared by their lines: those of
/// the checkout's own file (`ours`), or, when it has no line break or is not there, of the first
/// of `others` that has one. None when a file among them is not text or mixes line endings: those
/// are compared byte for byte.
pub(super) fn to_write(ours: Option<&[u8]>, others: [Option<&[u8]>; 2]) -> Option<Endings> {
    let kinds = std::iter::once(ours).chain(others).flatten().map(kind).collect::<Vec<_>>();
    if kinds.iter().any(|kind| matches!(kind, Kind::Binary | Kind::Mixed)) {
        return None;
    }
    let first = kinds.iter().find_map(|kind| match kind {
        Kind::Uniform(endings) => Some(*endings),
        _ => None,
    });
    Some(first.unwrap_or(Endings::Lf))
}

/// `bytes` with each CRLF read as LF.
pub(super) fn to_lf(bytes: &[u8]) -> Vec<u8> {
    let mut lines = Vec::with_capacity(bytes.len());
    for (at, byte) in bytes.iter().enumerate() {
        if *byte == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
            continue;
        }
        lines.push(*byte);
    }
    lines
}

impl Endings {
    /// `lines`, read with LF, written with these line endings.
    pub(super) fn write(self, lines: Vec<u8>) -> Vec<u8> {
        match self {
            Self::Lf => lines,
            Self::CrLf => {
                let mut bytes = Vec::with_capacity(lines.len() + lines.len() / 32);
                for byte in lines {
                    if byte == b'\n' {
                        bytes.push(b'\r');
                    }
                    bytes.push(byte);
                }
                bytes
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_read_as_the_line_endings_it_uses_throughout() {
        assert_eq!(kind(b"a\nb\n"), Kind::Uniform(Endings::Lf));
        assert_eq!(kind(b"a\r\nb\r\n"), Kind::Uniform(Endings::CrLf));
        assert_eq!(kind(b"a\r\nb\n"), Kind::Mixed);
        assert_eq!(kind(b"no break"), Kind::NoBreak);
        assert_eq!(kind(b"a\0\n"), Kind::Binary);
    }

    #[test]
    fn the_checkouts_own_line_endings_are_kept_else_the_worktrees() {
        let (lf, crlf): (&[u8], &[u8]) = (b"a\nb\n", b"a\r\nb\r\n");
        assert_eq!(to_write(Some(lf), [Some(crlf), Some(crlf)]), Some(Endings::Lf));
        assert_eq!(to_write(Some(crlf), [Some(lf), None]), Some(Endings::CrLf));
        // A file the checkout lacks, or one without a line break, is written as the worktree has it.
        assert_eq!(to_write(None, [Some(crlf), None]), Some(Endings::CrLf));
        assert_eq!(to_write(Some(b"one line"), [Some(crlf), Some(lf)]), Some(Endings::CrLf));
        // Binary files and mixed line endings are compared byte for byte.
        assert_eq!(to_write(Some(lf), [Some(b"a\0"), None]), None);
        assert_eq!(to_write(Some(b"a\r\nb\n"), [Some(lf), Some(lf)]), None);
    }

    #[test]
    fn lines_read_with_lf_are_written_back_with_the_endings_chosen() {
        assert_eq!(to_lf(b"a\r\nb\r\nc"), b"a\nb\nc");
        assert_eq!(to_lf(b"a\rb\n"), b"a\rb\n", "a lone CR is part of the line");
        assert_eq!(Endings::CrLf.write(b"a\nb\n".to_vec()), b"a\r\nb\r\n");
        assert_eq!(Endings::Lf.write(b"a\nb\n".to_vec()), b"a\nb\n");
    }
}
