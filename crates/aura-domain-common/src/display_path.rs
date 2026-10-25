//! How a Windows path is spelled for people and for the shell.
use std::path::Path;

/// `path` without Windows' verbatim prefix. Core holds canonical paths as `\\?\C:\…` or
/// `\\?\UNC\host\share\…`; people read, and Explorer and the shell open, the ordinary spelling of
/// the same path.
#[must_use]
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::display_path;
    use std::path::Path;

    #[test]
    fn verbatim_paths_are_shown_in_their_ordinary_spelling() {
        let shown = |path: &str| display_path(Path::new(path));
        assert_eq!(shown(r"\\?\C:\Users\a\.memories"), r"C:\Users\a\.memories");
        assert_eq!(shown(r"\\?\UNC\host\share\m"), r"\\host\share\m");
        assert_eq!(shown(r"C:\plain"), r"C:\plain");
        assert_eq!(shown(r"\\host\share\plain"), r"\\host\share\plain");
    }
}
