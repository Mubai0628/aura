//! Core's lines about a download's target and its temporary file, which the model reads as they
//! are: the code, what was wrong, that nothing was downloaded, and what to do next. Each stays on
//! one line of at most 512 bytes; a path or a system's own words are cut to fit.
use crate::FsToolError;
use std::{fmt::Display, io::ErrorKind, path::Path};

/// The codes of the lines below; with [`super::FOLDER_LINE_PREFIX`] lines, every line Core writes
/// about where a download goes.
const CODES: &[&str] = &[
    "http.target_parent",
    "http.target_leaf",
    "http.target_exists",
    "http.target_not_file",
    "http.target_unusable",
    "http.target_changed",
    "http.parent_changed",
    "http.inconsistent_target",
    "http.staging_failed",
    "http.staging_write_failed",
    "http.staging_finished",
];

/// Whether `line` is one Core wrote about a download's target, its folders or its temporary file.
#[must_use]
pub fn is_core_line(line: &str) -> bool {
    line.starts_with(super::FOLDER_LINE_PREFIX)
        || CODES
            .iter()
            .any(|code| line.strip_prefix(code).is_some_and(|rest| rest.starts_with(": ")))
}

fn line(code: &str, sentence: impl Display) -> FsToolError {
    FsToolError::InvalidRequest(format!("{code}: {sentence}"))
}

pub(super) fn no_parent() -> FsToolError {
    line(
        "http.target_parent",
        "下载位置没有上一级文件夹，文件没有下载。请在 path 里写出文件夹和文件名。",
    )
}

pub(super) fn no_leaf() -> FsToolError {
    line("http.target_leaf", "下载位置没有文件名，文件没有下载。请在 path 里写出要保存成的文件名。")
}

pub(super) fn exists(path: &Path) -> FsToolError {
    line(
        "http.target_exists",
        format_args!(
            "{} 已经存在，文件没有下载。要用下载的文件替换它，请带上 overwrite: true 再调用；不替换就换一个文件名。",
            shown(path)
        ),
    )
}

pub(super) fn not_file(path: &Path) -> FsToolError {
    line(
        "http.target_not_file",
        format_args!(
            "{} 是文件夹或指向别处的链接，不能用下载的文件替换，文件没有下载。请换一个文件名。",
            shown(path)
        ),
    )
}

/// A download location that could not be opened or looked at, for a reason other than that it
/// does not exist.
#[must_use]
pub fn unusable(path: &Path, error: &std::io::Error) -> FsToolError {
    line(
        "http.target_unusable",
        format_args!(
            "打不开下载位置 {}（{}），文件没有下载。可以换一个下载位置。",
            shown(path),
            io_reason(error)
        ),
    )
}

pub(super) fn parent_changed() -> FsToolError {
    line(
        "http.parent_changed",
        "下载位置所在的文件夹在检查之后变了（被移动、删除或替换），文件没有下载。可以重新调用。",
    )
}

pub(super) fn target_changed(path: &Path) -> FsToolError {
    line(
        "http.target_changed",
        format_args!(
            "{} 在检查之后变了（被新建、修改或删除），文件没有下载。可以重新调用。",
            shown(path)
        ),
    )
}

pub(super) fn inconsistent() -> FsToolError {
    line("http.inconsistent_target", "下载目标的记录前后不一致，文件没有下载。可以重新调用。")
}

pub(super) fn staging_failed(folder: &Path, error: &dyn Display) -> FsToolError {
    line(
        "http.staging_failed",
        format_args!(
            "没能在 {} 里建立下载用的临时文件（{}），文件没有下载。请确认这个文件夹可以写入、磁盘空间足够后再下载。",
            shown(folder),
            detail(error)
        ),
    )
}

pub(super) fn staging_write_failed(error: &dyn Display) -> FsToolError {
    line(
        "http.staging_write_failed",
        format_args!(
            "写入下载用的临时文件失败（{}），文件没有保存。请确认磁盘空间足够后重新下载。",
            detail(error)
        ),
    )
}

pub(super) fn staging_finished() -> FsToolError {
    line(
        "http.staging_finished",
        "下载用的临时文件已经收走（这次下载已经结束或被停止），没有再写入。可以重新下载。",
    )
}

/// What went wrong opening a path, in Core's words for the usual causes.
fn io_reason(error: &std::io::Error) -> String {
    match error.kind() {
        ErrorKind::PermissionDenied => "没有权限".to_owned(),
        ErrorKind::NotADirectory => "路径里有一段不是文件夹".to_owned(),
        _ => detail(error),
    }
}

/// `path` as the model reads it: its last 120 bytes at most, without control characters.
fn shown(path: &Path) -> String {
    let text = clean(&path.display().to_string());
    if text.len() <= 120 {
        return text;
    }
    let mut start = text.len() - 119;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

/// A system's own words after Core's sentence: the first line, at most 120 bytes.
fn detail(error: &dyn Display) -> String {
    let text = error.to_string();
    let text = clean(text.lines().next().unwrap_or_default().trim());
    if text.len() <= 120 {
        return text;
    }
    let mut end = 119;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn clean(text: &str) -> String {
    text.chars().map(|c| if c.is_control() || c == '{' || c == '}' { ' ' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(error: FsToolError) -> String {
        match error {
            FsToolError::InvalidRequest(line) => line,
            other => panic!("not a line: {other}"),
        }
    }

    /// Each line names its code, says nothing was downloaded, and stays one bounded line, even
    /// for a long path or a long system message.
    #[test]
    fn a_target_line_says_what_was_wrong_and_what_to_do() {
        let long = Path::new("D:/").join("很长的文件夹".repeat(40)).join("a.zip");
        let io = std::io::Error::other(format!("{{raw}}\n{}", "x".repeat(400)));
        for line in [
            text(no_parent()),
            text(no_leaf()),
            text(exists(&long)),
            text(not_file(&long)),
            text(unusable(&long, &io)),
            text(parent_changed()),
            text(target_changed(&long)),
            text(inconsistent()),
            text(staging_failed(&long, &io)),
            text(staging_write_failed(&io)),
            text(staging_finished()),
        ] {
            assert!(is_core_line(&line), "{line}");
            assert!(
                line.contains("没有下载")
                    || line.contains("没有保存")
                    || line.contains("没有再写入")
            );
            assert!(line.len() <= 512, "{} bytes: {line}", line.len());
            assert!(!line.contains(['\n', '{', '}']), "{line}");
        }
        let exists = text(exists(Path::new("D:/w/a.zip")));
        assert!(exists.starts_with("http.target_exists: D:/w/a.zip 已经存在"), "{exists}");
        assert!(exists.contains("overwrite: true"), "{exists}");
        assert!(text(super::exists(&long)).contains("a.zip"), "the end of a long path is kept");
        let denied = std::io::Error::from(ErrorKind::PermissionDenied);
        assert!(text(unusable(Path::new("D:/w"), &denied)).contains("没有权限"));
        assert!(!is_core_line("http.target_exists"), "a bare code is not a line");
        assert!(!is_core_line("invalid fs request: http.target_exists"));
        assert!(is_core_line("http.download_folder_missing: 文件夹 x 不存在。"));
    }
}
